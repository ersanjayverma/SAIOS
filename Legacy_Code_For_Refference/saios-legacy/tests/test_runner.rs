//! SAIOS Test Runner
//!
//! This module provides a centralized test harness that can be invoked
//! from the kernel shell or automated test environments.
//!
//! Usage in kernel shell:
//!   - tests all     Run all tests
//!   - tests memory  Run memory tests only
//!   - tests fs      Run filesystem tests only
//!   - tests ci      Run CI-compatible test suite

use crate::println;
use core::sync::atomic::{AtomicBool, Ordering};

/// Test result type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
}

impl TestStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TestStatus::Pass => "PASS",
            TestStatus::Fail => "FAIL",
            TestStatus::Skip => "SKIP",
        }
    }
}

/// Single test result
pub struct TestResult {
    pub name: &'static str,
    pub status: TestStatus,
    pub duration_ms: u64,
}

/// Test suite result
pub struct SuiteResult {
    pub name: &'static str,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
}

impl SuiteResult {
    pub fn success(&self) -> bool {
        self.failed == 0 && self.total > 0
    }
}

/// CI-compatible output mode
pub static CI_MODE: AtomicBool = AtomicBool::new(false);

/// Run a single test function
pub fn run_test<F>(name: &'static str, f: F) -> TestResult
where
    F: FnOnce() -> bool,
{
    let start = crate::time::uptime_ns();
    let passed = f();
    let end = crate::time::uptime_ns();
    let duration = (end.saturating_sub(start)) / 1_000_000; // ns to ms

    let status = if passed {
        TestStatus::Pass
    } else {
        TestStatus::Fail
    };

    if CI_MODE.load(Ordering::Relaxed) {
        println!("{}: {} in {}ms", name, status.as_str(), duration);
    } else {
        println!("[test] {} - {} ({}ms)", name, status.as_str(), duration);
    }

    TestResult {
        name,
        status,
        duration_ms: duration,
    }
}

/// Memory test suite runner
pub fn run_memory_tests() -> SuiteResult {
    println!("\n=== Memory Tests ===");

    let start = crate::time::uptime_ns();
    let mut passed = 0usize;
    let failed = 0usize;

    let tests: &[(&str, fn())] = &[
        ("frame_allocator", super::memory::frame_allocator::run_all),
        ("paging", super::memory::paging::run_all),
        ("cow", super::memory::cow::run_all),
        ("address_space", super::memory::address_space::run_all),
    ];

    for (name, f) in tests {
        if CI_MODE.load(Ordering::Relaxed) {
            println!("::group::{}", name);
        }
        f();
        passed += 1;
        if CI_MODE.load(Ordering::Relaxed) {
            println!("::endgroup::");
        }
    }

    let end = crate::time::uptime_ns();
    let duration = (end.saturating_sub(start)) / 1_000_000;
    let total = passed + failed;

    println!("[test] Suite memory: {}/{} passed", passed, total);

    SuiteResult {
        name: "memory",
        total,
        passed,
        failed,
        skipped: 0,
        duration_ms: duration,
    }
}

/// Filesystem test suite runner
pub fn run_fs_tests() -> SuiteResult {
    println!("\n=== Filesystem Tests ===");

    let start = crate::time::uptime_ns();
    let mut passed = 0usize;
    let failed = 0usize;

    let tests: &[(&str, fn())] = &[
        ("ext4_mount", super::ext4::mount::run_all),
        ("ext4_readwrite", super::ext4::readwrite::run_all),
        ("ext4_directory", super::ext4::directory::run_all),
        ("ext4_crash", super::ext4::crash::run_all),
        ("vfs", super::vfs::run_all),
        ("block", super::block::run_all),
    ];

    for (name, f) in tests {
        if CI_MODE.load(Ordering::Relaxed) {
            println!("::group::{}", name);
        }
        f();
        passed += 1;
        if CI_MODE.load(Ordering::Relaxed) {
            println!("::endgroup::");
        }
    }

    let end = crate::time::uptime_ns();
    let duration = (end.saturating_sub(start)) / 1_000_000;
    let total = passed + failed;

    println!("[test] Suite fs: {}/{} passed", passed, total);

    SuiteResult {
        name: "fs",
        total,
        passed,
        failed,
        skipped: 0,
        duration_ms: duration,
    }
}

/// Run all tests
pub fn run_all_tests() {
    println!("\n=== SAIOS Test Suite ===\n");

    let mut total_passed = 0;
    let mut total_failed = 0;
    let mut total_duration = 0;

    let mem_result = run_memory_tests();
    total_passed += mem_result.passed;
    total_failed += mem_result.failed;
    total_duration += mem_result.duration_ms;

    let fs_result = run_fs_tests();
    total_passed += fs_result.passed;
    total_failed += fs_result.failed;
    total_duration += fs_result.duration_ms;

    let total = total_passed + total_failed;
    println!("\n=== Test Summary ===");
    println!(
        "[test] Total: {} tests, {} passed, {} failed",
        total, total_passed, total_failed
    );
    println!("[test] Duration: {}ms", total_duration);

    if total_failed > 0 {
        println!("[test] Some tests FAILED - review output above");
    } else {
        println!("[test] All tests PASSED!");
    }
}

/// Run CI-compatible test suite (subset for CI pipelines)
pub fn run_ci_tests() {
    CI_MODE.store(true, Ordering::Relaxed);
    println!("[test] Running CI test suite...");
    run_all_tests();
}

/// Handle the `tests` shell command.
pub fn handle_test_command(args: &str) {
    let cmd = args.trim();
    match cmd {
        "all" | "" => run_all_tests(),
        "memory" => {
            run_memory_tests();
        }
        "fs" => {
            run_fs_tests();
        }
        "ci" => run_ci_tests(),
        "help" => {
            println!("SAIOS Test Commands:");
            println!("  tests all     - Run all tests");
            println!("  tests memory  - Run memory subsystem tests");
            println!("  tests fs      - Run filesystem tests");
            println!("  tests ci      - Run CI-compatible test suite");
            println!("  tests help    - Show this help");
        }
        _ => {
            println!("Unknown test command: '{}'", cmd);
            println!("Type 'tests help' for available commands");
        }
    }
}
