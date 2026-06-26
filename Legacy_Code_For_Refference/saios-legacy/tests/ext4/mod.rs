//! ext4 filesystem tests

use crate::println;

pub mod crash;
pub mod directory;
pub mod mount;
pub mod readwrite;

/// Run all ext4 tests
pub fn run_all() {
    println!("[ext4] Running tests...");
    mount::run_all();
    readwrite::run_all();
    directory::run_all();
    crash::run_all();
    println!("[ext4] All tests completed");
}
