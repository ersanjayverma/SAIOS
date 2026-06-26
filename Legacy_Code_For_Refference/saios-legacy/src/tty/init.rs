//! TTY initialization - mounts TTY devices and initializes console
//!
//! This module provides TTY initialization routines to be called during
//! system boot, similar to how driver::keyboard::init() is called.

use crate::tty;

/// Initialize the TTY subsystem
pub fn init() {
    tty::init();
}

/// Mount TTY devices under /dev/
/// Called from init_rootfs() after /dev directory exists
pub fn mount_devices() {
    // TTY devices will be mounted in init_rootfs() or similar
    // For now, just ensure the /dev directory exists
    crate::mkdir_p("/dev");
}
