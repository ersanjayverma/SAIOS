use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::TimerHal;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("hal::timer::tsc currently supports only x86_64");

pub struct TscTimer {
    enabled: AtomicBool,
    periodic_hz: AtomicU64,
    oneshot_ns: AtomicU64,
    frequency_hz: AtomicU64,
}

impl TscTimer {
    pub const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            periodic_hz: AtomicU64::new(0),
            oneshot_ns: AtomicU64::new(0),
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

impl TimerHal for TscTimer {
    fn name(&self) -> &'static str {
        "tsc"
    }

    fn frequency_hz(&self) -> u64 {
        self.frequency_hz.load(Ordering::Relaxed)
    }

    fn counter(&self) -> u64 {
        // TSC is a free-running cycle counter on x86_64.
        unsafe { core::arch::x86_64::_rdtsc() }
    }

    fn set_periodic(&mut self, hz: u64) {
        self.periodic_hz.store(hz, Ordering::Relaxed);
    }

    fn set_oneshot(&mut self, ns: u64) {
        self.oneshot_ns.store(ns, Ordering::Relaxed);
    }

    fn enable(&mut self) {
        self.enabled.store(true, Ordering::Relaxed);
    }

    fn disable(&mut self) {
        self.enabled.store(false, Ordering::Relaxed);
    }

    fn acknowledge(&mut self) {}
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
