use crate::heap;
use crate::kernel::testing::framework::{TestCase, TestSuite};
use crate::pmm;
use crate::{kt_assert, kt_assert_eq};

fn test_pmm_has_pages() -> Result<(), &'static str> {
    kt_assert!(pmm::total_pages() > 0);
    Ok(())
}

fn test_pmm_accounting_balances() -> Result<(), &'static str> {
    kt_assert_eq!(pmm::total_pages(), pmm::free_pages() + pmm::used_pages());
    Ok(())
}

fn test_heap_is_initialized() -> Result<(), &'static str> {
    let stats = heap::stats();
    kt_assert!(stats.total > 0);
    kt_assert!(stats.used <= stats.total);
    Ok(())
}

const TESTS: [TestCase; 3] = [
    TestCase::new("pmm_has_pages", test_pmm_has_pages),
    TestCase::new("pmm_accounting_balances", test_pmm_accounting_balances),
    TestCase::new("heap_is_initialized", test_heap_is_initialized),
];

pub fn suite() -> TestSuite {
    TestSuite::new("memory", &TESTS)
}
