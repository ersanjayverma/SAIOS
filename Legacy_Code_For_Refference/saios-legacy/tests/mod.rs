//! SAIOS Test Suite
//!
//! This module provides a centralized test harness that can be invoked
//! from the kernel shell or automated test environments.
//!
//! Usage in kernel shell:
//!   - tests all     Run all tests
//!   - tests memory  Run memory tests only
//!   - tests fs      Run filesystem tests only
//!   - tests ci      Run CI-compatible test suite

pub mod block;
pub mod ext4;
pub mod memory;
pub mod test_runner;
pub mod vfs;

/// Run all tests
pub fn run_all() {
    test_runner::run_all_tests();
}

/// Run memory tests only
pub fn run_memory() {
    test_runner::run_memory_tests();
}

/// Run filesystem tests only
pub fn run_fs() {
    test_runner::run_fs_tests();
}

/// Run CI tests
pub fn run_ci() {
    test_runner::run_ci_tests();
}

/// Handle `tests <subcommand>` from the shell.
pub fn handle_command(args: &str) {
    test_runner::handle_test_command(args);
}
