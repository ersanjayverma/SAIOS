//! TTY subsystem - terminal devices for login sessions and job control
//!
//! Device nodes:
//!   /dev/console  - Primary console (read/write)
//!   /dev/tty0     - First virtual console
//!   /dev/ttyN     - Additional virtual consoles (N = 0..63)
//!
//! TTY state is owned by the TTY object itself, not duplicated across process,
//! session, or global state. This provides a single authoritative source of truth.

pub mod console;
pub mod init;
pub mod io;
pub mod tty0;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

// TTY device numbers
pub const DEV_TTY0: u64 = 5;
pub const DEV_CONSOLE: u64 = 4;

// Terminal control commands
pub const TCGETS: u64 = 0x5401;
pub const TCSETS: u64 = 0x5402;
pub const TCSETSW: u64 = 0x5403;
pub const TIOCGWINSZ: u64 = 0x5413;
pub const TIOCSWINSZ: u64 = 0x5414;
pub const TIOCGPGRP: u64 = 0x540F;
pub const TIOCSPGRP: u64 = 0x5410;
pub const TIOCSCTTY: u64 = 0x5480;
pub const TIOCGSID: u64 = 0x5429;

// TTY flags (basic, minimal implementation)
#[derive(Debug, Clone, Copy)]
pub struct TtyFlags {
    pub canonical: bool, // Line buffering mode
    pub echo: bool,      // Echo input characters
    pub isig: bool,      // Enable signals (Ctrl+C, Ctrl+Z)
}

impl Default for TtyFlags {
    fn default() -> Self {
        TtyFlags {
            canonical: true,
            echo: true,
            isig: true,
        }
    }
}

// Terminal state structure - single authoritative owner of TTY state
#[derive(Debug)]
pub struct TtyState {
    pub session_id: u32,
    pub foreground_pgid: u32,
    pub controlling_tty: Option<u64>,
    pub input_buffer: Vec<u8>,
    pub flags: TtyFlags,
}

impl Default for TtyState {
    fn default() -> Self {
        TtyState {
            session_id: 1,      // PID 1 is the session leader initially
            foreground_pgid: 1, // PID 1 is foreground initially
            controlling_tty: Some(DEV_CONSOLE),
            input_buffer: Vec::with_capacity(256),
            flags: TtyFlags::default(),
        }
    }
}

// Global TTY state - single authoritative source
// Using lazy_static pattern for safe initialization
lazy_static::lazy_static! {
    static ref TTY_STATE: Mutex<TtyState> = Mutex::new(TtyState::default());
}

// Initialize TTY subsystem - must be called before get_tty_state
pub fn init() {
    // State initialized with lazy_static
}

// Get TTY state reference
pub fn get_tty_state() -> spin::MutexGuard<'static, TtyState> {
    TTY_STATE.lock()
}

// TTY inode operations for /dev/console and /dev/tty0
pub struct TtyInode {
    pub devno: u64,
}

impl TtyInode {
    pub fn new_console() -> Self {
        TtyInode { devno: DEV_CONSOLE }
    }

    pub fn new_tty0() -> Self {
        TtyInode { devno: DEV_TTY0 }
    }
}

impl crate::vfs::InodeOps for TtyInode {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn stat(&self) -> crate::vfs::VfsResult<crate::vfs::Stat> {
        Ok(crate::vfs::Stat {
            st_ino: self.devno,
            st_mode: crate::vfs::FileType::CharDevice.mode_bits() | 0o666,
            st_nlink: 1,
            st_uid: 0,
            st_gid: 0,
            st_rdev: self.devno,
            ..Default::default()
        })
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> crate::vfs::VfsResult<usize> {
        if offset > 0 {
            // Only support reads from offset 0 for now
            return Ok(0);
        }

        if self.devno == DEV_CONSOLE {
            console::read(buf, offset)
        } else {
            // /dev/ttyN - not implemented yet
            Ok(0)
        }
    }

    fn write(&self, offset: u64, buf: &[u8]) -> crate::vfs::VfsResult<usize> {
        if offset > 0 {
            return Ok(0);
        }

        if self.devno == DEV_CONSOLE {
            console::write(buf, offset)
        } else {
            Ok(buf.len())
        }
    }

    fn readdir(&self, offset: u64) -> crate::vfs::VfsResult<Vec<crate::vfs::DirEntry>> {
        Err(crate::vfs::VfsError::NotADir)
    }

    fn lookup(&self, _name: &str) -> crate::vfs::VfsResult<alloc::sync::Arc<crate::vfs::Inode>> {
        Err(crate::vfs::VfsError::NotADir)
    }

    fn create(
        &self,
        _name: &str,
        _ftype: crate::vfs::FileType,
        _mode: u32,
    ) -> crate::vfs::VfsResult<alloc::sync::Arc<crate::vfs::Inode>> {
        Err(crate::vfs::VfsError::NotADir)
    }

    fn mkdir(
        &self,
        _name: &str,
        _mode: u32,
    ) -> crate::vfs::VfsResult<alloc::sync::Arc<crate::vfs::Inode>> {
        Err(crate::vfs::VfsError::NotADir)
    }

    fn unlink(&self, _name: &str) -> crate::vfs::VfsResult<()> {
        Err(crate::vfs::VfsError::NotADir)
    }

    fn rmdir(&self, _name: &str) -> crate::vfs::VfsResult<()> {
        Err(crate::vfs::VfsError::NotADir)
    }

    fn rename(
        &self,
        _old_name: &str,
        _new_parent: &alloc::sync::Arc<crate::vfs::Inode>,
        _new_name: &str,
    ) -> crate::vfs::VfsResult<()> {
        Err(crate::vfs::VfsError::NotADir)
    }

    fn readlink(&self) -> crate::vfs::VfsResult<String> {
        Err(crate::vfs::VfsError::NotAFile)
    }

    fn truncate(&self, _size: u64) -> crate::vfs::VfsResult<()> {
        Ok(())
    }

    fn chmod(&self, _mode: u32) -> crate::vfs::VfsResult<()> {
        Ok(())
    }

    fn chown(&self, _uid: u32, _gid: u32) -> crate::vfs::VfsResult<()> {
        Ok(())
    }

    fn symlink(
        &self,
        _name: &str,
        _target: &str,
    ) -> crate::vfs::VfsResult<alloc::sync::Arc<crate::vfs::Inode>> {
        Err(crate::vfs::VfsError::NotSupported)
    }

    fn link(
        &self,
        _name: &str,
        _target: &alloc::sync::Arc<crate::vfs::Inode>,
    ) -> crate::vfs::VfsResult<()> {
        Err(crate::vfs::VfsError::NotSupported)
    }
}
