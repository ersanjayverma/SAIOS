use crate::kernel::testing::framework::{TestCase, TestSuite};
use crate::saifs;
use crate::kt_assert;

fn test_saifs_initialized() -> Result<(), &'static str> {
    saifs::init();
    kt_assert!(saifs::is_initialized());
    Ok(())
}

fn test_saifs_root_mount_exists() -> Result<(), &'static str> {
    saifs::init();
    kt_assert!(saifs::mounts().iter().any(|m| m.path == "/"));
    Ok(())
}

const TESTS: [TestCase; 2] = [
    TestCase::new("saifs_initialized", test_saifs_initialized),
    TestCase::new("saifs_root_mount_exists", test_saifs_root_mount_exists),
];

pub fn suite() -> TestSuite {
    TestSuite::new("saifs", &TESTS)
}
