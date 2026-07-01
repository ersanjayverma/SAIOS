use alloc::vec::Vec;

use crate::heap;
use crate::kernel::testing::report::{VerifyCheck, VerifyReport};
use crate::pmm;

pub mod tests;

pub fn verify() -> VerifyReport {
    let total = pmm::total_pages();
    let free = pmm::free_pages();
    let used = pmm::used_pages();
    let heap_stats = heap::stats();

    let mut checks = Vec::new();

    checks.push(if total > 0 {
        VerifyCheck::pass("PMM total pages", "memory map initialized")
    } else {
        VerifyCheck::fail("PMM total pages", "no memory pages reported")
    });

    checks.push(if free + used == total {
        VerifyCheck::pass("Page accounting", "free + used == total")
    } else {
        VerifyCheck::fail("Page accounting", "free + used mismatch")
    });

    checks.push(if heap_stats.total > 0 {
        VerifyCheck::pass("Heap initialized", "heap arena configured")
    } else {
        VerifyCheck::fail("Heap initialized", "heap arena is empty")
    });

    checks.push(if heap_stats.used <= heap_stats.total {
        VerifyCheck::pass("Heap bounds", "used <= total")
    } else {
        VerifyCheck::fail("Heap bounds", "heap used exceeds total")
    });

    VerifyReport {
        target: "memory",
        checks,
    }
}
