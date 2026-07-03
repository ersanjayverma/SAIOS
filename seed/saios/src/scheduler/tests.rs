use crate::kernel::testing::framework::{TestCase, TestSuite};
use crate::kt_assert;
use crate::scheduler;

fn test_scheduler_has_threads() -> Result<(), &'static str> {
    kt_assert!(!scheduler::threads().is_empty());
    Ok(())
}

fn test_scheduler_has_running_thread() -> Result<(), &'static str> {
    let running = scheduler::threads()
        .into_iter()
        .filter(|t| t.state == scheduler::ThreadState::Running)
        .count();
    kt_assert!(running == 1);
    Ok(())
}

const TESTS: [TestCase; 2] = [
    TestCase::new("scheduler_has_threads", test_scheduler_has_threads),
    TestCase::new(
        "scheduler_has_running_thread",
        test_scheduler_has_running_thread,
    ),
];

pub fn suite() -> TestSuite {
    TestSuite::new("scheduler", &TESTS)
}
