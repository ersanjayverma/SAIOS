use core::sync::atomic::{AtomicU64, Ordering};

use super::{ClockSource, HalDevice};

#[cfg(not(target_arch = "x86_64"))]
compile_error!("hal::timer::tsc currently supports only x86_64");

pub struct TscTimer {
 frequency_hz: AtomicU64,
}

impl TscTimer {
    pub const fn new() -> Self {
        Self {
            frequency_hz: AtomicU64::new(0),
        }
    }

    pub fn init(&self) {
        let hz = detect_tsc_hz();
        if hz != 0 {
            self.frequency_hz.store(hz, Ordering::Relaxed);
        }
    }
}

impl ClockSource for TscTimer {
    fn init(&mut self) {
        self.frequency_hz.store(detect_tsc_hz(), Ordering::Relaxed);
    }
  fn frequency_hz(&self) -> u64 {
        self.frequency_hz.load(Ordering::Relaxed)
    }
    fn counter(&self) -> u64 {
        // TSC is a free-running cycle counter on x86_64.
        unsafe { core::arch::x86_64::_rdtsc() }
    }
}
impl HalDevice for TscTimer {
    fn name(&self) -> &'static str {
        "tsc"
    }
}

fn detect_tsc_hz() -> u64 {
    let max_leaf = core::arch::x86_64::__cpuid(0).eax;

    if max_leaf >= 0x15 {
        let leaf15 = core::arch::x86_64::__cpuid_count(0x15, 0);
        if leaf15.eax != 0 && leaf15.ebx != 0 && leaf15.ecx != 0 {
            return (leaf15.ecx as u64).saturating_mul(leaf15.ebx as u64) / leaf15.eax as u64;
        }
    }

    if max_leaf >= 0x16 {
        let leaf16 = core::arch::x86_64::__cpuid_count(0x16, 0);
        if leaf16.eax != 0 {
            return (leaf16.eax as u64).saturating_mul(1_000_000);
        }
    }

    0
}
