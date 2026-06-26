//! Syscall dispatch table - routes syscalls to their handlers.
//!
//! This file contains helper functions for the syscall dispatcher.
//! The main dispatcher is in mod.rs as it needs to handle handlers with
//! different argument counts.

use crate::syscall::handlers;

/// Maximum number of supported syscalls
pub const MAX_SYSCALLS: usize = 512;

/// Helper function to get handler for a syscall number
/// Returns None - the actual dispatch is done in mod.rs with specific handler signatures
pub fn get_handler(_num: u64) -> Option<()> {
    // We use a simple approach - return None for now
    // Actual dispatch happens in mod.rs which knows handler signatures
    None
}
