use alloc::vec::Vec;

use crate::{console, ksf, memory, object_manager, saifs, scheduler};

use super::report::{VerifyReport};

pub type KtResult = Result<(), &'static str>;
pub type KtTestFn = fn() -> KtResult;
pub type VerifyFn = fn() -> VerifyReport;

#[derive(Copy, Clone)]
pub struct TestCase {
    pub name: &'static str,
    pub run: KtTestFn,
}

impl TestCase {
    pub const fn new(name: &'static str, run: KtTestFn) -> Self {
        Self { name, run }
    }
}

#[derive(Copy, Clone)]
pub struct TestSuite {
    pub name: &'static str,
    pub tests: &'static [TestCase],
}

impl TestSuite {
    pub const fn new(name: &'static str, tests: &'static [TestCase]) -> Self {
        Self { name, tests }
    }
}

#[derive(Copy, Clone)]
pub struct Verifier {
    pub name: &'static str,
    pub run: VerifyFn,
}

impl Verifier {
    pub const fn new(name: &'static str, run: VerifyFn) -> Self {
        Self { name, run }
    }
}

pub struct KernelTestFramework {
    suites: Vec<TestSuite>,
    verifiers: Vec<Verifier>,
}

impl KernelTestFramework {
    pub fn new() -> Self {
        Self {
            suites: Vec::new(),
            verifiers: Vec::new(),
        }
    }

    pub fn register_suite(&mut self, suite: TestSuite) {
        if self.suites.iter().any(|s| s.name.eq_ignore_ascii_case(suite.name)) {
            return;
        }
        self.suites.push(suite);
    }

    pub fn register_verifier(&mut self, verifier: Verifier) {
        if self
            .verifiers
            .iter()
            .any(|v| v.name.eq_ignore_ascii_case(verifier.name))
        {
            return;
        }
        self.verifiers.push(verifier);
    }

    pub fn suites(&self) -> &[TestSuite] {
        &self.suites
    }

    pub fn find_suite(&self, name: &str) -> Option<TestSuite> {
        self.suites
            .iter()
            .copied()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn verifiers(&self) -> &[Verifier] {
        &self.verifiers
    }

    pub fn find_verifier(&self, name: &str) -> Option<Verifier> {
        self.verifiers
            .iter()
            .copied()
            .find(|v| v.name.eq_ignore_ascii_case(name))
    }

    pub fn register_defaults(&mut self) {
        self.register_suite(memory::tests::suite());
        self.register_suite(scheduler::tests::suite());
        self.register_suite(console::tests::suite());
        self.register_suite(object_manager::tests::suite());
        self.register_suite(saifs::tests::suite());

        self.register_verifier(Verifier::new("memory", memory::verify));
        self.register_verifier(Verifier::new("scheduler", scheduler::verify));
        self.register_verifier(Verifier::new("console", console::verify));
        self.register_verifier(Verifier::new("object", object_manager::verify));
        self.register_verifier(Verifier::new("service", ksf::verify));
        self.register_verifier(Verifier::new("saifs", saifs::verify));
    }

    pub fn print_registration_summary(&self) {
        console::println!(
            "KTF loaded suites={} verifiers={}",
            self.suites.len(),
            self.verifiers.len()
        );
    }
}
