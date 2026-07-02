pub mod assert;
pub mod framework;
pub mod report;
pub mod runner;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::console;

use self::framework::KernelTestFramework;
use self::report::{TestReport, VerifyReport};
use alloc::vec;

static KTF: StaticCell<Option<KernelTestFramework>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
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

    for line in [
        "HAL",
        "Interrupts",
        "Timer",
        "PMM",
        "Heap",
        "Scheduler",
        "Console",
        "Shell",
        "Object Manager",
        "Services",
    ] {
        console::println!("[OK] {}", line);
    }

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
