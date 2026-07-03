use crate::kernel::testing::framework::{TestCase, TestSuite};
use crate::kt_assert;
use crate::object_manager;

fn test_object_manager_initialized() -> Result<(), &'static str> {
    object_manager::init();
    kt_assert!(object_manager::is_initialized());
    Ok(())
}

fn test_object_manager_has_system_object() -> Result<(), &'static str> {
    object_manager::init();
    kt_assert!(object_manager::metadata("/system").is_some());
    Ok(())
}

const TESTS: [TestCase; 2] = [
    TestCase::new(
        "object_manager_initialized",
        test_object_manager_initialized,
    ),
    TestCase::new(
        "object_manager_has_system_object",
        test_object_manager_has_system_object,
    ),
];

pub fn suite() -> TestSuite {
    TestSuite::new("object", &TESTS)
}
