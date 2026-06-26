//! Virtual console /dev/tty0
//!
//! This is a placeholder module for virtual console support.
//! TTY devices can be created dynamically as needed.

use crate::tty::TtyInode;
use crate::vfs::Inode;
use alloc::sync::Arc;

/// Create a virtual console device
pub fn create_tty_device(devno: u64) -> Arc<Inode> {
    let tty = TtyInode::new_tty0();
    Inode::new(devno, crate::vfs::FileType::CharDevice, Arc::new(tty))
}
