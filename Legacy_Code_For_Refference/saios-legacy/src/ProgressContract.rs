//! Progress attribution contract.
//!
//! This contract distinguishes a busy system that is doing work from a busy
//! system that is spinning without observable progress. Heartbeats prove timer
//! liveness; they are not, by themselves, work progress.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub heartbeat: u64,
    pub timer_irqs: u64,
    pub boot_ticks: u64,
    pub scheduler_progress: u64,
    pub context_switches: u64,
    pub kds_events: u64,
    pub kds_metrics: u64,
    pub kds_state: u64,
    pub run_queue_fingerprint: u64,
    pub scheduler_snapshot_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressDelta {
    pub scheduler_progress_changed: bool,
    pub context_switches_changed: bool,
    pub kds_events_changed: bool,
    pub kds_metrics_changed: bool,
    pub kds_state_changed: bool,
    pub run_queue_changed: bool,
}

impl ProgressDelta {
    pub fn work_progressing(self) -> bool {
        self.scheduler_progress_changed
            || self.context_switches_changed
            || self.kds_events_changed
            || self.kds_metrics_changed
            || self.kds_state_changed
            || self.run_queue_changed
    }
}

pub struct ProgressContract;

impl ProgressContract {
    pub fn snapshot() -> ProgressSnapshot {
        let stats = crate::kds::stats();
        let (run_queue_fingerprint, scheduler_snapshot_available) = scheduler_fingerprint();
        ProgressSnapshot {
            heartbeat: crate::diag::heartbeat::HEARTBEAT_LAST_TICK
                .load(core::sync::atomic::Ordering::Relaxed),
            timer_irqs: crate::interrupts::TIMER_IRQS.load(core::sync::atomic::Ordering::Relaxed),
            boot_ticks: crate::shell::commands::boot_ticks(),
            scheduler_progress: crate::kds::aggregate_value(
                crate::kds::KdsSubsystem::Scheduler,
                crate::kds::KdsMetricId::SchedulerProgress,
            )
            .unwrap_or_else(|| {
                crate::kds::count_metrics(crate::kds::KdsMetricId::SchedulerProgress)
            }),
            context_switches: crate::kds::aggregate_value(
                crate::kds::KdsSubsystem::Scheduler,
                crate::kds::KdsMetricId::ContextSwitches,
            )
            .unwrap_or_else(|| crate::kds::count_metrics(crate::kds::KdsMetricId::ContextSwitches)),
            kds_events: stats.events.records,
            kds_metrics: stats.metrics.records,
            kds_state: stats.state.records,
            run_queue_fingerprint,
            scheduler_snapshot_available,
        }
    }

    pub fn delta(previous: ProgressSnapshot, current: ProgressSnapshot) -> ProgressDelta {
        ProgressDelta {
            scheduler_progress_changed: current.scheduler_progress != previous.scheduler_progress,
            context_switches_changed: current.context_switches != previous.context_switches,
            kds_events_changed: current.kds_events != previous.kds_events,
            kds_metrics_changed: current.kds_metrics != previous.kds_metrics,
            kds_state_changed: current.kds_state != previous.kds_state,
            run_queue_changed: current.scheduler_snapshot_available
                && previous.scheduler_snapshot_available
                && current.run_queue_fingerprint != previous.run_queue_fingerprint,
        }
    }

    pub fn emit_forward_progress_stall(
        secs_stalled: u64,
        last_progress: u64,
        heartbeat: u64,
        current_pid: u32,
        cr3: u64,
    ) {
        let (pid, owner) = if current_pid == 0 {
            (None, crate::observability_contract::ResourceOwner::Unknown)
        } else {
            (
                Some(current_pid),
                crate::observability_contract::ResourceOwner::Pid(current_pid),
            )
        };
        crate::observability_contract::ObservabilityContract::emit(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::DiagnosticDump,
                contract: crate::observability_contract::ContractId::Watchdog,
                tag: crate::observability_contract::ObservationTag::ForwardProgressStall,
                reason: "watchdog detected no forward progress",
                outcome: crate::observability_contract::ObservationOutcome::Failed,
                resource: crate::observability_contract::ResourceClass::Scheduler,
                owner,
                cpu: Some(crate::process::table::cpu_idx()),
                pid,
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence: [secs_stalled, last_progress, heartbeat, cr3],
            },
        );
    }
}

fn scheduler_fingerprint() -> (u64, bool) {
    let Some(table) = crate::process::table::TABLE.try_lock() else {
        return (0, false);
    };
    let snapshot = table.scheduler_snapshot();
    let mut hash = snapshot.run_queue.len() as u64;
    for (index, pid) in snapshot.run_queue.iter().enumerate() {
        hash ^= (*pid as u64).rotate_left((index as u32) & 31);
    }
    for (index, pid) in snapshot.current.iter().enumerate() {
        hash ^= (*pid as u64).rotate_left(((index + 8) as u32) & 31);
    }
    for (index, pid) in snapshot.idle.iter().enumerate() {
        hash ^= (*pid as u64).rotate_left(((index + 16) as u32) & 31);
    }
    hash ^= (table.procs.len() as u64).rotate_left(48);
    hash ^= (table.zombies.len() as u64).rotate_left(56);
    (hash, true)
}
