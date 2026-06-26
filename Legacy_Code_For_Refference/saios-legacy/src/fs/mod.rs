//! Filesystem module.

pub mod devfs;
pub mod ext4;
pub mod procfs;
pub mod tmpfs;

pub fn register_builtin_filesystems() -> Result<(), &'static str> {
    tmpfs::register_driver()?;
    procfs::register_driver()?;
    devfs::register_driver()?;
    ext4::register_driver()?;
    Ok(())
}

// Re-export old ramfs functions used by the kernel shell
pub use crate::fs_ramfs::{append, init, ls, mkdir, read, remove, stat, write};
