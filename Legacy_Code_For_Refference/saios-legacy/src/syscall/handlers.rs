//! Linux x86_64 syscall handlers used by the current SAIOS compatibility layer.
//!
//! The surface is broad, but not complete: some syscalls are fully implemented,
//! some are partial compatibility shims, and others still return `-ENOSYS` or a
//! simplified success value. Check `src/syscall/mod.rs` for the active mapping.

mod fs;
mod memory;
mod misc;
mod network;
#[path = "handlers/process.rs"]
mod proc_handlers;
mod signal;
mod thread;

pub use fs::*;
pub use memory::*;
pub use misc::*;
pub use network::*;
pub use proc_handlers::*;
pub use signal::*;
pub use thread::*;

use crate::process::{self, FD_STDERR, FD_STDIN, FD_STDOUT};
use crate::vfs::{FileType, VfsError, file::OpenFile};
use alloc::string::String;
use alloc::vec::Vec;

pub const EINVAL: i64 = -22;
pub const ENOSYS: i64 = -38;
pub const ENOMEM: i64 = -12;
pub const ENOENT: i64 = -2;
pub const EBADF: i64 = -9;

pub(crate) const EPERM: i64 = -1;
pub(crate) const ESRCH: i64 = -3;
pub(crate) const EINTR: i64 = -4;
pub(crate) const EIO: i64 = -5;
pub(crate) const ENXIO: i64 = -6;
pub(crate) const E2BIG: i64 = -7;
pub(crate) const ENOEXEC: i64 = -8;
pub(crate) const ECHILD: i64 = -10;
pub(crate) const EAGAIN: i64 = -11;
pub(crate) const EACCES: i64 = -13;
pub(crate) const EFAULT: i64 = -14;
pub(crate) const EBUSY: i64 = -16;
pub(crate) const EEXIST: i64 = -17;
pub(crate) const EXDEV: i64 = -18;
pub(crate) const ENODEV: i64 = -19;
pub(crate) const ENOTDIR: i64 = -20;
pub(crate) const EISDIR: i64 = -21;
pub(crate) const ENFILE: i64 = -23;
pub(crate) const EMFILE: i64 = -24;
pub(crate) const ENOTTY: i64 = -25;
pub(crate) const EFBIG: i64 = -27;
pub(crate) const ENOSPC: i64 = -28;
pub(crate) const ESPIPE: i64 = -29;
pub(crate) const EROFS: i64 = -30;
pub(crate) const EPIPE: i64 = -32;
pub(crate) const ERANGE: i64 = -34;
pub(crate) const ENOTEMPTY: i64 = -39;
pub(crate) const ELOOP: i64 = -40;
pub(crate) const ENOMSG: i64 = -42;
pub(crate) const EPROTO: i64 = -71;
pub(crate) const ENOTSUP: i64 = -95;
pub(crate) const EADDRINUSE: i64 = -98;
pub(crate) const ECONNREFUSED: i64 = -111;

pub(crate) fn vfs_err(e: VfsError) -> i64 {
    e.to_errno()
}

pub(crate) unsafe fn read_user_str(ptr: u64, max: usize) -> Option<String> {
    unsafe {
        if ptr == 0 || ptr < 0x1000 {
            return None;
        }
        let mut v = Vec::new();
        let mut p = ptr as *const u8;
        for _ in 0..max {
            let c = core::ptr::read_volatile(p);
            if c == 0 {
                break;
            }
            v.push(c);
            p = p.add(1);
        }
        String::from_utf8(v).ok()
    }
}

pub(crate) unsafe fn write_user<T: Copy>(ptr: u64, val: T) -> bool {
    unsafe {
        if ptr == 0 {
            return false;
        }
        core::ptr::write_volatile(ptr as *mut T, val);
        true
    }
}

pub(crate) fn with_fd<F: FnOnce(&crate::vfs::file::OpenFile) -> i64>(fd: u64, f: F) -> i64 {
    if let Ok(file) = crate::vfs_contract::VfsContract::get_fd(fd as usize) {
        f(&file)
    } else if process::current_pid().is_some() {
        EBADF
    } else {
        match fd {
            0 => EBADF,
            1 | 2 => f(&OpenFile::new(dummy_stdout_inode(), 1)),
            _ => EBADF,
        }
    }
}

pub(crate) fn dummy_stdout_inode() -> alloc::sync::Arc<crate::vfs::Inode> {
    use crate::vfs::{DirEntry, Inode, InodeOps, Stat, VfsResult, alloc_ino};
    use alloc::sync::Arc;

    struct StdoutOps;

    impl InodeOps for StdoutOps {
        fn stat(&self) -> VfsResult<Stat> {
            Ok(Stat {
                st_mode: FileType::CharDevice.mode_bits() | 0o600,
                ..Default::default()
            })
        }
        fn read(&self, _: u64, _: &mut [u8]) -> VfsResult<usize> {
            Err(VfsError::BadFd)
        }
        fn write(&self, _: u64, buf: &[u8]) -> VfsResult<usize> {
            if let Ok(s) = core::str::from_utf8(buf) {
                crate::print!("{}", s);
                crate::serial_print!("{}", s);
            }
            Ok(buf.len())
        }
        fn readdir(&self, _: u64) -> VfsResult<Vec<DirEntry>> {
            Err(VfsError::NotADir)
        }
        fn lookup(&self, _: &str) -> VfsResult<Arc<Inode>> {
            Err(VfsError::NotADir)
        }
        fn create(&self, _: &str, _: FileType, _: u32) -> VfsResult<Arc<Inode>> {
            Err(VfsError::PermDenied)
        }
        fn mkdir(&self, _: &str, _: u32) -> VfsResult<Arc<Inode>> {
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
        fn symlink(&self, _: &str, _: &str) -> VfsResult<Arc<Inode>> {
            Err(VfsError::PermDenied)
        }
        fn link(&self, _: &str, _: &Arc<Inode>) -> VfsResult<()> {
            Err(VfsError::PermDenied)
        }
        fn rename(&self, _: &str, _: &Arc<Inode>, _: &str) -> VfsResult<()> {
            Err(VfsError::PermDenied)
        }
    }

    Inode::new(alloc_ino(), FileType::CharDevice, Arc::new(StdoutOps))
}
