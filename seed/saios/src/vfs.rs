//! Virtual filesystem (VFS) layer.
//!
//! The VFS provides a tree of in-memory nodes backed by a simple tmpfs
//! implementation. It supports path resolution, file open/read/write/seek,
//! directory creation and mount-point tracking.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::driver::storage;
use crate::kernel::device as kernel_device;
use crate::object_manager;

/// Type of a VFS node.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FileType {
    Directory,
    File,
}

#[derive(Debug, Clone)]
pub struct VNode {
    /// Inode number.
    pub inode: u64,
    /// File or directory name.
    pub name: String,
    /// Node type.
    pub kind: FileType,
}

/// File descriptor identifier used by the VFS.
pub type VfsFd = u32;

#[derive(Debug, Copy, Clone)]
pub enum SeekFrom {
    /// Seek relative to the start of the file.
    Start(usize),
    /// Seek relative to the current offset.
    Current(isize),
    /// Seek relative to the end of the file.
    End(isize),
}

/// Flags controlling how a file is opened.
#[derive(Debug, Copy, Clone)]
pub struct OpenOptions {
    /// Open for reading.
    pub read: bool,
    /// Open for writing.
    pub write: bool,
    /// Create the file if it does not exist.
    pub create: bool,
    /// Truncate the file to zero length.
    pub truncate: bool,
    /// Append writes to the end of the file.
    pub append: bool,
}

impl OpenOptions {
    /// Returns options for read-only access.
    pub const fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            create: false,
            truncate: false,
            append: false,
        }
    }

    /// Returns options for write-only access, creating the file if needed.
    pub const fn write_only_create() -> Self {
        Self {
            read: false,
            write: true,
            create: true,
            truncate: true,
            append: false,
        }
    }

    /// Returns options for read/write access, creating the file if needed.
    pub const fn read_write_create() -> Self {
        Self {
            read: true,
            write: true,
            create: true,
            truncate: false,
            append: false,
        }
    }

    /// Returns options for append-only access, creating the file if needed.
    pub const fn append_create() -> Self {
        Self {
            read: false,
            write: true,
            create: true,
            truncate: false,
            append: true,
        }
    }
}

#[derive(Debug, Clone)]
struct OpenFile {
    inode: u64,
    offset: usize,
    readable: bool,
    writable: bool,
    append: bool,
}

#[derive(Debug, Clone)]
pub struct MountRecord {
    /// Absolute mount path.
    pub path: String,
    /// Name of the mounted filesystem.
    pub fs_name: String,
    /// True if the mount is read-only.
    pub read_only: bool,
}

#[derive(Clone)]
struct Node {
    inode: u64,
    name: String,
    kind: FileType,
    parent: Option<u64>,
    children: Vec<u64>,
    data: Vec<u8>,
}

pub trait FileSystem {
    fn create(&mut self, path: &str) -> Result<(), &'static str>;
    fn open_node(&self, path: &str) -> Result<VNode, &'static str>;
    fn read(&self, path: &str) -> Result<Vec<u8>, &'static str>;
    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str>;
    fn remove(&mut self, path: &str) -> Result<(), &'static str>;
    fn mkdir(&mut self, path: &str) -> Result<(), &'static str>;
    fn readdir(&self, path: &str) -> Result<Vec<String>, &'static str>;
}

struct TmpFs {
    nodes: Vec<Option<Node>>,
    root: u64,
    cwd: u64,
}

impl TmpFs {
    fn new() -> Self {
        let mut nodes = Vec::new();
        let root = Node {
            inode: 1,
            name: "/".to_string(),
            kind: FileType::Directory,
            parent: None,
            children: Vec::new(),
            data: Vec::new(),
        };
        nodes.push(Some(root));
        Self {
            nodes,
            root: 1,
            cwd: 1,
        }
    }

    fn inode_to_index(inode: u64) -> Option<usize> {
        inode.checked_sub(1).map(|v| v as usize)
    }

    fn node(&self, inode: u64) -> Result<&Node, &'static str> {
        let idx = Self::inode_to_index(inode).ok_or("invalid inode")?;
        self.nodes
            .get(idx)
            .and_then(|n| n.as_ref())
            .ok_or("node missing")
    }

    fn node_mut(&mut self, inode: u64) -> Result<&mut Node, &'static str> {
        let idx = Self::inode_to_index(inode).ok_or("invalid inode")?;
        self.nodes
            .get_mut(idx)
            .and_then(|n| n.as_mut())
            .ok_or("node missing")
    }

    fn next_inode(&self) -> u64 {
        (self.nodes.len() as u64) + 1
    }

    fn path_parts(path: &str) -> impl Iterator<Item = &str> {
        path.split('/').filter(|p| !p.is_empty())
    }

    fn lookup_child_by_name(&self, dir_inode: u64, name: &str) -> Option<u64> {
        let dir = self.node(dir_inode).ok()?;
        if dir.kind != FileType::Directory {
            return None;
        }

        for &child_inode in &dir.children {
            if let Ok(child) = self.node(child_inode)
                && child.name == name
            {
                return Some(child_inode);
            }
        }
        None
    }

    fn resolve_from(&self, start: u64, path: &str) -> Result<u64, &'static str> {
        if path == "/" {
            return Ok(self.root);
        }

        let mut current = if path.starts_with('/') {
            self.root
        } else {
            start
        };

        for part in Self::path_parts(path) {
            match part {
                // Current directory: no change.
                "." => {}
                ".." => {
                    let parent = self.node(current)?.parent;
                    if let Some(p) = parent {
                        current = p;
                    }
                }
                name => {
                    current = self
                        .lookup_child_by_name(current, name)
                        .ok_or("path not found")?;
                }
            }
        }

        Ok(current)
    }

    fn resolve(&self, path: &str) -> Result<u64, &'static str> {
        self.resolve_from(self.cwd, path)
    }

    fn resolve_parent_and_name(&self, path: &str) -> Result<(u64, String), &'static str> {
        let mut parts: Vec<&str> = Self::path_parts(path).collect();
        let name = parts.pop().ok_or("missing name")?;
        if name == "." || name == ".." {
            return Err("invalid name");
        }

        let parent_path = if path.starts_with('/') {
            if parts.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", parts.join("/"))
            }
        } else {
            parts.join("/")
        };

        let parent = if parent_path.is_empty() {
            self.cwd
        } else {
            self.resolve(&parent_path)?
        };

        Ok((parent, name.to_string()))
    }

    fn insert_node(
        &mut self,
        parent_inode: u64,
        name: String,
        kind: FileType,
    ) -> Result<u64, &'static str> {
        let parent = self.node(parent_inode)?;
        if parent.kind != FileType::Directory {
            return Err("parent is not a directory");
        }
        if self.lookup_child_by_name(parent_inode, &name).is_some() {
            return Err("already exists");
        }

        let inode = self.next_inode();
        let node = Node {
            inode,
            name,
            kind,
            parent: Some(parent_inode),
            children: Vec::new(),
            data: Vec::new(),
        };

        self.nodes.push(Some(node));
        self.node_mut(parent_inode)?.children.push(inode);
        Ok(inode)
    }

    fn remove_inode(&mut self, inode: u64) -> Result<(), &'static str> {
        if inode == self.root {
            return Err("cannot remove root");
        }

        let node = self.node(inode)?.clone();
        if node.kind == FileType::Directory && !node.children.is_empty() {
            return Err("directory not empty");
        }

        if let Some(parent_inode) = node.parent {
            let parent = self.node_mut(parent_inode)?;
            parent.children.retain(|&c| c != inode);
        }

        let idx = Self::inode_to_index(inode).ok_or("invalid inode")?;
        if let Some(slot) = self.nodes.get_mut(idx) {
            *slot = None;
        }

        if self.cwd == inode {
            self.cwd = self.root;
        }

        Ok(())
    }

    fn cwd_path(&self) -> String {
        self.path_for_inode(self.cwd)
    }

    fn path_for_inode(&self, inode: u64) -> String {
        if inode == self.root {
            return "/".to_string();
        }

        let mut parts: Vec<String> = Vec::new();
        let mut current = inode;

        while let Ok(node) = self.node(current) {
            if current == self.root {
                break;
            }

            parts.push(node.name.clone());
            current = node.parent.unwrap_or(self.root);
            if current == self.root {
                break;
            }
        }

        parts.reverse();
        format!("/{}", parts.join("/"))
    }

    fn normalized_path(&self, path: &str) -> String {
        let mut parts: Vec<String> = Vec::new();

        if !path.starts_with('/') {
            let cwd = self.cwd_path();
            for p in cwd.split('/').filter(|p| !p.is_empty()) {
                parts.push(p.to_string());
            }
        }

        for p in Self::path_parts(path) {
            match p {
                // Current directory: no change.
                "." => {}
                ".." => {
                    let _ = parts.pop();
                }
                name => parts.push(name.to_string()),
            }
        }

        if parts.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", parts.join("/"))
        }
    }

    fn rename_path(&mut self, from: &str, to: &str) -> Result<(), &'static str> {
        let inode = self.resolve(from)?;
        if inode == self.root {
            return Err("cannot rename root");
        }

        let (new_parent, new_name) = self.resolve_parent_and_name(to)?;
        if self.lookup_child_by_name(new_parent, &new_name).is_some() {
            return Err("destination exists");
        }

        let old_parent = self.node(inode)?.parent.ok_or("missing parent")?;
        {
            let parent = self.node_mut(old_parent)?;
            parent.children.retain(|&c| c != inode);
        }
        {
            let parent = self.node_mut(new_parent)?;
            parent.children.push(inode);
        }

        let node = self.node_mut(inode)?;
        node.name = new_name;
        node.parent = Some(new_parent);
        Ok(())
    }

    fn read_inode_range(
        &self,
        inode: u64,
        offset: usize,
        max_len: usize,
    ) -> Result<Vec<u8>, &'static str> {
        let node = self.node(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }
        if offset >= node.data.len() {
            return Ok(Vec::new());
        }

        let end = core::cmp::min(node.data.len(), offset.saturating_add(max_len));
        Ok(node.data[offset..end].to_vec())
    }

    fn write_inode_at(
        &mut self,
        inode: u64,
        offset: usize,
        data: &[u8],
    ) -> Result<usize, &'static str> {
        let node = self.node_mut(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }

        let need_len = offset.saturating_add(data.len());
        if need_len > node.data.len() {
            node.data.resize(need_len, 0);
        }

        node.data[offset..offset + data.len()].copy_from_slice(data);
        Ok(data.len())
    }

    fn truncate_inode(&mut self, inode: u64) -> Result<(), &'static str> {
        let node = self.node_mut(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }
        node.data.clear();
        Ok(())
    }

    fn inode_len(&self, inode: u64) -> Result<usize, &'static str> {
        let node = self.node(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }
        Ok(node.data.len())
    }
}

impl FileSystem for TmpFs {
    fn create(&mut self, path: &str) -> Result<(), &'static str> {
        let (parent, name) = self.resolve_parent_and_name(path)?;
        self.insert_node(parent, name, FileType::File)?;
        Ok(())
    }

    fn open_node(&self, path: &str) -> Result<VNode, &'static str> {
        let inode = self.resolve(path)?;
        let node = self.node(inode)?;
        Ok(VNode {
            inode: node.inode,
            name: node.name.clone(),
            kind: node.kind,
        })
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, &'static str> {
        let inode = self.resolve(path)?;
        let node = self.node(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }
        Ok(node.data.clone())
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str> {
        let inode = self.resolve(path)?;
        let node = self.node_mut(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }
        node.data.clear();
        node.data.extend_from_slice(data);
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), &'static str> {
        let inode = self.resolve(path)?;
        self.remove_inode(inode)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), &'static str> {
        let (parent, name) = self.resolve_parent_and_name(path)?;
        self.insert_node(parent, name, FileType::Directory)?;
        Ok(())
    }

    fn readdir(&self, path: &str) -> Result<Vec<String>, &'static str> {
        let inode = if path.is_empty() {
            self.cwd
        } else {
            self.resolve(path)?
        };
        let node = self.node(inode)?;
        if node.kind != FileType::Directory {
            return Err("not a directory");
        }

        let mut out = Vec::new();
        for &child_inode in &node.children {
            let child = self.node(child_inode)?;
            out.push(child.name.clone());
        }
        out.sort();
        Ok(out)
    }
}

struct VfsState {
    fs: TmpFs,
    open_files: Vec<Option<OpenFile>>,
    mounts: Vec<MountRecord>,
}

impl VfsState {
    fn new() -> Self {
        let mut fs = TmpFs::new();
        seed_standard_tree(&mut fs);
        Self {
            fs,
            open_files: Vec::new(),
            mounts: vec![MountRecord {
                path: "/".to_string(),
                fs_name: "tmpfs".to_string(),
                read_only: false,
            }],
        }
    }

    fn alloc_fd(&mut self, file: OpenFile) -> VfsFd {
        if let Some((idx, slot)) = self
            .open_files
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(file);
            return idx as VfsFd;
        }

        self.open_files.push(Some(file));
        (self.open_files.len() - 1) as VfsFd
    }

    fn get_open_file(&self, fd: VfsFd) -> Result<&OpenFile, &'static str> {
        self.open_files
            .get(fd as usize)
            .and_then(|slot| slot.as_ref())
            .ok_or("bad file descriptor")
    }

    fn get_open_file_mut(&mut self, fd: VfsFd) -> Result<&mut OpenFile, &'static str> {
        self.open_files
            .get_mut(fd as usize)
            .and_then(|slot| slot.as_mut())
            .ok_or("bad file descriptor")
    }

    fn close_fd(&mut self, fd: VfsFd) -> Result<(), &'static str> {
        let slot = self
            .open_files
            .get_mut(fd as usize)
            .ok_or("bad file descriptor")?;
        if slot.is_none() {
            return Err("bad file descriptor");
        }
        *slot = None;
        Ok(())
    }

    fn invalidate_inode_descriptors(&mut self, inode: u64) {
        for slot in &mut self.open_files {
            if let Some(open) = slot
                && open.inode == inode
            {
                *slot = None;
            }
        }
    }
}

fn seed_standard_tree(fs: &mut TmpFs) {
    let roots = [
        "/system", "/dev", "/proc", "/sys", "/home", "/tmp", "/var", "/boot", "/bin", "/usr",
        "/etc", "/mnt",
    ];
    for path in roots {
        let _ = fs.mkdir(path);
    }

    let sys_nodes = [
        "/sys/devices",
        "/sys/drivers",
        "/sys/memory",
        "/sys/scheduler",
        "/sys/storage",
        "/sys/network",
        "/sys/services",
    ];
    for path in sys_nodes {
        let _ = fs.mkdir(path);
    }

    let user_programs = [
        "hello", "true", "false", "argc", "env", "fail", "ls", "cat", "cp", "mv", "rm", "mkdir",
        "ps", "kill", "top", "uname", "calc", "stress", "cc",
    ];
    for name in user_programs {
        let _ = fs.create(format!("/bin/{}", name).as_str());
    }
}

fn is_sys_path(path: &str) -> bool {
    path == "/sys" || path.starts_with("/sys/")
}

fn is_storage_backed(path: &str) -> bool {
    storage::mounted_volume_for_path(path).is_some_and(|v| v.name != "tmpfs")
}

fn storage_node_name(path: &str) -> String {
    if path == "/" {
        return "/".to_string();
    }
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("/")
        .to_string()
}

fn storage_inode(path: &str) -> u64 {
    let mut h = 1469598103934665603u64;
    for b in path.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

static VFS: StaticCell<Option<VfsState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_vfs<R>(f: impl FnOnce(&mut VfsState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *VFS.get();
            if slot.is_none() {
                *slot = Some(VfsState::new());
            }
            slot.as_mut().expect("vfs not initialized")
        };
        f(state)
    };
    unlock();
    out
}

/// Initializes the VFS subsystem.
pub fn init() {
    with_vfs(|_| {});
}

/// Creates a directory at `path`.
pub fn mkdir(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if is_sys_path(&abs) {
            return Err("read-only virtual path");
        }

        if is_storage_backed(&abs) {
            return storage::fs_mkdir(&abs);
        }

        vfs.fs.mkdir(path)?;
        object_manager::log_event(&format!("Directory created: {}", abs));
        Ok(())
    })
}

/// Creates an empty file at `path`.
pub fn touch(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if is_sys_path(&abs) {
            return Err("read-only virtual path");
        }

        if is_storage_backed(&abs) {
            return storage::fs_create(&abs);
        }

        vfs.fs.create(path)?;
        object_manager::log_event(&format!("File created: {}", abs));
        Ok(())
    })
}

/// Records a mount of `fs_name` at `path`.
pub fn mount(path: &str, fs_name: &str, read_only: bool) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        let inode = vfs.fs.resolve(&abs)?;
        if vfs.fs.node(inode)?.kind != FileType::Directory {
            return Err("mount target is not a directory");
        }

        if vfs.mounts.iter().any(|m| m.path == abs) {
            return Err("already mounted");
        }

        vfs.mounts.push(MountRecord {
            path: abs.clone(),
            fs_name: fs_name.to_string(),
            read_only,
        });
        object_manager::log_event(&format!("Mounted {} on {}", fs_name, abs));
        Ok(())
    })
}

/// Returns a snapshot of all recorded mount points.
pub fn mounts() -> Vec<MountRecord> {
    with_vfs(|vfs| vfs.mounts.clone())
}

/// Remove a mount record registered at `path`.  The root mount (`/`) cannot
/// be unmounted.  The directory node itself is left in place.
pub fn umount(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if abs == "/" {
            return Err("cannot unmount root filesystem");
        }
        let before = vfs.mounts.len();
        vfs.mounts.retain(|m| m.path != abs);
        if vfs.mounts.len() == before {
            return Err("not mounted");
        }
        object_manager::log_event(&format!("Unmounted {}", abs));
        Ok(())
    })
}

/// Opens the file at `path` with the given options and returns a file
/// descriptor.
pub fn open(path: &str, options: OpenOptions) -> Result<VfsFd, &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if !options.read && !options.write {
            return Err("open requires read or write access");
        }

        if is_sys_path(&abs)
            && (options.write || options.create || options.truncate || options.append)
        {
            return Err("read-only virtual path");
        }

        if options.create && !is_sys_path(&abs) && vfs.fs.resolve(path).is_err() {
            vfs.fs.create(path)?;
        }

        let inode = vfs.fs.resolve(path)?;
        let node = vfs.fs.node(inode)?;
        if node.kind != FileType::File {
            return Err("not a file");
        }

        if options.truncate {
            if !options.write {
                return Err("truncate requires write access");
            }
            vfs.fs.truncate_inode(inode)?;
        }

        let offset = if options.append {
            vfs.fs.inode_len(inode)?
        } else {
            0
        };

        Ok(vfs.alloc_fd(OpenFile {
            inode,
            offset,
            readable: options.read,
            writable: options.write,
            append: options.append,
        }))
    })
}

/// Closes a file descriptor previously returned by [`open`].
pub fn close(fd: VfsFd) -> Result<(), &'static str> {
    with_vfs(|vfs| vfs.close_fd(fd))
}

/// Reads up to `max_len` bytes from the file descriptor.
pub fn read(fd: VfsFd, max_len: usize) -> Result<Vec<u8>, &'static str> {
    with_vfs(|vfs| {
        let (inode, offset, readable) = {
            let of = vfs.get_open_file(fd)?;
            (of.inode, of.offset, of.readable)
        };

        if !readable {
            return Err("file descriptor is not readable");
        }

        let data = vfs.fs.read_inode_range(inode, offset, max_len)?;
        let new_offset = offset.saturating_add(data.len());
        vfs.get_open_file_mut(fd)?.offset = new_offset;
        Ok(data)
    })
}

/// Writes `data` to the file descriptor and returns the number of bytes
/// written.
pub fn write(fd: VfsFd, data: &[u8]) -> Result<usize, &'static str> {
    with_vfs(|vfs| {
        let (inode, mut offset, writable, append) = {
            let of = vfs.get_open_file(fd)?;
            (of.inode, of.offset, of.writable, of.append)
        };

        if !writable {
            return Err("file descriptor is not writable");
        }

        if append {
            offset = vfs.fs.inode_len(inode)?;
        }

        let written = vfs.fs.write_inode_at(inode, offset, data)?;
        vfs.get_open_file_mut(fd)?.offset = offset.saturating_add(written);
        Ok(written)
    })
}

/// Repositions the file descriptor's offset according to `from`.
pub fn seek(fd: VfsFd, from: SeekFrom) -> Result<usize, &'static str> {
    with_vfs(|vfs| {
        let (inode, current) = {
            let of = vfs.get_open_file(fd)?;
            (of.inode, of.offset)
        };
        let len = vfs.fs.inode_len(inode)? as isize;

        let target = match from {
            SeekFrom::Start(pos) => pos as isize,
            SeekFrom::Current(delta) => (current as isize).saturating_add(delta),
            SeekFrom::End(delta) => len.saturating_add(delta),
        };

        let clamped = if target < 0 {
            0
        } else if target > len {
            len as usize
        } else {
            target as usize
        };

        vfs.get_open_file_mut(fd)?.offset = clamped;
        Ok(clamped)
    })
}

/// Lists the entries in `path`, or the current directory if `path` is None.
pub fn ls(path: Option<&str>) -> Result<Vec<String>, &'static str> {
    with_vfs(|vfs| {
        let req = path.unwrap_or(".");
        let abs = vfs.fs.normalized_path(req);
        if is_sys_path(&abs) {
            return object_manager::sys_readdir(&abs).ok_or("not a directory");
        }

        if is_storage_backed(&abs) {
            return storage::fs_readdir(&abs);
        }

        if abs == "/dev" {
            let mut merged = vfs.fs.readdir(req)?;
            for dev in kernel_device::devices() {
                if let Some(name) = dev.name.strip_prefix("/dev/") {
                    merged.push(name.to_string());
                }
            }
            merged.sort();
            merged.dedup();
            return Ok(merged);
        }

        vfs.fs.readdir(req)
    })
}

/// Reads the entire file at `path` as a UTF-8 string.
pub fn cat(path: &str) -> Result<String, &'static str> {
    let bytes = read_path(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Reads the entire file at `path` as raw bytes.
pub fn read_path(path: &str) -> Result<Vec<u8>, &'static str> {
    let abs = with_vfs(|vfs| vfs.fs.normalized_path(path));
    if is_sys_path(&abs) {
        let lines = object_manager::sys_read(&abs).ok_or("not a file")?;
        return Ok(lines.join("\n").into_bytes());
    }

    if is_storage_backed(&abs) {
        return storage::fs_read(&abs);
    }

    let fd = open(path, OpenOptions::read_only())?;
    let read_result = read(fd, usize::MAX);
    let close_result = close(fd);
    let bytes = read_result?;
    close_result?;
    Ok(bytes)
}

/// Removes the file or directory at `path`.
pub fn rm(path: &str) -> Result<(), &'static str> {
    unlink(path)
}

/// Removes the file or directory at `path`.
pub fn unlink(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs = vfs.fs.normalized_path(path);
        if is_sys_path(&abs) {
            return Err("read-only virtual path");
        }

        if is_storage_backed(&abs) {
            return storage::fs_delete(&abs);
        }

        let inode = vfs.fs.resolve(path)?;
        vfs.invalidate_inode_descriptors(inode);
        vfs.fs.remove(path)?;
        object_manager::log_event(&format!("Object removed: {}", abs));
        Ok(())
    })
}

/// Renames or moves `from` to `to`.
pub fn rename(from: &str, to: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let abs_from = vfs.fs.normalized_path(from);
        let abs_to = vfs.fs.normalized_path(to);
        if is_sys_path(&abs_from) || is_sys_path(&abs_to) {
            return Err("read-only virtual path");
        }

        if is_storage_backed(&abs_from) || is_storage_backed(&abs_to) {
            return storage::fs_rename(&abs_from, &abs_to);
        }

        vfs.fs.rename_path(from, to)?;
        object_manager::log_event(&format!("Renamed {} -> {}", abs_from, abs_to));
        Ok(())
    })
}

/// Changes the current working directory to `path`.
pub fn cd(path: &str) -> Result<(), &'static str> {
    with_vfs(|vfs| {
        let inode = vfs.fs.resolve(path)?;
        let node = vfs.fs.node(inode)?;
        if node.kind != FileType::Directory {
            return Err("not a directory");
        }
        vfs.fs.cwd = inode;
        Ok(())
    })
}

/// Returns the current working directory.
pub fn pwd() -> String {
    with_vfs(|vfs| vfs.fs.cwd_path())
}

/// Writes `data` to the file at `path`, creating it if necessary.
pub fn write_path(path: &str, data: &[u8]) -> Result<(), &'static str> {
    let abs = with_vfs(|vfs| vfs.fs.normalized_path(path));
    if is_storage_backed(&abs) {
        return storage::fs_write(&abs, data);
    }

    let fd = open(path, OpenOptions::write_only_create())?;
    let write_result = write(fd, data);
    let close_result = close(fd);
    write_result?;
    close_result
}

/// Returns the VFS node for `path` without opening it.
pub fn open_node(path: &str) -> Result<VNode, &'static str> {
    let abs = with_vfs(|vfs| vfs.fs.normalized_path(path));
    if is_storage_backed(&abs) {
        let stat = storage::fs_stat(&abs)?;
        return Ok(VNode {
            inode: storage_inode(&abs),
            name: storage_node_name(&abs),
            kind: match stat.kind {
                storage::FsNodeKind::File => FileType::File,
                storage::FsNodeKind::Directory => FileType::Directory,
            },
        });
    }

    with_vfs(|vfs| vfs.fs.open_node(path))
}
