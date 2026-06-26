//! TTY I/O operations for VFS integration
//!
//! This module provides the interface between the VFS layer and the TTY drivers.
//! All TTY state is owned by the TTYState in mod.rs - no duplication.

use crate::tty::{DEV_CONSOLE, DEV_TTY0, TtyInode};
use crate::vfs::{FileType, Inode, InodeOps, VfsError, VfsResult};
use alloc::sync::Arc;

use crate::ipc::signal as ipc_signal;
// TTY device major number (arbitrary)
pub const TTY_MAJOR: u64 = 5;

// Create a TTY device inode
pub fn create_tty_inode(devno: u64) -> Arc<Inode> {
    let tty = match devno {
        DEV_TTY0 => TtyInode::new_tty0(),
        _ => TtyInode::new_console(),
    };
    Inode::new(devno, FileType::CharDevice, Arc::new(tty))
}

// Create console device
pub fn create_console() -> Arc<Inode> {
    create_tty_inode(DEV_CONSOLE)
}

// Create /dev/tty0
pub fn create_tty0() -> Arc<Inode> {
    let tty = TtyInode::new_tty0();
    Inode::new(DEV_TTY0, FileType::CharDevice, Arc::new(tty))
}

// Initialize TTY devices and register with VFS
pub fn init_vfs() -> Result<(), &'static str> {
    // TTY devices are mounted under /dev/
    // /dev/console - primary console
    // /dev/tty0   - first virtual console
    Ok(())
}

// Get TTY by device number
pub fn get_tty_inode(devno: u64) -> Option<Arc<Inode>> {
    match devno {
        DEV_CONSOLE => Some(create_console()),
        DEV_TTY0 => Some(create_tty0()),
        _ => None,
    }
}

// TTY ioctls for terminal control
pub fn tty_ioctl(tty: &TtyInode, request: u64, arg: u64) -> VfsResult<()> {
    match request {
        // Get window size
        0x5413 /* TIOCGWINSZ */ => {
            // Use global TTY state for consistent window size
            let state = crate::tty::get_tty_state();
            let (rows, cols) = get_console_size();
            unsafe {
                core::ptr::write_volatile(arg as *mut u16, rows);
                core::ptr::write_volatile((arg + 2) as *mut u16, cols);
                core::ptr::write_volatile((arg + 4) as *mut u16, 0); // xpixel
                core::ptr::write_volatile((arg + 6) as *mut u16, 0); // ypixel
            }
        }

        // Set window size (no-op, just acknowledge)
        0x5414 /* TIOCSWINSZ */ => {}

        // Get foreground process group
        0x540F /* TIOCGPGRP */ => {
            let state = crate::tty::get_tty_state();
            unsafe {
                core::ptr::write_volatile(arg as *mut u32, state.foreground_pgid);
            }
        }

        // Set foreground process group
        0x5410 /* TIOCSPGRP */ => {
            let pgid = unsafe { core::ptr::read_volatile(arg as *const u32) };
            if !set_fg_pgid_if_process_group_exists(pgid) {
                return Err(VfsError::InvalidArg);
            }
        }

        // Set controlling TTY
        0x5480 /* TIOCSCTTY */ => {
            let mut state = crate::tty::get_tty_state();
            state.controlling_tty = Some(tty.devno);
            // Session ID is set in the state - default to 1 for init
        }

        // Get session ID
        0x5429 /* TIOCGSID */ => {
            let state = crate::tty::get_tty_state();
            unsafe {
                core::ptr::write_volatile(arg as *mut u32, state.session_id);
            }
        }

        _ => return Err(VfsError::NotSupported),
    }

    Ok(())
}

// Get console size - simplified, returns fixed size
fn get_console_size() -> (u16, u16) {
    // Use global terminal state - simplified to fixed 25x80 for now
    // Full implementation would read from hardware or use actual display size
    (25, 80)
}

// Get foreground process group from TTY state
pub fn get_fg_pgid() -> u32 {
    let state = crate::tty::get_tty_state();
    state.foreground_pgid
}

// Set foreground process group in TTY state
pub fn set_fg_pgid(pgid: u32) {
    let mut state = crate::tty::get_tty_state();
    state.foreground_pgid = pgid;
}

pub fn set_fg_pgid_if_process_group_exists(pgid: u32) -> bool {
    if !process_group_exists(pgid) {
        return false;
    }
    set_fg_pgid(pgid);
    true
}

pub fn is_signal_generation_enabled() -> bool {
    let state = crate::tty::get_tty_state();
    state.flags.isig
}

fn process_group_exists(pgid: u32) -> bool {
    crate::process::table::TABLE
        .lock()
        .procs
        .values()
        .any(|proc| proc.pgid == pgid)
}

// Get session ID from TTY state
pub fn get_session_id() -> u32 {
    let state = crate::tty::get_tty_state();
    state.session_id
}

// Set session ID in TTY state
pub fn set_session_id(sid: u32) {
    let mut state = crate::tty::get_tty_state();
    state.session_id = sid;
}

// Get controlling TTY from TTY state
pub fn get_controlling_tty() -> Option<u64> {
    let state = crate::tty::get_tty_state();
    state.controlling_tty
}

// Set controlling TTY in TTY state
pub fn set_controlling_tty(devno: u64) {
    let mut state = crate::tty::get_tty_state();
    state.controlling_tty = Some(devno);
}

// Signal the foreground process group (for terminal-generated signals)
pub fn signal_fg_pgid(sig: u32) -> bool {
    let pgid = {
        let state = crate::tty::get_tty_state();
        state.foreground_pgid
    };
    let pids = crate::process::table::TABLE
        .lock()
        .pids_in_process_group(pgid);
    let mut delivered = false;
    for pid in pids {
        delivered |= crate::ipc::signal::raise_signal_for_pid(pid, sig);
    }
    delivered
}
