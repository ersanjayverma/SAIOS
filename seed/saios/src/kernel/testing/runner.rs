use alloc::vec::Vec;

use super::framework::KernelTestFramework;
use super::report::{TestFailure, TestReport, VerifyReport};

pub fn run_all(framework: &KernelTestFramework) -> TestReport {
    let mut report = TestReport::new();

    for suite in framework.suites() {
        for test in suite.tests {
            report.total += 1;
            match (test.run)() {
                Ok(()) => report.passed += 1,
                Err(reason) => {
                    report.failed += 1;
                    report.failures.push(TestFailure {
                        suite: suite.name,
                        test: test.name,
                        reason,
                    });
                }
            }
        }
    }

    report
}

pub fn run_suite(
    framework: &KernelTestFramework,
    suite_name: &str,
) -> Result<TestReport, &'static str> {
    let suite = framework
        .find_suite(suite_name)
        .ok_or("unknown test suite")?;
    let mut report = TestReport::new();

    for test in suite.tests {
        report.total += 1;
        match (test.run)() {
            Ok(()) => report.passed += 1,
            Err(reason) => {
                report.failed += 1;
                report.failures.push(TestFailure {
                    suite: suite.name,
                    test: test.name,
                    reason,
                });
            }
        }
    }

    Ok(report)
}

pub fn verify_all(framework: &KernelTestFramework) -> Vec<VerifyReport> {
    framework.verifiers().iter().map(|v| (v.run)()).collect()
}

pub fn verify_one(
    framework: &KernelTestFramework,
    target: &str,
) -> Result<VerifyReport, &'static str> {
    let verifier = framework
        .find_verifier(target)
        .ok_or("unknown verify target")?;
    Ok((verifier.run)())
}
