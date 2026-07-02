use alloc::vec::Vec;

#[derive(Copy, Clone)]
pub struct TestFailure {
    pub suite: &'static str,
    pub test: &'static str,
    pub reason: &'static str,
}

pub struct TestReport {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub failures: Vec<TestFailure>,
}

impl TestReport {
    pub fn new() -> Self {
        Self {
            total: 0,
            passed: 0,
            failed: 0,
            failures: Vec::new(),
        }
    }

    pub fn pass_rate_percent(&self) -> usize {
        if self.total == 0 {
            return 100;
        }
        (self.passed * 100) / self.total
    }
}

#[derive(Clone)]
pub struct VerifyCheck {
    pub name: &'static str,
    pub passed: bool,
    pub detail: &'static str,
}

impl VerifyCheck {
    pub fn pass(name: &'static str, detail: &'static str) -> Self {
        Self {
            name,
            passed: true,
            detail,
        }
    }

    pub fn fail(name: &'static str, detail: &'static str) -> Self {
        Self {
            name,
            passed: false,
            detail,
        }
    }
}

#[derive(Clone)]
pub struct VerifyReport {
    pub target: &'static str,
    pub checks: Vec<VerifyCheck>,
}

impl VerifyReport {
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }
}
