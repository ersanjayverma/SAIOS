use alloc::vec::Vec;

use crate::heap;
use crate::kernel::testing::report::{VerifyCheck, VerifyReport};
use crate::pmm;
use crate::vmm;

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

    checks.push(if pmm::available_bytes() + pmm::used_bytes()
        == (pmm::total_pages() as u64).saturating_mul(pmm::PAGE_SIZE)
    {
        VerifyCheck::pass("PMM byte accounting", "available + used == total bytes")
    } else {
        VerifyCheck::fail("PMM byte accounting", "byte accounting mismatch")
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

    let vmm_report = vmm::verify();
    for check in vmm_report.checks {
        checks.push(check);
    }

    VerifyReport {
        target: "memory",
        checks,
    }
}
