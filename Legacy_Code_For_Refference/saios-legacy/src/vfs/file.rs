//! Open file description table — per-process file descriptors.

use super::{FileType, Inode, VfsError, VfsResult};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// Open flags (Linux values)
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_CLOEXEC: u32 = 0o2000000;
pub const O_DIRECTORY: u32 = 0o200000;

/// An open file — one per `open()` call, shared via dup/dup2.
pub struct OpenFile {
    pub inode: Arc<Inode>,
    pub offset: AtomicU64,
    pub flags: u32,
}

impl OpenFile {
    pub fn new(inode: Arc<Inode>, flags: u32) -> Arc<Self> {
        Arc::new(Self {
            inode,
            offset: AtomicU64::new(0),
            flags,
        })
    }

    pub fn read(&self, buf: &mut [u8]) -> VfsResult<usize> {
        let off = self.offset.load(Ordering::Relaxed);
        let n = self.inode.ops.read(off, buf)?;
        self.offset.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }

    pub fn write(&self, buf: &[u8]) -> VfsResult<usize> {
        let off = if self.flags & O_APPEND != 0 {
            // Append: write to end of file
            let stat = self.inode.ops.stat()?;
            stat.st_size as u64
        } else {
            self.offset.load(Ordering::Relaxed)
        };
        let n = self.inode.ops.write(off, buf)?;
        self.offset.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }

    pub fn seek(&self, offset: i64, whence: u32) -> VfsResult<u64> {
        let stat = self.inode.ops.stat()?;
        let new_off: u64 = match whence {
            0 /* SEEK_SET */ => {
                if offset < 0 { return Err(VfsError::InvalidArg); }
                offset as u64
            }
            1 /* SEEK_CUR */ => {
                let cur = self.offset.load(Ordering::Relaxed) as i64;
                let new = cur + offset;
                if new < 0 { return Err(VfsError::InvalidArg); }
                new as u64
            }
            2 /* SEEK_END */ => {
                let end = stat.st_size;
                let new = end + offset;
                if new < 0 { return Err(VfsError::InvalidArg); }
                new as u64
            }
            _ => return Err(VfsError::InvalidArg),
        };
        self.offset.store(new_off, Ordering::Relaxed);
        Ok(new_off)
    }

    pub fn is_readable(&self) -> bool {
        self.flags & 3 != O_WRONLY
    }
    pub fn is_writable(&self) -> bool {
        self.flags & 3 != O_RDONLY
    }
}

// -- Per-process file descriptor table -------------------------------------

pub const MAX_FDS: usize = 1024;

#[derive(Clone)]
pub struct FdTable {
    fds: Vec<Option<Arc<OpenFile>>>,
}

impl FdTable {
    pub fn new() -> Self {
        let mut fds = Vec::with_capacity(16);
        fds.resize(16, None);
        Self { fds }
    }

    pub fn insert(&mut self, file: Arc<OpenFile>) -> VfsResult<usize> {
        // Find lowest free slot ≥ 3
        for (i, slot) in self.fds.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(file);
                return Ok(i);
            }
        }
        if self.fds.len() >= MAX_FDS {
            return Err(VfsError::TooManyOpen);
        }
        let i = self.fds.len();
        self.fds.push(Some(file));
        Ok(i)
    }

    pub fn insert_at(&mut self, fd: usize, file: Arc<OpenFile>) {
        if fd >= self.fds.len() {
            self.fds.resize(fd + 1, None);
        }
        self.fds[fd] = Some(file);
    }

    pub fn get(&self, fd: usize) -> VfsResult<Arc<OpenFile>> {
        self.fds
            .get(fd)
            .and_then(|s| s.clone())
            .ok_or(VfsError::BadFd)
    }

    pub fn close(&mut self, fd: usize) -> VfsResult<()> {
        let slot = self.fds.get_mut(fd).ok_or(VfsError::BadFd)?;
        if slot.is_none() {
            return Err(VfsError::BadFd);
        }
        *slot = None;
        Ok(())
    }

    pub fn dup(&self, fd: usize) -> VfsResult<Arc<OpenFile>> {
        self.get(fd)
    }

    pub fn close_on_exec(&mut self) {
        for slot in self.fds.iter_mut() {
            if let Some(f) = slot
                && f.flags & O_CLOEXEC != 0
            {
                *slot = None;
            }
        }
    }
}
