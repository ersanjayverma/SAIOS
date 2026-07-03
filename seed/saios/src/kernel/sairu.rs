use alloc::string::String;
use alloc::vec::Vec;

use crate::kernel::driver;
use crate::kernel::event;
use crate::kernel::telemetry;
use crate::ksf;

pub fn health_score() -> (u8, Vec<String>) {
    let mut score: i32 = 100;
    let mut warnings = Vec::new();

    let services = ksf::list();
    let failed_services = services
        .iter()
        .filter(|s| matches!(s.state, crate::ksf::ServiceState::Failed))
        .count() as i32;
    if failed_services > 0 {
        score -= core::cmp::min(60, failed_services * 20);
        warnings.push(alloc::format!("{} failed service(s)", failed_services));
    }

    let warn_services = services
        .iter()
        .filter(|s| {
            matches!(
                s.health,
                crate::som::HealthState::Warning
                    | crate::som::HealthState::Critical
                    | crate::som::HealthState::Offline
            )
        })
        .count() as i32;
    if warn_services > 0 {
        score -= core::cmp::min(20, warn_services * 3);
        warnings.push(alloc::format!(
            "{} service(s) in warning state",
            warn_services
        ));
    }

    let mut restarted = 0i32;
    let mut faulted_drivers = 0i32;
    for d in driver::drivers() {
        restarted += (d.reload_count > 0) as i32;
        if matches!(d.status, crate::kernel::driver::DriverStatus::Faulted) {
            faulted_drivers += 1;
            if let Some(err) = d.last_error {
                warnings.push(alloc::format!("Driver {} fault: {}", d.name, err));
            } else {
                warnings.push(alloc::format!("Driver {} faulted", d.name));
            }
        }
    }

    if restarted > 0 {
        score -= core::cmp::min(8, restarted);
        warnings.push(alloc::format!("{} driver(s) reloaded", restarted));
    }

    if faulted_drivers > 0 {
        score -= core::cmp::min(30, faulted_drivers * 10);
    }

    let h = crate::heap::stats();
    if h.total > 0 {
        let pct = h.used.saturating_mul(100).checked_div(h.total).unwrap_or(0);
        if pct > 85 {
            score -= 10;
            warnings.push(alloc::format!("Heap pressure high: {}%", pct));
        } else if pct > 70 {
            score -= 5;
            warnings.push(alloc::format!("Heap pressure elevated: {}%", pct));
        }
    }

    if score < 0 {
        score = 0;
    }

    (score as u8, warnings)
}

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
        out.push(alloc::format!(
            "scheduler: threads={}",
            crate::scheduler::threads().len()
        ));
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
            svc.name,
            svc.version,
            svc.state,
            svc.health
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

pub fn recover() -> Vec<String> {
    let mut out = Vec::new();

    for svc in ksf::list() {
        if matches!(svc.state, crate::ksf::ServiceState::Failed) {
            match ksf::restart(svc.name.as_str()) {
                Ok(()) => out.push(alloc::format!("recover: restarted service {}", svc.name)),
                Err(e) => out.push(alloc::format!(
                    "recover: service {} failed ({})",
                    svc.name,
                    e
                )),
            }
        }
    }

    for drv in driver::drivers() {
        if matches!(drv.status, crate::kernel::driver::DriverStatus::Faulted)
            || matches!(drv.status, crate::kernel::driver::DriverStatus::Stopped)
        {
            match driver::reload(drv.name.as_str()) {
                Ok(()) => out.push(alloc::format!("recover: reloaded driver {}", drv.name)),
                Err(e) => out.push(alloc::format!(
                    "recover: driver {} failed ({})",
                    drv.name,
                    e
                )),
            }
        }
    }

    event::clear_stale(64);
    out.push("recover: cleared stale events (kept latest 64)".into());

    for line in diagnose() {
        out.push(alloc::format!("recover: {}", line));
    }

    out
}
