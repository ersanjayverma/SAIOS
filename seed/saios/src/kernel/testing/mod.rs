pub mod assert;
pub mod framework;
pub mod report;
pub mod runner;

use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;

use hal::arch::x86_64::sync::StaticCell;

use crate::console;
use crate::kernel::validation;

use self::framework::KernelTestFramework;
use self::report::{TestReport, VerifyReport};
use alloc::vec;

static KTF: StaticCell<Option<KernelTestFramework>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    hal::arch::x86_64::sync::spinlock_acquire(&LOCK);
}

fn unlock() {
    hal::arch::x86_64::sync::spinlock_release(&LOCK);
}

fn with_ktf<R>(f: impl FnOnce(&mut KernelTestFramework) -> R) -> R {
    lock();
    let out = {
        let framework = unsafe {
            let slot = &mut *KTF.get();
            if slot.is_none() {
                let mut built = KernelTestFramework::new();
                built.register_defaults();
                built.print_registration_summary();
                *slot = Some(built);
            }
            slot.as_mut().expect("KTF unavailable")
        };
        f(framework)
    };
    unlock();
    out
}

pub fn run_tests(target: Option<&str>) -> Result<TestReport, &'static str> {
    with_ktf(|ktf| match target {
        None | Some("all") => Ok(runner::run_all(ktf)),
        Some(name) => runner::run_suite(ktf, name),
    })
}

pub fn verify_target(target: Option<&str>) -> Result<Vec<VerifyReport>, &'static str> {
    with_ktf(|ktf| match target {
        None | Some("all") => Ok(runner::verify_all(ktf)),
        Some(name) => Ok(vec![runner::verify_one(ktf, name)?]),
    })
}

pub fn boot_self_test() {
    console::println!("SAIOS Self Test");

    match verify_target(Some("all")) {
        Ok(reports) => {
            let passed = reports.iter().all(|r| r.passed());
            if passed {
                console::println!("All systems operational.");
            } else {
                console::println!("Self-test completed with verification failures.");
            }
        }
        Err(_) => console::println!("Self-test completed with framework error."),
    }
}

pub fn boot_readiness_gate() -> bool {
    let options = validation::ValidateOptions {
        verbose: false,
        perf: false,
        stress: false,
        json: false,
        ready: true,
    };

    let report = validation::run(&options);
    validation::print_report(&report, &options);

    for gate in report.readiness_gate_statuses() {
        let status = if gate.passed {
            "PASS"
        } else if gate.skipped {
            "SKIP"
        } else {
            "FAIL"
        };
        console::println!("[READY] {:<16} {}", gate.label, status);
    }

    report.kernel_ready()
}
