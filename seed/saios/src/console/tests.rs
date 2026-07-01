use crate::console;
use crate::kernel::testing::framework::{TestCase, TestSuite};
use crate::kt_assert;

fn test_console_initialized() -> Result<(), &'static str> {
    kt_assert!(console::is_initialized());
    Ok(())
}

const TESTS: [TestCase; 1] = [TestCase::new("console_initialized", test_console_initialized)];

pub fn suite() -> TestSuite {
    TestSuite::new("console", &TESTS)
}
