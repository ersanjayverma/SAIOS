//! Memory subsystem tests for SAIOS
//!
//! This module contains unit and integration tests for:
//!   - Frame allocator
//!   - Page table management
//!   - Copy-on-write implementation
//!   - Address space management

use crate::println;

pub mod address_space;
pub mod cow;
pub mod frame_allocator;
pub mod paging;

/// Run all memory tests
pub fn run_all() {
    println!("[memory] Running tests...");

    println!("[memory] Frame allocator tests:");
    frame_allocator::run_tests();

    println!("[memory] Paging tests:");
    paging::run_tests();

    println!("[memory] COW tests:");
    cow::run_tests();

    println!("[memory] Address space tests:");
    address_space::run_tests();

    println!("[memory] All tests completed");
}
