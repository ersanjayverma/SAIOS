use alloc::string::String;
use alloc::vec::Vec;

use crate::kernel::driver;
use crate::kernel::event;
use crate::kernel::telemetry;
use crate::ksf;

pub fn health() -> Vec<String> {
    let t = telemetry::snapshot();
    let mut out = Vec::new();
    out.push(alloc::format!("health: cpu.logical={}", t.cpu_logical));
    out.push(alloc::format!("health: ram.mb={}", t.ram_mb));
    out.push(alloc::format!("health: heap.used.kb={}", t.heap_used_kb));
    out.push(alloc::format!("health: irq.total={}", t.irq_total));
    out.push(alloc::format!("health: events.total={}", t.event_total));
    out
}

pub fn diagnose() -> Vec<String> {
    let mut out = Vec::new();
    for d in driver::drivers() {
        if matches!(d.status, crate::kernel::driver::DriverStatus::Faulted) {
            out.push(alloc::format!("diag: faulted driver {}", d.name));
        }
        if let Some(err) = d.last_error {
            out.push(alloc::format!("diag: driver {} last_error={}", d.name, err));
        }
    }
    if out.is_empty() {
        out.push("diag: no active driver faults".into());
    }
    out
}

pub fn explain(target: &str) -> Vec<String> {
    let mut out = Vec::new();
    if target.eq_ignore_ascii_case("scheduler") {
        out.push("scheduler: preemptive tick-driven round-robin".into());
        out.push(alloc::format!("scheduler: threads={}", crate::scheduler::threads().len()));
    } else if target.eq_ignore_ascii_case("memory") {
        let h = crate::heap::stats();
        out.push("memory: PMM + growable kernel heap".into());
        out.push(alloc::format!("memory: heap.total.kb={}", h.total / 1024));
        out.push(alloc::format!("memory: heap.used.kb={}", h.used / 1024));
    } else {
        out.push("explain: supported targets are scheduler|memory".into());
    }
    out
}

pub fn service_health() -> Vec<String> {
    let mut out = Vec::new();
    for svc in ksf::list() {
        out.push(alloc::format!(
            "service {} v{} state={:?} health={:?}",
            svc.name, svc.version, svc.state, svc.health
        ));
    }
    let events = event::recent(8);
    for e in events {
        out.push(alloc::format!(
            "event#{} {} {} {}",
            e.seq,
            e.kind.as_str(),
            e.source,
            e.detail
        ));
    }
    out
}
