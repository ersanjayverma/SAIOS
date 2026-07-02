use crate::heap;
use crate::kernel::driver;
use crate::kernel::event;
use crate::kernel::process;
use crate::pmm;
use crate::saifs;
use crate::scheduler;
use crate::timer;

#[derive(Copy, Clone, Debug, Default)]
pub struct TelemetrySnapshot {
    pub cpu_logical: u8,
    pub ram_mb: usize,
    pub heap_total_kb: usize,
    pub heap_used_kb: usize,
    pub scheduler_threads: usize,
    pub irq_total: u64,
    pub driver_count: usize,
    pub process_count: usize,
    pub mount_count: usize,
    pub event_total: u64,
}

pub fn snapshot() -> TelemetrySnapshot {
    let heap_stats = heap::stats();
    let event_counters = event::counters();

    TelemetrySnapshot {
        cpu_logical: hal::arch::x86_64::cpuid::logical_processors(),
        ram_mb: pmm::total_ram_mb(),
        heap_total_kb: heap_stats.total / 1024,
        heap_used_kb: heap_stats.used / 1024,
        scheduler_threads: scheduler::threads().len(),
        irq_total: timer::ticks(),
        driver_count: driver::count(),
        process_count: process::jobs().len(),
        mount_count: saifs::mounts().len(),
        event_total: event_counters.total,
    }
}
