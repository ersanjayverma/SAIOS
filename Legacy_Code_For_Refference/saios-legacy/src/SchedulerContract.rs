//! Canonical scheduler authority.
//!
//! Scheduler storage may contain queues and per-CPU slots, but callers must
//! express intent through enqueue, block, wake, exit, and pick operations.

use crate::process::table::{MAX_CPUS, ProcessTable, SchedulerSnapshot};
use crate::process::{Process, ProcessState};
use core::sync::atomic::{AtomicU64, Ordering};

static STALE_ON_CPU_REPAIRS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerAction {
    Enqueue,
    Block,
    Wake,
    Exit,
    PickNext,
    FinishSwitch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerOwnership {
    pub pid: u32,
    pub on_cpu: bool,
    pub queued: bool,
    pub current_cpu: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerTopology {
    SharedFifoRoundRobin,
    PerCpuQueues,
    WorkStealing,
    Priority,
    Affinity,
    NumaAware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerPlacement {
    pub cpu: usize,
    pub is_bsp: bool,
    pub numa_node: Option<usize>,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerClass {
    Deadline = 1,
    FifoRealtime = 2,
    RoundRobinRealtime = 3,
    CfsNormal = 4,
    Idle = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerCapabilityView {
    pub active_topology: SchedulerTopology,
    pub default_class: SchedulerClass,
    pub has_per_cpu_run_queues: bool,
    pub has_cfs_vruntime: bool,
    pub has_realtime_classes: bool,
    pub has_affinity_filtering: bool,
    pub has_numa_metadata_scoring: bool,
    pub has_numa_balancer: bool,
    pub has_finish_switch_bookkeeping: bool,
}

pub struct SchedulerContract;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezeDiagnosticView {
    pub watchdog_stall_events: u64,
    pub scheduler_stall_events: u64,
    pub heartbeat_metrics: u64,
    pub scheduler_progress_metrics: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuEvidenceView {
    pub timestamp: u64,
    pub cpu: u32,
    pub subsystem: &'static str,
    pub metric: &'static str,
    pub value: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchdogStallView {
    pub cpu: u32,
    pub pid: u32,
    pub seconds_without_progress: u64,
    pub event_id: u64,
}

fn is_current_slot_state(state: &ProcessState) -> bool {
    matches!(
        state,
        ProcessState::Running | ProcessState::Blocked | ProcessState::Zombie
    )
}

impl SchedulerContract {
    pub const ACTIVE_TOPOLOGY: SchedulerTopology = SchedulerTopology::SharedFifoRoundRobin;

    pub fn capability_view() -> SchedulerCapabilityView {
        SchedulerCapabilityView {
            active_topology: Self::ACTIVE_TOPOLOGY,
            default_class: SchedulerClass::RoundRobinRealtime,
            has_per_cpu_run_queues: false,
            has_cfs_vruntime: false,
            has_realtime_classes: false,
            has_affinity_filtering: true,
            has_numa_metadata_scoring: true,
            has_numa_balancer: false,
            has_finish_switch_bookkeeping: true,
        }
    }

    pub fn freeze_diagnostic_view() -> FreezeDiagnosticView {
        crate::kds::flush_aggregates();
        FreezeDiagnosticView {
            watchdog_stall_events: crate::kds::count_events(
                crate::kds::KdsEventType::WatchdogCpuStall,
            ),
            scheduler_stall_events: crate::kds::count_events(
                crate::kds::KdsEventType::SchedulerStall,
            ),
            heartbeat_metrics: crate::kds::count_metrics(crate::kds::KdsMetricId::CpuHeartbeat),
            scheduler_progress_metrics: crate::kds::count_metrics(
                crate::kds::KdsMetricId::SchedulerProgress,
            ) + crate::kds::count_metrics(
                crate::kds::KdsMetricId::ContextSwitches,
            ),
        }
    }

    pub fn for_each_cpu_evidence_view(
        cpu: Option<u32>,
        limit: usize,
        mut visit: impl FnMut(CpuEvidenceView),
    ) {
        crate::kds::flush_aggregates();
        crate::kds::for_each_metric(limit, |record| {
            if cpu.is_none_or(|target| record.metadata.cpu_id == target) {
                visit(CpuEvidenceView {
                    timestamp: record.metadata.timestamp,
                    cpu: record.metadata.cpu_id,
                    subsystem: crate::kds::subsystem_name(record.metadata.subsystem),
                    metric: crate::kds::metric_name(record.metric_id),
                    value: record.value,
                });
            }
        });
    }

    pub fn latest_watchdog_stall_view() -> Option<WatchdogStallView> {
        crate::kds::latest_event(crate::kds::KdsEventType::WatchdogCpuStall).map(|record| {
            WatchdogStallView {
                cpu: record.metadata.cpu_id,
                pid: record.metadata.process_id,
                seconds_without_progress: record.payload[0],
                event_id: record.event_id,
            }
        })
    }

    pub fn stale_on_cpu_repair_count() -> u64 {
        STALE_ON_CPU_REPAIRS.load(Ordering::Relaxed)
    }

    /// Detect stale on_cpu ownership. With proper contract paths (claim_contract_current,
    /// release_cpu_owner) this should NEVER fire. If it does, that's a real bug — Red Ring.
    /// F-SCHED-04: converted from silent repair to diagnostic-only assertion.
    pub fn recover_stale_on_cpu_ownership(table: &mut ProcessTable, tag: &'static str) -> usize {
        let snapshot = table.scheduler_snapshot();
        for pid in table.pids() {
            let stale_cpu = table.procs.get(&pid).and_then(|proc| {
                if !proc.is_on_cpu() {
                    return None;
                }
                let cpu = proc.cpu_owner()?;
                if snapshot.current.get(cpu).copied() == Some(pid)
                    || snapshot.prev.get(cpu).copied() == Some(pid)
                {
                    None
                } else {
                    Some(cpu)
                }
            });
            if let Some(cpu) = stale_cpu {
                let repair_count = STALE_ON_CPU_REPAIRS.fetch_add(1, Ordering::Relaxed) + 1;
                crate::serial_println!(
                    "[sched-contract] FATAL stale on_cpu #{} pid={} cpu={} current={} tag={}",
                    repair_count,
                    pid,
                    cpu,
                    snapshot.current.get(cpu).copied().unwrap_or(0),
                    tag
                );
                Self::dump_table(
                    table,
                    "stale_on_cpu_violation",
                    "on_cpu pid is not current on owner CPU — contract violation",
                );
                crate::kds::kds_event(
                    crate::kds::KdsSubsystem::Scheduler,
                    crate::kds::KdsEventType::State,
                    crate::kds::KdsSeverity::Fatal,
                    [pid as u64, cpu as u64, repair_count, 0],
                );
                // Red Ring: stale ownership should be impossible with contract paths.
                crate::reliability_contract::ReliabilityContract::enter_red_ring(
                    crate::reliability_contract::RedRingEvidence {
                        cause: crate::reliability_contract::RedRingCause::ContractViolation,
                        evidence_event_id: pid as u64,
                        invariant_id: cpu as u64,
                        detail: repair_count,
                    },
                );
                return 1;
            }
        }
        0
    }

    pub fn insert_detached(table: &mut ProcessTable, proc: Process, tag: &'static str) {
        let pid = proc.pid;
        table.insert_detached(proc);
        Self::emit_transition(
            crate::kds::KdsEventType::TaskCreate,
            pid,
            0,
            "sched.enqueue",
            table,
        );
        Self::validate_table_or_panic(table, tag);
    }

    pub fn enqueue_runnable(
        table: &mut ProcessTable,
        proc: Process,
        reason: &'static str,
        caller: &'static str,
    ) {
        let pid = proc.pid;
        let is_shell = proc.name == "shell";
        if is_shell {
            crate::serial_println!(
                "[sched] enqueue pid={} name=shell state={:?} boot_cpu_affine={} allowed={:#x} preferred={:?} numa={:?} kernel_rsp={:#x} before_queue_len={}",
                pid,
                proc.state(),
                proc.boot_cpu_affine,
                proc.scheduling.allowed_cpus,
                proc.scheduling.preferred_cpu,
                proc.scheduling.numa_node,
                proc.kernel_rsp,
                table.scheduler_snapshot().run_queue.len()
            );
        }
        table.insert_with_reason(proc, reason, caller);
        if is_shell {
            let snapshot = table.scheduler_snapshot();
            crate::serial_println!(
                "[sched] enqueue pid={} name=shell queue_len={} queued={}",
                pid,
                snapshot.run_queue.len(),
                snapshot.run_queue.contains(&pid)
            );
        }
        Self::emit_transition(
            crate::kds::KdsEventType::TaskUnblock,
            pid,
            0,
            "sched.enqueue",
            table,
        );
        Self::validate_table_or_panic(table, reason);
    }

    pub fn register_running_current(
        table: &mut ProcessTable,
        cpu: usize,
        pid: u32,
        make_idle: bool,
        tag: &'static str,
    ) {
        crate::serial_println!(
            "[sched-contract] register current table begin tag={} cpu={} pid={}",
            tag,
            cpu,
            pid
        );
        table.register_contract_running_current(cpu, pid, make_idle);
        crate::serial_println!(
            "[sched-contract] register current emit begin tag={} cpu={} pid={}",
            tag,
            cpu,
            pid
        );
        Self::emit_transition(
            crate::kds::KdsEventType::ContextSwitch,
            pid,
            cpu as u64,
            "sched.switch",
            table,
        );
        crate::serial_println!(
            "[sched-contract] register current validate begin tag={} cpu={} pid={}",
            tag,
            cpu,
            pid
        );
        Self::validate_table_or_panic(table, tag);
        crate::serial_println!(
            "[sched-contract] register current complete tag={} cpu={} pid={}",
            tag,
            cpu,
            pid
        );
    }

    pub fn claim_next_on_cpu(
        table: &mut ProcessTable,
        cpu: usize,
        next: u32,
        prev: u32,
        tag: &'static str,
    ) {
        table.claim_contract_current(cpu, next, prev);
        Self::emit_transition(
            crate::kds::KdsEventType::ContextSwitch,
            next,
            ((prev as u64) << 32) | cpu as u64,
            "sched.pick",
            table,
        );
    }

    pub fn release_cpu_owner(table: &mut ProcessTable, pid: u32, tag: &'static str) {
        table.clear_contract_cpu_owner(pid);
        Self::emit_transition(
            crate::kds::KdsEventType::State,
            pid,
            0,
            "sched.finish",
            table,
        );
    }

    pub fn requeue_after_switch(table: &mut ProcessTable, pid: u32, tag: &'static str) {
        table.enqueue_recovered_if_absent(pid);
        Self::emit_transition(
            crate::kds::KdsEventType::TaskUnblock,
            pid,
            0,
            "sched.finish",
            table,
        );
        Self::validate_table_or_panic(table, tag);
    }

    pub fn remove_from_run_queue(table: &mut ProcessTable, pid: u32, tag: &'static str) {
        table.remove_from_run_queue(pid);
        Self::emit_transition(
            crate::kds::KdsEventType::State,
            pid,
            0,
            "sched.block",
            table,
        );
        Self::validate_table_or_panic(table, tag);
    }

    pub fn block_current(table: &mut ProcessTable, tag: &'static str) -> Option<u32> {
        let blocked = table.block_current();
        if let Some(pid) = blocked {
            Self::emit_transition(
                crate::kds::KdsEventType::TaskBlock,
                pid,
                0,
                "sched.block",
                table,
            );
        }
        Self::validate_table_or_panic(table, tag);
        blocked
    }

    pub fn wake_pid(table: &mut ProcessTable, pid: u32, tag: &'static str) -> bool {
        let woke = table.wake_pid(pid);
        if woke {
            Self::emit_transition(
                crate::kds::KdsEventType::TaskUnblock,
                pid,
                0,
                "sched.wake",
                table,
            );
            Self::validate_table_or_panic(table, tag);
        }
        woke
    }

    pub fn pick_next(table: &mut ProcessTable, tag: &'static str) -> Option<u32> {
        let next = table.pick_next();
        let shell_pid = table
            .procs
            .iter()
            .find_map(|(pid, proc)| (proc.name == "shell").then_some(*pid));
        let shell_queued =
            shell_pid.is_some_and(|pid| table.scheduler_snapshot().run_queue.contains(&pid));
        if let Some(pid) = next {
            let name = table
                .procs
                .get(&pid)
                .map(|proc| proc.name.as_str())
                .unwrap_or("<missing>");
            if name == "shell" || shell_queued {
                crate::serial_println!(
                    "[sched] pick pid={} name={} shell_pid={:?} shell_queued={}",
                    pid,
                    name,
                    shell_pid,
                    shell_queued
                );
            }
            Self::emit_transition(
                crate::kds::KdsEventType::ContextSwitch,
                pid,
                0,
                "sched.pick",
                table,
            );
        } else if shell_queued {
            crate::serial_println!(
                "[sched] pick none shell_pid={:?} shell_queued=true",
                shell_pid
            );
        }
        Self::validate_table_or_panic(table, tag);
        next
    }

    fn emit_transition(
        event_type: crate::kds::KdsEventType,
        pid: u32,
        aux: u64,
        tag: &'static str,
        table: &ProcessTable,
    ) {
        let snapshot = table.scheduler_snapshot();
        crate::observability_contract::ObservabilityContract::emit_as_kds_event(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::Scheduler,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason: "",
                outcome: crate::observability_contract::ObservationOutcome::Success,
                resource: crate::observability_contract::ResourceClass::Scheduler,
                owner: crate::observability_contract::ResourceOwner::Pid(pid),
                cpu: Some(crate::process::table::cpu_idx()),
                pid: Some(pid),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence: [
                    pid as u64,
                    snapshot.run_queue.len() as u64,
                    aux,
                    tag.as_ptr() as u64,
                ],
            },
            event_type,
            crate::kds::KdsSeverity::Trace,
        );
    }

    pub fn active_topology() -> SchedulerTopology {
        Self::ACTIVE_TOPOLOGY
    }

    pub fn numa_node_for_cpu(cpu: usize) -> Option<usize> {
        if cpu >= MAX_CPUS {
            return None;
        }
        crate::smp::numa_node_for_cpu(cpu)
    }

    pub fn placement_for_cpu(cpu: usize) -> SchedulerPlacement {
        SchedulerPlacement {
            cpu,
            is_bsp: cpu == 0,
            numa_node: Self::numa_node_for_cpu(cpu),
        }
    }

    pub fn can_run_on_cpu(
        proc: &Process,
        placement: SchedulerPlacement,
    ) -> Result<(), &'static str> {
        if placement.cpu >= MAX_CPUS {
            return Err("scheduler: target CPU is out of range");
        }
        if proc.scheduling.allowed_cpus & (1u64 << placement.cpu) == 0 {
            return Err("scheduler: process is not allowed on target CPU");
        }
        if proc.boot_cpu_affine && !placement.is_bsp {
            return Err("scheduler: boot-CPU-affine process cannot run on another CPU");
        }
        Ok(())
    }

    pub fn placement_score(proc: &Process, placement: SchedulerPlacement) -> Option<u8> {
        Self::can_run_on_cpu(proc, placement).ok()?;
        let mut score = 1;
        if proc.scheduling.numa_node.is_some() && proc.scheduling.numa_node == placement.numa_node {
            score += 2;
        }
        if proc.scheduling.preferred_cpu == Some(placement.cpu) {
            score += 1;
        }
        Some(score)
    }

    pub fn validate_ownership(ownership: SchedulerOwnership) -> Result<(), &'static str> {
        if ownership.pid == 0 {
            return Err("scheduler: pid is empty");
        }
        if ownership.on_cpu && ownership.queued {
            return Err("scheduler: on-CPU process must not be queued");
        }
        if ownership.on_cpu && ownership.current_cpu.is_none() {
            return Err("scheduler: on-CPU process has no CPU owner");
        }
        Ok(())
    }

    pub fn validate_table(table: &ProcessTable, tag: &'static str) -> Result<(), &'static str> {
        let snapshot = table.scheduler_snapshot();
        Self::validate_run_queue(table, &snapshot, tag)?;
        Self::validate_current_slots(table, &snapshot, tag)?;
        Self::validate_on_cpu_ownership(table, &snapshot, tag)?;
        Ok(())
    }

    pub fn validate_table_or_panic(table: &ProcessTable, tag: &'static str) {
        if let Err(reason) = Self::validate_table(table, tag) {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Scheduler,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Scheduler,
                crate::observability_contract::ResourceOwner::Cpu(crate::process::table::cpu_idx()),
                [
                    table.scheduler_snapshot().run_queue.len() as u64,
                    table.procs.len() as u64,
                    table.zombies.len() as u64,
                    0,
                ],
            );
            crate::serial_println!("[sched-contract] {} violation: {}", tag, reason);
            Self::dump_table(table, tag, reason);
            panic!("[sched-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn dump_table(table: &ProcessTable, tag: &'static str, reason: &'static str) {
        let snapshot = table.scheduler_snapshot();
        crate::serial_println!(
            "[sched-contract] dump tag={} reason={} cpu={} queue_len={} procs={} zombies={}",
            tag,
            reason,
            crate::process::table::cpu_idx(),
            snapshot.run_queue.len(),
            table.procs.len(),
            table.zombies.len()
        );
        crate::serial_println!(
            "[sched-contract] current={:?} idle={:?} prev={:?}",
            snapshot.current,
            snapshot.idle,
            snapshot.prev
        );
        crate::serial_println!("[sched-contract] run_queue={:?}", snapshot.run_queue);
        for proc in table.procs.values() {
            crate::serial_println!(
                "[sched-contract] proc pid={} ppid={} name={} state={:?} on_cpu={} cpu={:?} allowed={:#x} preferred={:?} numa={:?} rip={:#x} rsp={:#x} ktop={:#x} krsp={:#x} pml4={:#x}",
                proc.pid,
                proc.parent_pid,
                proc.name.as_str(),
                proc.state(),
                proc.is_on_cpu(),
                proc.cpu_owner(),
                proc.scheduling.allowed_cpus,
                proc.scheduling.preferred_cpu,
                proc.scheduling.numa_node,
                proc.rip,
                proc.rsp,
                proc.kernel_stack_top(),
                proc.kernel_rsp,
                proc.address_space_pml4()
            );
        }
    }

    fn validate_run_queue(
        table: &ProcessTable,
        snapshot: &SchedulerSnapshot,
        tag: &'static str,
    ) -> Result<(), &'static str> {
        let mut i = 0usize;
        while i < snapshot.run_queue.len() {
            let pid = snapshot.run_queue[i];
            if pid == 0 {
                crate::serial_println!("[sched-contract] {} queued empty pid", tag);
                return Err("queued pid is empty");
            }
            let Some(proc) = table.procs.get(&pid) else {
                crate::serial_println!("[sched-contract] {} queued missing pid={}", tag, pid);
                return Err("queued pid is not in process table");
            };
            if proc.is_on_cpu() {
                crate::serial_println!("[sched-contract] {} queued on_cpu pid={}", tag, pid);
                return Err("queued pid is marked on_cpu");
            }
            if snapshot.current.contains(&pid) {
                crate::serial_println!("[sched-contract] {} queued current pid={}", tag, pid);
                return Err("queued pid is current on a CPU");
            }
            if !matches!(proc.state(), ProcessState::Ready | ProcessState::Running) {
                crate::serial_println!(
                    "[sched-contract] {} queued non-runnable pid={} state={:?}",
                    tag,
                    pid,
                    proc.state()
                );
                return Err("queued pid is not runnable");
            }
            if snapshot.run_queue[..i].contains(&pid) {
                crate::serial_println!("[sched-contract] {} duplicate queued pid={}", tag, pid);
                return Err("pid is queued more than once");
            }
            i += 1;
        }
        Ok(())
    }

    fn validate_current_slots(
        table: &ProcessTable,
        snapshot: &SchedulerSnapshot,
        tag: &'static str,
    ) -> Result<(), &'static str> {
        let mut cpu = 0usize;
        while cpu < MAX_CPUS {
            let pid = snapshot.current[cpu];
            if pid == 0 {
                cpu += 1;
                continue;
            }
            if snapshot.current[..cpu].contains(&pid) {
                crate::serial_println!("[sched-contract] {} duplicate current pid={}", tag, pid);
                return Err("pid is current on more than one CPU");
            }
            let Some(proc) = table.procs.get(&pid) else {
                crate::serial_println!(
                    "[sched-contract] {} current missing cpu={} pid={}",
                    tag,
                    cpu,
                    pid
                );
                return Err("current pid is not in process table");
            };
            if !proc.is_on_cpu() {
                crate::serial_println!(
                    "[sched-contract] {} current not on_cpu cpu={} pid={}",
                    tag,
                    cpu,
                    pid
                );
                return Err("current pid is not marked on_cpu");
            }
            if proc.cpu_owner() != Some(cpu) {
                crate::serial_println!(
                    "[sched-contract] {} current cpu mismatch cpu={} pid={} proc_cpu={:?}",
                    tag,
                    cpu,
                    pid,
                    proc.cpu_owner()
                );
                return Err("current pid CPU owner is wrong");
            }
            if cpu != 0 && proc.boot_cpu_affine {
                crate::serial_println!(
                    "[sched-contract] {} BSP-only pid current on AP cpu={} pid={}",
                    tag,
                    cpu,
                    pid
                );
                return Err("BSP-only pid is current on an AP");
            }
            if !is_current_slot_state(proc.state()) {
                crate::serial_println!(
                    "[sched-contract] {} current not in handoff-safe state cpu={} pid={} state={:?}",
                    tag,
                    cpu,
                    pid,
                    proc.state()
                );
                return Err("current pid is not in a handoff-safe state");
            }
            cpu += 1;
        }
        Ok(())
    }

    fn validate_on_cpu_ownership(
        table: &ProcessTable,
        snapshot: &SchedulerSnapshot,
        tag: &'static str,
    ) -> Result<(), &'static str> {
        for proc in table.procs.values() {
            if !proc.is_on_cpu() && proc.cpu_owner().is_some() {
                crate::serial_println!(
                    "[sched-contract] {} off_cpu with owner pid={} cpu={:?} state={:?}",
                    tag,
                    proc.pid,
                    proc.cpu_owner(),
                    proc.state()
                );
                return Err("off_cpu pid has stale CPU owner");
            }
            if !proc.is_on_cpu() {
                continue;
            }
            let Some(cpu) = proc.cpu_owner() else {
                crate::serial_println!(
                    "[sched-contract] {} on_cpu without cpu pid={}",
                    tag,
                    proc.pid
                );
                return Err("on_cpu pid has no CPU owner");
            };
            if cpu >= MAX_CPUS {
                crate::serial_println!(
                    "[sched-contract] {} invalid cpu pid={} cpu={}",
                    tag,
                    proc.pid,
                    cpu
                );
                return Err("on_cpu pid has invalid CPU owner");
            }
            if snapshot.current[cpu] != proc.pid {
                crate::serial_println!(
                    "[sched-contract] {} on_cpu not current pid={} cpu={} current={} prev={} state={:?}",
                    tag,
                    proc.pid,
                    cpu,
                    snapshot.current[cpu],
                    snapshot.prev[cpu],
                    proc.state()
                );
                return Err("on_cpu pid is not current on its CPU");
            }
        }
        Ok(())
    }
}
