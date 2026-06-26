//! Canonical observability and diagnostic-evidence authority.
//!
//! Runtime contracts own behavior. Observability owns the evidence shape used to
//! understand, explain, diagnose, predict, and improve that behavior. This file
//! is intentionally no-allocation friendly: panic and fault paths can validate
//! and emit compact records without heap use or contract-specific formatting.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::process::table::MAX_CPUS;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractId {
    AddressSpace,
    Capability,
    Configuration,
    Debug,
    Driver,
    Execution,
    Identity,
    Interrupt,
    Kds,
    Memory,
    Network,
    Observability,
    Power,
    Process,
    Resource,
    Sairu,
    Security,
    Scheduler,
    Syscall,
    Vfs,
    Watchdog,
}

pub use ContractId as ContractOwner;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservableEvent {
    ContractViolation,
    ValidationFailure,
    Transition,
    Snapshot,
    ResourceDelta,
    DiagnosticDump,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationTag {
    ContractViolation = 1,
    ValidationFailure = 2,
    Transition = 3,
    Snapshot = 4,
    ResourceDelta = 5,
    DiagnosticDump = 6,
    ForwardProgressStall = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceClass {
    None,
    Cpu,
    Memory,
    AddressSpace,
    Process,
    Scheduler,
    Syscall,
    Interrupt,
    Driver,
    Resource,
    Identity,
    Security,
    Configuration,
    Capability,
    Power,
    Vfs,
    FileDescriptor,
    Signal,
    Device,
    Namespace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationOutcome {
    Success,
    Blocked,
    Retried,
    Denied,
    Faulted,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceOwner {
    None,
    Cpu(usize),
    Pid(u32),
    AddressSpace(u64),
    Device(&'static str),
    Inode(u64),
    Driver(&'static str),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRecord {
    pub event: ObservableEvent,
    pub contract: ContractId,
    pub tag: ObservationTag,
    pub reason: &'static str,
    pub outcome: ObservationOutcome,
    pub resource: ResourceClass,
    pub owner: ResourceOwner,
    pub cpu: Option<usize>,
    pub pid: Option<u32>,
    pub correlation_id: u64,
    pub evidence: [u64; 4],
}

pub struct ObservabilityContract;

static EMITTED_EVENTS: AtomicU64 = AtomicU64::new(0);
static INVALID_EVENTS: AtomicU64 = AtomicU64::new(0);

impl ObservabilityContract {
    pub fn emitted_event_count() -> u64 {
        EMITTED_EVENTS.load(Ordering::Relaxed)
    }

    pub fn invalid_event_count() -> u64 {
        INVALID_EVENTS.load(Ordering::Relaxed)
    }

    pub fn current_correlation_id() -> u64 {
        let cpu = crate::process::table::cpu_idx() as u64;
        let pid = crate::process::table::TABLE
            .try_lock()
            .map(|table| table.current_pid())
            .unwrap_or(0) as u64;
        ((cpu + 1) << 32) | pid
    }

    pub fn current_pid_owner() -> ResourceOwner {
        let pid = crate::process::table::TABLE
            .try_lock()
            .map(|table| table.current_pid())
            .unwrap_or(0);
        if pid != 0 {
            ResourceOwner::Pid(pid)
        } else {
            ResourceOwner::Unknown
        }
    }

    fn kds_subsystem(contract: ContractId) -> crate::kds::KdsSubsystem {
        match contract {
            ContractId::AddressSpace | ContractId::Memory => crate::kds::KdsSubsystem::Memory,
            ContractId::Capability | ContractId::Resource => crate::kds::KdsSubsystem::Kernel,
            ContractId::Configuration => crate::kds::KdsSubsystem::Kernel,
            ContractId::Driver => crate::kds::KdsSubsystem::Driver,
            ContractId::Execution => crate::kds::KdsSubsystem::Scheduler,
            ContractId::Identity => crate::kds::KdsSubsystem::Process,
            ContractId::Interrupt => crate::kds::KdsSubsystem::Interrupt,
            ContractId::Kds | ContractId::Observability => crate::kds::KdsSubsystem::Kernel,
            ContractId::Network => crate::kds::KdsSubsystem::Network,
            ContractId::Power => crate::kds::KdsSubsystem::Driver,
            ContractId::Process => crate::kds::KdsSubsystem::Process,
            ContractId::Scheduler => crate::kds::KdsSubsystem::Scheduler,
            ContractId::Security => crate::kds::KdsSubsystem::Security,
            ContractId::Syscall => crate::kds::KdsSubsystem::Syscall,
            ContractId::Vfs => crate::kds::KdsSubsystem::Vfs,
            ContractId::Watchdog => crate::kds::KdsSubsystem::Watchdog,
            ContractId::Debug | ContractId::Sairu => crate::kds::KdsSubsystem::Kernel,
        }
    }

    fn kds_event_type(record: &EventRecord) -> crate::kds::KdsEventType {
        match record.tag {
            ObservationTag::ForwardProgressStall => match record.contract {
                ContractId::Watchdog => crate::kds::KdsEventType::WatchdogCpuStall,
                _ => crate::kds::KdsEventType::SchedulerStall,
            },
            _ => match record.event {
                ObservableEvent::ContractViolation | ObservableEvent::ValidationFailure => {
                    crate::kds::KdsEventType::CompatibilityFailure
                }
                ObservableEvent::Transition => crate::kds::KdsEventType::State,
                ObservableEvent::Snapshot => crate::kds::KdsEventType::State,
                ObservableEvent::ResourceDelta => crate::kds::KdsEventType::Metric,
                ObservableEvent::DiagnosticDump => crate::kds::KdsEventType::State,
            },
        }
    }

    fn kds_severity(outcome: ObservationOutcome) -> crate::kds::KdsSeverity {
        match outcome {
            ObservationOutcome::Success => crate::kds::KdsSeverity::Info,
            ObservationOutcome::Blocked | ObservationOutcome::Retried => {
                crate::kds::KdsSeverity::Warn
            }
            ObservationOutcome::Denied
            | ObservationOutcome::Faulted
            | ObservationOutcome::Degraded
            | ObservationOutcome::Failed => crate::kds::KdsSeverity::Error,
        }
    }

    fn owner_word(owner: ResourceOwner) -> u64 {
        match owner {
            ResourceOwner::None => 0,
            ResourceOwner::Cpu(cpu) => (1u64 << 56) | cpu as u64,
            ResourceOwner::Pid(pid) => (2u64 << 56) | pid as u64,
            ResourceOwner::AddressSpace(handle) => (3u64 << 56) | handle,
            ResourceOwner::Device(name) => (4u64 << 56) | Self::stable_name_hash(name),
            ResourceOwner::Inode(inode) => (5u64 << 56) | inode,
            ResourceOwner::Driver(name) => (6u64 << 56) | Self::stable_name_hash(name),
            ResourceOwner::Unknown => u64::MAX,
        }
    }

    fn stable_name_hash(name: &'static str) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in name.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash & 0x00ff_ffff_ffff_ffff
    }

    fn kds_event_shape(record: &EventRecord) -> crate::kds::KdsEventShape {
        crate::kds::KdsEventShape {
            contract: record.contract as u16,
            tag: record.tag as u16,
            outcome: record.outcome as u16,
            resource: record.resource as u16,
            owner: Self::owner_word(record.owner),
            correlation_id: record.correlation_id,
            reason_hash: Self::stable_name_hash(record.reason),
        }
    }

    fn append_kds_event_as(
        record: &EventRecord,
        event_type: crate::kds::KdsEventType,
        severity: crate::kds::KdsSeverity,
    ) -> u64 {
        let pid = record.pid.unwrap_or(0);
        let payload = [
            ((record.contract as u64) << 48)
                | ((record.tag as u64) << 32)
                | ((record.outcome as u64) << 16)
                | record.resource as u64,
            record.correlation_id,
            Self::owner_word(record.owner),
            record.evidence[0],
        ];
        crate::kds::kds_event_record_for(
            Self::kds_subsystem(record.contract),
            event_type,
            severity,
            pid,
            0,
            Self::kds_event_shape(record),
            payload,
        )
    }

    fn append_kds_event(record: &EventRecord) -> u64 {
        Self::append_kds_event_as(
            record,
            Self::kds_event_type(record),
            Self::kds_severity(record.outcome),
        )
    }

    pub fn validate_event(record: &EventRecord) -> Result<(), &'static str> {
        if matches!(
            record.outcome,
            ObservationOutcome::Denied
                | ObservationOutcome::Faulted
                | ObservationOutcome::Degraded
                | ObservationOutcome::Failed
        ) && record.reason.is_empty()
        {
            return Err("observability: failure event has no reason");
        }
        if let Some(cpu) = record.cpu
            && cpu >= MAX_CPUS
        {
            return Err("observability: CPU attribution is out of range");
        }
        if let Some(0) = record.pid {
            return Err("observability: PID attribution uses empty pid");
        }
        match record.owner {
            ResourceOwner::Cpu(cpu) if cpu >= MAX_CPUS => {
                return Err("observability: resource owner CPU is out of range");
            }
            ResourceOwner::Pid(0) => return Err("observability: resource owner PID is empty"),
            ResourceOwner::AddressSpace(0) => {
                return Err("observability: address-space owner is empty");
            }
            ResourceOwner::Device(name) | ResourceOwner::Driver(name) if name.is_empty() => {
                return Err("observability: named resource owner is empty");
            }
            _ => {}
        }
        if record.correlation_id == 0
            && !matches!(
                record.outcome,
                ObservationOutcome::Success | ObservationOutcome::Blocked
            )
        {
            return Err("observability: non-success event has no correlation id");
        }
        Ok(())
    }

    pub fn validate_event_or_panic(record: &EventRecord) {
        if let Err(reason) = Self::validate_event(record) {
            INVALID_EVENTS.fetch_add(1, Ordering::Relaxed);
            crate::serial_println!(
                "[observability-contract] invalid event tag={:?} contract={:?} event={:?} reason={}",
                record.tag,
                record.contract,
                record.event,
                reason
            );
            panic!("[observability-contract] invalid event: {}", reason);
        }
    }

    pub fn emit(record: EventRecord) {
        Self::validate_event_or_panic(&record);
        Self::append_kds_event(&record);
        EMITTED_EVENTS.fetch_add(1, Ordering::Relaxed);
        Self::print_failed_event(&record);
    }

    pub fn emit_as_kds_event(
        record: EventRecord,
        event_type: crate::kds::KdsEventType,
        severity: crate::kds::KdsSeverity,
    ) {
        Self::validate_event_or_panic(&record);
        Self::append_kds_event_as(&record, event_type, severity);
        EMITTED_EVENTS.fetch_add(1, Ordering::Relaxed);
        Self::print_failed_event(&record);
    }

    pub fn kds_event(
        subsystem: crate::kds::KdsSubsystem,
        event_type: crate::kds::KdsEventType,
        severity: crate::kds::KdsSeverity,
        payload: [u64; 4],
    ) -> u64 {
        crate::kds::kds_event(subsystem, event_type, severity, payload)
    }

    pub fn kds_event_tier(
        tier: crate::kds::TelemetryTier,
        subsystem: crate::kds::KdsSubsystem,
        event_type: crate::kds::KdsEventType,
        severity: crate::kds::KdsSeverity,
        payload: [u64; 4],
    ) -> Option<u64> {
        crate::kds::kds_event_tier(tier, subsystem, event_type, severity, payload)
    }

    pub fn set_telemetry_tier(tier: crate::kds::TelemetryTier, enabled: bool) {
        crate::kds::set_telemetry_tier(tier, enabled);
    }

    pub fn telemetry_tier_enabled(tier: crate::kds::TelemetryTier) -> bool {
        crate::kds::tier_enabled(tier)
    }

    pub fn kds_event_for(
        subsystem: crate::kds::KdsSubsystem,
        event_type: crate::kds::KdsEventType,
        severity: crate::kds::KdsSeverity,
        pid: u32,
        tid: u32,
        payload: [u64; 4],
    ) -> u64 {
        crate::kds::kds_event_for(subsystem, event_type, severity, pid, tid, payload)
    }

    pub fn kds_metric(metric_id: crate::kds::KdsMetricId, value: u64, payload: [u64; 2]) {
        crate::kds::kds_metric(metric_id, value, payload);
    }

    pub fn kds_metric_for(
        subsystem: crate::kds::KdsSubsystem,
        metric_id: crate::kds::KdsMetricId,
        value: u64,
        pid: u32,
        tid: u32,
        payload: [u64; 2],
    ) {
        crate::kds::kds_metric_for(subsystem, metric_id, value, pid, tid, payload);
    }

    pub fn kds_state(
        subsystem: crate::kds::KdsSubsystem,
        state_id: u64,
        value: u64,
        severity: crate::kds::KdsSeverity,
        payload: [u64; 2],
    ) {
        crate::kds::kds_state(subsystem, state_id, value, severity, payload);
    }

    pub fn kds_state_for(
        subsystem: crate::kds::KdsSubsystem,
        state_id: u64,
        value: u64,
        severity: crate::kds::KdsSeverity,
        pid: u32,
        tid: u32,
        payload: [u64; 2],
    ) {
        crate::kds::kds_state_for(subsystem, state_id, value, severity, pid, tid, payload);
    }

    pub fn kds_object(
        object_kind: crate::kds::KdsObjectKind,
        parent_object_id: u64,
        payload: [u64; 2],
    ) -> u64 {
        crate::kds::kds_object(object_kind, parent_object_id, payload)
    }

    pub fn obs_counter(
        subsystem: crate::kds::KdsSubsystem,
        metric_id: crate::kds::KdsMetricId,
        delta: u64,
    ) {
        crate::kds::obs_counter(subsystem, metric_id, delta);
    }

    pub fn obs_gauge(
        subsystem: crate::kds::KdsSubsystem,
        metric_id: crate::kds::KdsMetricId,
        value: u64,
    ) {
        crate::kds::obs_gauge(subsystem, metric_id, value);
    }

    pub fn obs_histogram(
        subsystem: crate::kds::KdsSubsystem,
        metric_id: crate::kds::KdsMetricId,
        sample: u64,
    ) {
        crate::kds::obs_histogram(subsystem, metric_id, sample);
    }

    fn print_failed_event(record: &EventRecord) {
        if matches!(
            record.outcome,
            ObservationOutcome::Denied
                | ObservationOutcome::Faulted
                | ObservationOutcome::Degraded
                | ObservationOutcome::Failed
        ) {
            crate::serial_println!(
                "[obs] event={:?} contract={:?} tag={:?} outcome={:?} resource={:?} owner={:?} cpu={:?} pid={:?} cid={:#x} reason={} evidence={:#x},{:#x},{:#x},{:#x}",
                record.event,
                record.contract,
                record.tag,
                record.outcome,
                record.resource,
                record.owner,
                record.cpu,
                record.pid,
                record.correlation_id,
                record.reason,
                record.evidence[0],
                record.evidence[1],
                record.evidence[2],
                record.evidence[3]
            );
        }
    }

    pub fn validate_storage_independent_kds() -> Result<(), &'static str> {
        let stats = crate::kds::stats();
        if stats.events.capacity == 0
            || stats.metrics.capacity == 0
            || stats.traces.capacity == 0
            || stats.objects.capacity == 0
            || stats.state.capacity == 0
        {
            return Err("observability: KDS memory stream capacity missing");
        }
        if !stats.sealed || stats.reserved_base == 0 || stats.reserved_size == 0 {
            return Err("observability: KDS reserved region is not sealed");
        }
        if stats.cpu_rings == 0 || stats.events.record_size != 256 {
            return Err("observability: KDS per-CPU event rings are not constitutional");
        }
        if stats.events.storage_provider != stats.metrics.storage_provider
            || stats.events.storage_provider != stats.traces.storage_provider
            || stats.events.storage_provider != stats.objects.storage_provider
            || stats.events.storage_provider != stats.state.storage_provider
        {
            return Err("observability: KDS provider selection is inconsistent");
        }
        Self::validate_numa_kds_segment_evidence()?;
        Self::validate_flight_recorder_persistence_evidence()?;
        Ok(())
    }

    pub fn validate_flight_recorder_persistence_evidence() -> Result<(), &'static str> {
        let stats = crate::kds::stats();
        if stats.flight_recorder_critical_failures > 0 {
            return Err("observability: Flight Recorder critical persistence failures recorded");
        }
        if stats
            .flight_recorder_final_seals
            .saturating_add(stats.flight_recorder_final_seal_failures)
            > stats.flight_recorder_final_seal_attempts
        {
            return Err("observability: Flight Recorder final seal accounting is inconsistent");
        }
        if stats.flight_recorder_final_seals > stats.flight_recorder_final_seal_attempts {
            return Err("observability: Flight Recorder final seals exceed attempts");
        }
        Ok(())
    }

    pub fn validate_numa_kds_segment_evidence() -> Result<(), &'static str> {
        let evidence = crate::numa_contract::NumaContract::kds_segment_evidence();
        if evidence.node_count == 0 {
            return Err("observability: NUMA topology has no nodes");
        }
        if evidence.assignment_count == 0 {
            return Err("observability: NUMA_KDS_SEGMENT evidence is missing");
        }
        if !evidence.all_online_nodes_assigned {
            return Err("observability: NUMA_KDS_SEGMENT evidence does not cover online nodes");
        }
        Ok(())
    }

    pub fn validate_flight_recorder_node_assignments() -> Result<(), &'static str> {
        let evidence = crate::numa_contract::NumaContract::kds_segment_evidence();
        if evidence.durable_flight_recorder_assignments != evidence.node_count {
            return Err("observability: durable FR_NODE_ASSIGNMENT evidence is incomplete");
        }
        Ok(())
    }

    pub fn contract_violation(
        contract: ContractId,
        tag: &'static str,
        reason: &'static str,
        resource: ResourceClass,
        owner: ResourceOwner,
        evidence: [u64; 4],
    ) {
        let cpu = crate::process::table::cpu_idx();
        let pid = crate::process::current_pid();
        let record = EventRecord {
            event: ObservableEvent::ContractViolation,
            contract,
            tag: ObservationTag::ContractViolation,
            reason,
            outcome: ObservationOutcome::Failed,
            resource,
            owner,
            cpu: Some(cpu),
            pid,
            correlation_id: Self::current_correlation_id(),
            evidence,
        };
        Self::validate_event_or_panic(&record);
        let evidence_event_id = Self::append_kds_event_as(
            &record,
            crate::kds::KdsEventType::ContractViolation,
            crate::kds::KdsSeverity::Fatal,
        );
        EMITTED_EVENTS.fetch_add(1, Ordering::Relaxed);
        Self::print_failed_event(&record);
        if !crate::reliability_contract::ReliabilityContract::active() {
            crate::reliability_contract::ReliabilityContract::enter_red_ring(
                crate::reliability_contract::RedRingEvidence {
                    cause: crate::reliability_contract::RedRingCause::ContractViolation,
                    evidence_event_id,
                    invariant_id: Self::stable_name_hash(tag),
                    detail: ((contract as u64) << 48) | Self::stable_name_hash(reason),
                },
            );
        }
    }
}
