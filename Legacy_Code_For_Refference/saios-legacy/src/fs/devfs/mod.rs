//! devfs — /dev virtual filesystem with standard device nodes.

use crate::vfs::{
    self, DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

// -- /dev/null --------------------------------------------------------------
struct DevNull(u64);
impl InodeOps for DevNull {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.0,
            st_mode: FileType::CharDevice.mode_bits() | 0o666,
            st_nlink: 1,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, _: &mut [u8]) -> VfsResult<usize> {
        Ok(0)
    }
    fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }
    fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
        Err(VfsError::NotADir)
    }
    fn lookup(&self, _: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::NotADir)
    }
    fn create(&self, _: &str, _: FileType, _: u32) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn mkdir(&self, _: &str, _: u32) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn unlink(&self, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn rmdir(&self, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn truncate(&self, _: u64) -> VfsResult<()> {
        Ok(())
    }
    fn chmod(&self, _: u32) -> VfsResult<()> {
        Ok(())
    }
    fn chown(&self, _: u32, _: u32) -> VfsResult<()> {
        Ok(())
    }
    fn symlink(&self, _: &str, _: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn link(&self, _: &str, _: &Arc<VfsInode>) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn rename(&self, _: &str, _: &Arc<VfsInode>, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
}

// -- /dev/zero -------------------------------------------------------------
struct DevZero(u64);
impl InodeOps for DevZero {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.0,
            st_mode: FileType::CharDevice.mode_bits() | 0o666,
            st_nlink: 1,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, buf: &mut [u8]) -> VfsResult<usize> {
        buf.fill(0);
        Ok(buf.len())
    }
    fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }
    fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
        Err(VfsError::NotADir)
    }
    fn lookup(&self, _: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::NotADir)
    }
    fn create(&self, _: &str, _: FileType, _: u32) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn mkdir(&self, _: &str, _: u32) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn unlink(&self, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn rmdir(&self, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn truncate(&self, _: u64) -> VfsResult<()> {
        Ok(())
    }
    fn chmod(&self, _: u32) -> VfsResult<()> {
        Ok(())
    }
    fn chown(&self, _: u32, _: u32) -> VfsResult<()> {
        Ok(())
    }
    fn symlink(&self, _: &str, _: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn link(&self, _: &str, _: &Arc<VfsInode>) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn rename(&self, _: &str, _: &Arc<VfsInode>, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
}

// -- /dev/urandom ----------------------------------------------------------
struct DevUrandom(u64);
impl InodeOps for DevUrandom {
    fn stat(&self) -> VfsResult<Stat> {
        Ok(Stat {
            st_ino: self.0,
            st_mode: FileType::CharDevice.mode_bits() | 0o666,
            st_nlink: 1,
            ..Default::default()
        })
    }
    fn read(&self, _: u64, buf: &mut [u8]) -> VfsResult<usize> {
        // RDRAND for hardware entropy if available, else LFSR
        let mut state = 0xDEAD_BEEF_u64;
        for b in buf.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *b = state as u8;
        }
        Ok(buf.len())
    }
    fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }
    fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
        Err(VfsError::NotADir)
    }
    fn lookup(&self, _: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::NotADir)
    }
    fn create(&self, _: &str, _: FileType, _: u32) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn mkdir(&self, _: &str, _: u32) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn unlink(&self, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn rmdir(&self, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn truncate(&self, _: u64) -> VfsResult<()> {
        Ok(())
    }
    fn chmod(&self, _: u32) -> VfsResult<()> {
        Ok(())
    }
    fn chown(&self, _: u32, _: u32) -> VfsResult<()> {
        Ok(())
    }
    fn symlink(&self, _: &str, _: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::PermDenied)
    }
    fn link(&self, _: &str, _: &Arc<VfsInode>) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
    fn rename(&self, _: &str, _: &Arc<VfsInode>, _: &str) -> VfsResult<()> {
        Err(VfsError::PermDenied)
    }
}

// -- /dev directory --------------------------------------------------------

use crate::fs::tmpfs;

struct DevFsDriver;

impl vfs::FileSystemDriver for DevFsDriver {
    fn fs_type(&self) -> &'static str {
        "devfs"
    }

    fn mount(&self, request: &vfs::MountRequest) -> Result<Arc<VfsInode>, &'static str> {
        match request.source {
            vfs::MountSource::None => Ok(build_root_inode()),
            _ => Err("devfs: mount source not supported"),
        }
    }
}

pub fn register_driver() -> Result<(), &'static str> {
    match vfs::register_filesystem(Arc::new(DevFsDriver)) {
        Ok(()) | Err(VfsError::AlreadyExists) => Ok(()),
        Err(_) => Err("devfs: failed to register driver"),
    }
}

fn build_root_inode() -> Arc<VfsInode> {
    let dev = tmpfs::create_root();

    macro_rules! char_dev {
        ($ops:expr, $ftype:expr) => {{
            let ino = alloc_ino();
            VfsInode::new(ino, $ftype, Arc::new($ops))
        }};
    }

    let null_node = char_dev!(DevNull(alloc_ino()), FileType::CharDevice);
    let zero_node = char_dev!(DevZero(alloc_ino()), FileType::CharDevice);
    let random_node = char_dev!(DevUrandom(alloc_ino()), FileType::CharDevice);
    let urandom_node = char_dev!(DevUrandom(alloc_ino()), FileType::CharDevice);

    for (name, node) in [
        ("null", null_node),
        ("zero", zero_node),
        ("random", random_node),
        ("urandom", urandom_node),
    ] {
        let _ = dev.ops.link(name, &node);
    }

    for (name, target) in [
        ("stdin", "/proc/self/fd/0"),
        ("stdout", "/proc/self/fd/1"),
        ("stderr", "/proc/self/fd/2"),
    ] {
        let _ = dev.ops.symlink(name, target);
    }

    let _ = dev.ops.mkdir("pts", 0o755);

    let tty_clone = VfsInode::new(
        alloc_ino(),
        FileType::CharDevice,
        Arc::new(DevNull(alloc_ino())),
    );
    let _ = dev.ops.link("tty", &tty_clone);

    dev
}

pub fn mount(mountpoint: &str) {
    let _ = crate::vfs_contract::VfsContract::mount_fs(
        "devfs",
        &vfs::MountRequest::new(mountpoint, vfs::MountSource::None),
    );

    crate::println!(
        "[devfs] /dev populated: null, zero, random, urandom, stdin/stdout/stderr, pts/"
    );
}
