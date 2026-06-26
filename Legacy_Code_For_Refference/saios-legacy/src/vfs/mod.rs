//! Virtual Filesystem Switch — the single interface all filesystems implement.
//!
//! Architecture:
//!   Inode   — a file/directory/device identity (may be cached)
//!   Dentry  — a name ↔ inode mapping (directory entry cache)
//!   File    — an open file description (offset, flags, inode)
//!   Superblock — a mounted filesystem instance
//!   VfsOps  — trait every filesystem must implement

pub mod file;
pub mod mount_namespace;
pub mod namespace;
pub mod path;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

// -- Error type -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    NotADir,
    NotAFile,
    IsDir,
    PermDenied,
    NoSpace,
    AlreadyExists,
    NotEmpty,
    InvalidArg,
    Io,
    NotSupported,
    TooManyOpen,
    BadFd,
    Interrupted,
    WouldBlock,
    BrokenPipe,
    Loop, // symlink loop
    NameTooLong,
    CrossDevice,
    NoEntry,
}

impl VfsError {
    /// Convert to a Linux errno value (negative).
    pub fn to_errno(self) -> i64 {
        match self {
            Self::NotFound => -2,       // ENOENT
            Self::NotADir => -20,       // ENOTDIR
            Self::IsDir => -21,         // EISDIR
            Self::NotAFile => -22,      // EINVAL
            Self::PermDenied => -13,    // EACCES
            Self::NoSpace => -28,       // ENOSPC
            Self::AlreadyExists => -17, // EEXIST
            Self::NotEmpty => -39,      // ENOTEMPTY
            Self::InvalidArg => -22,    // EINVAL
            Self::Io => -5,             // EIO
            Self::NotSupported => -38,  // ENOSYS
            Self::TooManyOpen => -24,   // EMFILE
            Self::BadFd => -9,          // EBADF
            Self::Interrupted => -4,    // EINTR
            Self::WouldBlock => -11,    // EAGAIN
            Self::BrokenPipe => -32,    // EPIPE
            Self::Loop => -40,          // ELOOP
            Self::NameTooLong => -36,   // ENAMETOOLONG
            Self::CrossDevice => -18,   // EXDEV
            Self::NoEntry => -2,        // ENOENT
        }
    }
}

pub type VfsResult<T> = Result<T, VfsError>;

// -- Inode types ------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    RegularFile,
    Directory,
    SymLink,
    CharDevice,
    BlockDevice,
    Pipe,
    Socket,
}

impl FileType {
    pub fn mode_bits(self) -> u32 {
        match self {
            Self::RegularFile => 0o100000,
            Self::Directory => 0o040000,
            Self::SymLink => 0o120000,
            Self::CharDevice => 0o020000,
            Self::BlockDevice => 0o060000,
            Self::Pipe => 0o010000,
            Self::Socket => 0o140000,
        }
    }
}

// -- Stat structure (mirrors Linux struct stat64) --------------------------

#[derive(Default, Clone, Copy)]
#[repr(C)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub _pad0: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: u64,
    pub st_atime_ns: u64,
    pub st_mtime: u64,
    pub st_mtime_ns: u64,
    pub st_ctime: u64,
    pub st_ctime_ns: u64,
    pub _unused: [i64; 3],
}

// -- Directory entry --------------------------------------------------------

#[derive(Clone)]
pub struct DirEntry {
    pub name: String,
    pub inode: u64,
    pub ftype: FileType,
}

// -- Inode operations trait -------------------------------------------------

pub trait InodeOps: Send + Sync + core::any::Any {
    fn as_any(&self) -> &dyn core::any::Any
    where
        Self: Sized,
    {
        self
    }
    fn stat(&self) -> VfsResult<Stat>;
    fn read(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize>;
    fn write(&self, offset: u64, buf: &[u8]) -> VfsResult<usize>;
    fn readdir(&self, offset: u64) -> VfsResult<Vec<DirEntry>>;
    fn lookup(&self, name: &str) -> VfsResult<Arc<Inode>>;
    fn create(&self, name: &str, ftype: FileType, mode: u32) -> VfsResult<Arc<Inode>>;
    fn mkdir(&self, name: &str, mode: u32) -> VfsResult<Arc<Inode>>;
    fn unlink(&self, name: &str) -> VfsResult<()>;
    fn rmdir(&self, name: &str) -> VfsResult<()>;
    fn rename(&self, old_name: &str, new_parent: &Arc<Inode>, new_name: &str) -> VfsResult<()>;
    fn readlink(&self) -> VfsResult<String> {
        Err(VfsError::InvalidArg)
    }
    fn truncate(&self, size: u64) -> VfsResult<()>;
    fn chmod(&self, mode: u32) -> VfsResult<()>;
    fn chown(&self, uid: u32, gid: u32) -> VfsResult<()>;
    fn symlink(&self, name: &str, target: &str) -> VfsResult<Arc<Inode>>;
    fn link(&self, name: &str, target: &Arc<Inode>) -> VfsResult<()>;
}

// -- Inode -----------------------------------------------------------------

pub struct Inode {
    pub ino: u64,
    pub ftype: FileType,
    pub ops: Arc<dyn InodeOps>,
}

impl Inode {
    pub fn new(ino: u64, ftype: FileType, ops: Arc<dyn InodeOps>) -> Arc<Self> {
        Arc::new(Self { ino, ftype, ops })
    }
}

// -- Filesystem drivers ----------------------------------------------------

#[derive(Clone)]
pub enum MountSource {
    None,
    BlockDevice(Arc<dyn crate::block::BlockDevice>),
}

pub struct MountRequest<'a> {
    pub target: &'a str,
    pub source: MountSource,
    pub flags: u64,
}

impl<'a> MountRequest<'a> {
    pub fn new(target: &'a str, source: MountSource) -> Self {
        Self {
            target,
            source,
            flags: 0,
        }
    }
}

pub trait FileSystemDriver: Send + Sync {
    fn fs_type(&self) -> &'static str;
    fn mount(&self, request: &MountRequest) -> Result<Arc<Inode>, &'static str>;
}

static FILESYSTEMS: Mutex<BTreeMap<String, Arc<dyn FileSystemDriver>>> =
    Mutex::new(BTreeMap::new());
static INODE_COUNTER: AtomicU64 = AtomicU64::new(1);

lazy_static! {
    static ref KERNEL_MOUNT_NAMESPACE: Arc<mount_namespace::MountNamespace> =
        Arc::new(mount_namespace::MountNamespace::new());
}

pub fn alloc_ino() -> u64 {
    INODE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub fn register_filesystem(driver: Arc<dyn FileSystemDriver>) -> VfsResult<()> {
    let mut filesystems = FILESYSTEMS.lock();
    let fs_type = String::from(driver.fs_type());
    if filesystems.contains_key(&fs_type) {
        return Err(VfsError::AlreadyExists);
    }
    filesystems.insert(fs_type, driver);
    Ok(())
}

pub fn kernel_mount_namespace() -> Arc<mount_namespace::MountNamespace> {
    KERNEL_MOUNT_NAMESPACE.clone()
}

pub fn active_mount_namespace() -> Arc<mount_namespace::MountNamespace> {
    crate::process::with_current_process(|proc| proc.mount_namespace.clone())
        .unwrap_or_else(kernel_mount_namespace)
}

pub fn clone_active_mount_namespace() -> Arc<mount_namespace::MountNamespace> {
    active_mount_namespace().clone_namespace()
}

pub fn mount_fs(fs_type: &str, request: &MountRequest) -> Result<(), &'static str> {
    let driver = {
        let filesystems = FILESYSTEMS.lock();
        filesystems
            .get(fs_type)
            .cloned()
            .ok_or("vfs: filesystem driver not registered")?
    };

    let root = driver.mount(request)?;
    mount(request.target, fs_type, root);
    Ok(())
}

pub fn unmount(path: &str) -> VfsResult<()> {
    let path = path::normalise(path);
    active_mount_namespace().remove_mount(&path)?;
    crate::serial_println!("[vfs] unmounted {}", path);
    Ok(())
}

/// Mount a filesystem root at the given path.
pub fn mount(path: &str, fs_type: &str, root: Arc<Inode>) {
    let path = path::normalise(path);
    active_mount_namespace().insert_mount(&path, fs_type, root);
    crate::serial_println!("[vfs] mounted {} at {}", fs_type, path);
}

/// Resolve a path to an inode, following mount points and symlinks.
pub fn resolve(path: &str) -> VfsResult<Arc<Inode>> {
    path::resolve(path, 0)
}

pub fn resolve_parent(path: &str) -> VfsResult<(Arc<Inode>, String)> {
    path::resolve_parent(path)
}

/// Get the root inode for a mount point (or its parent).
pub fn get_mount_root(path: &str) -> Option<Arc<Inode>> {
    active_mount_namespace().get_mount_root(path)
}

pub fn list_mounts() -> Vec<(String, String)> {
    active_mount_namespace().list_mounts()
}

pub fn list_filesystems() -> Vec<String> {
    FILESYSTEMS.lock().keys().cloned().collect()
}
