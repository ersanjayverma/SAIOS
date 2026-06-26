//! tmpfs — in-memory filesystem backed by the kernel heap.
//! Used for /tmp, /run, /dev/shm.

use crate::vfs::{
    self, DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

// -- Node types -------------------------------------------------------------

enum NodeData {
    File(Vec<u8>),
    Dir(BTreeMap<String, Arc<VfsInode>>),
    Symlink(String),
}

struct TmpNode {
    ino: u64,
    ftype: FileType,
    mode: Mutex<u32>,
    uid: Mutex<u32>,
    gid: Mutex<u32>,
    data: Mutex<NodeData>,
    mtime: u64,
    size: Mutex<u64>,
}

impl TmpNode {
    fn current_owner() -> (u32, u32) {
        let (_, _, euid, egid) = crate::user::get_current_credentials();
        (euid, egid)
    }

    fn new_dir(mode: u32) -> Arc<Self> {
        let ino = alloc_ino();
        let (uid, gid) = Self::current_owner();
        Arc::new(Self {
            ino,
            ftype: FileType::Directory,
            mode: Mutex::new(mode),
            uid: Mutex::new(uid),
            gid: Mutex::new(gid),
            data: Mutex::new(NodeData::Dir(BTreeMap::new())),
            mtime: 0,
            size: Mutex::new(0),
        })
    }
    fn new_file(mode: u32) -> Arc<Self> {
        let ino = alloc_ino();
        let (uid, gid) = Self::current_owner();
        Arc::new(Self {
            ino,
            ftype: FileType::RegularFile,
            mode: Mutex::new(mode),
            uid: Mutex::new(uid),
            gid: Mutex::new(gid),
            data: Mutex::new(NodeData::File(Vec::new())),
            mtime: 0,
            size: Mutex::new(0),
        })
    }
    fn new_symlink(target: &str) -> Arc<Self> {
        let ino = alloc_ino();
        let len = target.len() as u64;
        let (uid, gid) = Self::current_owner();
        Arc::new(Self {
            ino,
            ftype: FileType::SymLink,
            mode: Mutex::new(0o777),
            uid: Mutex::new(uid),
            gid: Mutex::new(gid),
            data: Mutex::new(NodeData::Symlink(String::from(target))),
            mtime: 0,
            size: Mutex::new(len),
        })
    }
}

struct TmpNodeOps(Arc<TmpNode>);

impl InodeOps for TmpNodeOps {
    fn stat(&self) -> VfsResult<Stat> {
        let n = &self.0;
        let size = *n.size.lock();
        let mode = *n.mode.lock();
        Ok(Stat {
            st_ino: n.ino,
            st_mode: n.ftype.mode_bits() | mode,
            st_uid: *n.uid.lock(),
            st_gid: *n.gid.lock(),
            st_size: size as i64,
            st_blksize: 4096,
            st_blocks: size.div_ceil(512) as i64,
            st_nlink: 1,
            ..Default::default()
        })
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let data = self.0.data.lock();
        if let NodeData::File(ref v) = *data {
            let off = offset as usize;
            if off >= v.len() {
                return Ok(0);
            }
            let n = buf.len().min(v.len() - off);
            buf[..n].copy_from_slice(&v[off..off + n]);
            Ok(n)
        } else {
            Err(VfsError::IsDir)
        }
    }

    fn write(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        let mut data = self.0.data.lock();
        if let NodeData::File(ref mut v) = *data {
            let end = offset as usize + buf.len();
            if end > v.len() {
                v.resize(end, 0);
            }
            v[offset as usize..end].copy_from_slice(buf);
            *self.0.size.lock() = v.len() as u64;
            Ok(buf.len())
        } else {
            Err(VfsError::IsDir)
        }
    }

    fn truncate(&self, size: u64) -> VfsResult<()> {
        let mut data = self.0.data.lock();
        if let NodeData::File(ref mut v) = *data {
            v.resize(size as usize, 0);
            *self.0.size.lock() = size;
            Ok(())
        } else {
            Err(VfsError::IsDir)
        }
    }

    fn readdir(&self, _offset: u64) -> VfsResult<Vec<DirEntry>> {
        let data = self.0.data.lock();
        if let NodeData::Dir(ref map) = *data {
            Ok(map
                .iter()
                .map(|(name, inode)| DirEntry {
                    name: name.clone(),
                    inode: inode.ino,
                    ftype: inode.ftype,
                })
                .collect())
        } else {
            Err(VfsError::NotADir)
        }
    }

    fn lookup(&self, name: &str) -> VfsResult<Arc<VfsInode>> {
        let data = self.0.data.lock();
        if let NodeData::Dir(ref map) = *data {
            map.get(name).cloned().ok_or(VfsError::NotFound)
        } else {
            Err(VfsError::NotADir)
        }
    }

    fn create(&self, name: &str, ftype: FileType, mode: u32) -> VfsResult<Arc<VfsInode>> {
        let node = match ftype {
            FileType::RegularFile => TmpNode::new_file(mode),
            FileType::Directory => TmpNode::new_dir(mode),
            _ => return Err(VfsError::NotSupported),
        };
        let vnode = VfsInode::new(node.ino, ftype, Arc::new(TmpNodeOps(node.clone())));
        let mut data = self.0.data.lock();
        if let NodeData::Dir(ref mut map) = *data {
            if map.contains_key(name) {
                return Err(VfsError::AlreadyExists);
            }
            map.insert(String::from(name), vnode.clone());
            Ok(vnode)
        } else {
            Err(VfsError::NotADir)
        }
    }

    fn mkdir(&self, name: &str, mode: u32) -> VfsResult<Arc<VfsInode>> {
        self.create(name, FileType::Directory, mode)
    }

    fn unlink(&self, name: &str) -> VfsResult<()> {
        let mut data = self.0.data.lock();
        if let NodeData::Dir(ref mut map) = *data {
            map.remove(name).map(|_| ()).ok_or(VfsError::NotFound)
        } else {
            Err(VfsError::NotADir)
        }
    }

    fn rmdir(&self, name: &str) -> VfsResult<()> {
        self.unlink(name)
    }

    fn symlink(&self, name: &str, target: &str) -> VfsResult<Arc<VfsInode>> {
        let node = TmpNode::new_symlink(target);
        let vnode = VfsInode::new(node.ino, FileType::SymLink, Arc::new(TmpNodeOps(node)));
        let mut data = self.0.data.lock();
        if let NodeData::Dir(ref mut map) = *data {
            map.insert(String::from(name), vnode.clone());
            Ok(vnode)
        } else {
            Err(VfsError::NotADir)
        }
    }

    fn link(&self, name: &str, target: &Arc<VfsInode>) -> VfsResult<()> {
        let mut data = self.0.data.lock();
        if let NodeData::Dir(ref mut map) = *data {
            map.insert(String::from(name), target.clone());
            Ok(())
        } else {
            Err(VfsError::NotADir)
        }
    }

    fn rename(&self, old: &str, new_parent: &Arc<VfsInode>, new: &str) -> VfsResult<()> {
        let node = self.lookup(old)?;
        new_parent.ops.link(new, &node)?;
        self.unlink(old)
    }

    fn readlink(&self) -> VfsResult<String> {
        let data = self.0.data.lock();
        if let NodeData::Symlink(ref s) = *data {
            Ok(s.clone())
        } else {
            Err(VfsError::InvalidArg)
        }
    }

    fn chmod(&self, mode: u32) -> VfsResult<()> {
        *self.0.mode.lock() = mode & 0o7777;
        Ok(())
    }
    fn chown(&self, u: u32, g: u32) -> VfsResult<()> {
        *self.0.uid.lock() = u;
        *self.0.gid.lock() = g;
        Ok(())
    }
}

struct TmpFsDriver;

impl vfs::FileSystemDriver for TmpFsDriver {
    fn fs_type(&self) -> &'static str {
        "tmpfs"
    }

    fn mount(&self, request: &vfs::MountRequest) -> Result<Arc<VfsInode>, &'static str> {
        match request.source {
            vfs::MountSource::None => Ok(create_root()),
            _ => Err("tmpfs: mount source not supported"),
        }
    }
}

pub fn register_driver() -> Result<(), &'static str> {
    match vfs::register_filesystem(Arc::new(TmpFsDriver)) {
        Ok(()) | Err(VfsError::AlreadyExists) => Ok(()),
        Err(_) => Err("tmpfs: failed to register driver"),
    }
}

pub(crate) fn create_root() -> Arc<VfsInode> {
    let root_node = TmpNode::new_dir(0o755);
    VfsInode::new(
        root_node.ino,
        FileType::Directory,
        Arc::new(TmpNodeOps(root_node)),
    )
}

/// Create and mount a new tmpfs instance.
pub fn mount(mountpoint: &str) {
    let _ = crate::vfs_contract::VfsContract::mount_fs(
        "tmpfs",
        &vfs::MountRequest::new(mountpoint, vfs::MountSource::None),
    );
}
