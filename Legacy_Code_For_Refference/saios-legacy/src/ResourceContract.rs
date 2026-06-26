//! Resource contract owner surface.

use spin::Mutex;

use crate::observability_contract::{
    ContractId, EventRecord, ObservabilityContract, ObservableEvent, ObservationOutcome,
    ObservationTag, ResourceClass, ResourceOwner,
};

const RESOURCE_KIND_COUNT: usize = 10;
const RESOURCE_BUCKET_CAPACITY: usize = 128;
const UNLIMITED_QUOTA: u64 = u64::MAX;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountableEntityKind {
    Kernel = 1,
    Process = 2,
    User = 3,
    Service = 4,
    Unattributed = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountableEntity {
    pub kind: AccountableEntityKind,
    pub id: u64,
}

impl AccountableEntity {
    pub const KERNEL: Self = Self {
        kind: AccountableEntityKind::Kernel,
        id: 0,
    };

    pub const UNATTRIBUTED: Self = Self {
        kind: AccountableEntityKind::Unattributed,
        id: 0,
    };

    pub const fn process(pid: u32) -> Self {
        Self {
            kind: AccountableEntityKind::Process,
            id: pid as u64,
        }
    }

    fn word(self) -> u64 {
        ((self.kind as u64) << 56) | (self.id & 0x00ff_ffff_ffff_ffff)
    }

    fn owner(self) -> ResourceOwner {
        match self.kind {
            AccountableEntityKind::Process => ResourceOwner::Pid(self.id as u32),
            AccountableEntityKind::Kernel => ResourceOwner::None,
            AccountableEntityKind::User
            | AccountableEntityKind::Service
            | AccountableEntityKind::Unattributed => ResourceOwner::Unknown,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    MemoryPages = 0,
    VirtualMappings = 1,
    NetworkBytes = 2,
    NetworkPackets = 3,
    StorageBytes = 4,
    IpcObjects = 5,
    IpcBytes = 6,
    DriverResources = 7,
    CpuTimeNs = 8,
    PowerUnits = 9,
}

impl ResourceKind {
    const fn index(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::MemoryPages => "memory_pages",
            Self::VirtualMappings => "virtual_mappings",
            Self::NetworkBytes => "network_bytes",
            Self::NetworkPackets => "network_packets",
            Self::StorageBytes => "storage_bytes",
            Self::IpcObjects => "ipc_objects",
            Self::IpcBytes => "ipc_bytes",
            Self::DriverResources => "driver_resources",
            Self::CpuTimeNs => "cpu_time_ns",
            Self::PowerUnits => "power_units",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceCoverageDescriptor {
    pub kind: ResourceKind,
    pub owner_contract: &'static str,
    pub charge_path: &'static str,
    pub release_path: &'static str,
    pub fallback_path: &'static str,
    pub evidence_event: crate::kds::KdsEventType,
    pub implemented: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceCoverageReport {
    pub resource_kinds: usize,
    pub descriptors: usize,
    pub implemented: usize,
    pub missing: usize,
    pub fallback_paths: usize,
    pub all_kinds_described: bool,
    pub all_kinds_implemented: bool,
    pub accounting_invariants: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributionChain {
    pub accountable: AccountableEntity,
    pub acting_pid: Option<u32>,
    pub correlation_id: u64,
    pub evidence_event_id: u64,
}

impl AttributionChain {
    pub fn current() -> Self {
        let accountable = current_accountable_entity();
        Self {
            accountable,
            acting_pid: crate::process::current_pid(),
            correlation_id: ObservabilityContract::current_correlation_id(),
            evidence_event_id: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResourceBucket {
    entity: AccountableEntity,
    kind: ResourceKind,
    used: u64,
    quota: u64,
    active: bool,
}

impl ResourceBucket {
    const EMPTY: Self = Self {
        entity: AccountableEntity::UNATTRIBUTED,
        kind: ResourceKind::MemoryPages,
        used: 0,
        quota: UNLIMITED_QUOTA,
        active: false,
    };
}

struct AccountingState {
    buckets: [ResourceBucket; RESOURCE_BUCKET_CAPACITY],
    totals: [u64; RESOURCE_KIND_COUNT],
    unattributed: [u64; RESOURCE_KIND_COUNT],
    active_buckets: usize,
    quota_exceeded: u64,
    quota_changes: u64,
    attribution_failures: u64,
    invariant_violations: u64,
}

impl AccountingState {
    const fn new() -> Self {
        Self {
            buckets: [ResourceBucket::EMPTY; RESOURCE_BUCKET_CAPACITY],
            totals: [0; RESOURCE_KIND_COUNT],
            unattributed: [0; RESOURCE_KIND_COUNT],
            active_buckets: 0,
            quota_exceeded: 0,
            quota_changes: 0,
            attribution_failures: 0,
            invariant_violations: 0,
        }
    }

    fn bucket_index(&mut self, entity: AccountableEntity, kind: ResourceKind) -> Option<usize> {
        for idx in 0..RESOURCE_BUCKET_CAPACITY {
            let bucket = self.buckets[idx];
            if bucket.active && bucket.entity == entity && bucket.kind == kind {
                return Some(idx);
            }
        }
        for idx in 0..RESOURCE_BUCKET_CAPACITY {
            if !self.buckets[idx].active {
                self.buckets[idx] = ResourceBucket {
                    entity,
                    kind,
                    used: 0,
                    quota: UNLIMITED_QUOTA,
                    active: true,
                };
                self.active_buckets = self.active_buckets.saturating_add(1);
                return Some(idx);
            }
        }
        None
    }

    fn used_sum(&self, kind: ResourceKind) -> u64 {
        let mut sum = 0u64;
        for bucket in self
            .buckets
            .iter()
            .filter(|bucket| bucket.active && bucket.kind == kind)
        {
            sum = sum.saturating_add(bucket.used);
        }
        sum
    }
}

static ACCOUNTING: Mutex<AccountingState> = Mutex::new(AccountingState::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub cpu_count: usize,
    pub current_pid: Option<u32>,
    pub page_alloc_metrics: u64,
    pub page_free_metrics: u64,
    pub network_tx_depth: usize,
    pub network_rx_depth: usize,
    pub accounted: [u64; RESOURCE_KIND_COUNT],
    pub unattributed: [u64; RESOURCE_KIND_COUNT],
    pub active_entities: usize,
    pub quota_exceeded: u64,
    pub quota_changes: u64,
    pub attribution_failures: u64,
    pub invariant_violations: u64,
}

pub struct ResourceContract;

pub const RESOURCE_COVERAGE_DESCRIPTORS: [ResourceCoverageDescriptor; RESOURCE_KIND_COUNT] = [
    ResourceCoverageDescriptor {
        kind: ResourceKind::MemoryPages,
        owner_contract: "MemoryContract",
        charge_path: "MemoryContract::alloc_page/alloc_contiguous_pages",
        release_path: "MemoryContract::free_page/release_shared_or_free",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::ResourceAccountPeriod,
        implemented: true,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::VirtualMappings,
        owner_contract: "AddressSpaceContract",
        charge_path: "AddressSpaceContract::map_user_frames_with_flags_in",
        release_path: "AddressSpaceContract::unmap_user_range_in",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::ResourceAccountPeriod,
        implemented: true,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::NetworkBytes,
        owner_contract: "NetworkContract",
        charge_path: "NetworkContract::enqueue_tx/enqueue_rx",
        release_path: "not retained; traffic byte accounting is cumulative",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::ResourceAccountPeriod,
        implemented: true,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::NetworkPackets,
        owner_contract: "NetworkContract",
        charge_path: "NetworkContract::enqueue_tx/enqueue_rx",
        release_path: "not retained; traffic packet accounting is cumulative",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::ResourceAccountPeriod,
        implemented: true,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::StorageBytes,
        owner_contract: "VfsContract/storage platform",
        charge_path: "VFS/storage byte accounting pending",
        release_path: "not retained",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::AccountingAttributionFailure,
        implemented: false,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::IpcObjects,
        owner_contract: "IpcContract",
        charge_path: "IpcContract object allocation accounting pending",
        release_path: "IpcContract object release accounting pending",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::AccountingAttributionFailure,
        implemented: false,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::IpcBytes,
        owner_contract: "IpcContract",
        charge_path: "IpcContract byte transfer accounting pending",
        release_path: "not retained",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::AccountingAttributionFailure,
        implemented: false,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::DriverResources,
        owner_contract: "DriverContract",
        charge_path: "DriverContract resource accounting pending",
        release_path: "DriverContract resource release accounting pending",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::AccountingAttributionFailure,
        implemented: false,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::CpuTimeNs,
        owner_contract: "SchedulerContract",
        charge_path: "process::scheduler::account_cpu_time",
        release_path: "not retained",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::ResourceAccountPeriod,
        implemented: true,
    },
    ResourceCoverageDescriptor {
        kind: ResourceKind::PowerUnits,
        owner_contract: "PowerContract",
        charge_path: "PowerContract::reboot/shutdown",
        release_path: "not retained",
        fallback_path: "ResourceContract::charge_unattributed",
        evidence_event: crate::kds::KdsEventType::ResourceAccountPeriod,
        implemented: true,
    },
];

impl ResourceContract {
    pub fn coverage_descriptors() -> &'static [ResourceCoverageDescriptor; RESOURCE_KIND_COUNT] {
        &RESOURCE_COVERAGE_DESCRIPTORS
    }

    pub fn coverage_report() -> ResourceCoverageReport {
        let mut implemented = 0usize;
        let mut fallback_paths = 0usize;
        let mut described = [false; RESOURCE_KIND_COUNT];
        for descriptor in &RESOURCE_COVERAGE_DESCRIPTORS {
            described[descriptor.kind.index()] = true;
            if descriptor.implemented {
                implemented = implemented.saturating_add(1);
            }
            if !descriptor.fallback_path.is_empty() {
                fallback_paths = fallback_paths.saturating_add(1);
            }
        }
        let all_kinds_described = described.iter().all(|present| *present)
            && RESOURCE_COVERAGE_DESCRIPTORS.len() == RESOURCE_KIND_COUNT;
        let accounting_invariants = Self::validate_accounting_invariants().is_ok();
        ResourceCoverageReport {
            resource_kinds: RESOURCE_KIND_COUNT,
            descriptors: RESOURCE_COVERAGE_DESCRIPTORS.len(),
            implemented,
            missing: RESOURCE_KIND_COUNT.saturating_sub(implemented),
            fallback_paths,
            all_kinds_described,
            all_kinds_implemented: implemented == RESOURCE_KIND_COUNT,
            accounting_invariants,
        }
    }

    pub fn validate_accounting_coverage() -> Result<ResourceCoverageReport, &'static str> {
        let report = Self::coverage_report();
        if !report.all_kinds_described {
            return Err("resource: accounting coverage descriptor gap");
        }
        if !report.accounting_invariants {
            return Err("resource: accounting invariant validation failed");
        }
        Ok(report)
    }

    pub fn snapshot() -> ResourceSnapshot {
        let memory = crate::memory_contract::MemoryContract::diagnostic_view();
        let network = crate::network_contract::NetworkContract::status_view();
        let accounting = ACCOUNTING.lock();
        let snapshot = ResourceSnapshot {
            cpu_count: crate::process::table::MAX_CPUS,
            current_pid: crate::process::current_pid(),
            page_alloc_metrics: memory.page_alloc_metrics,
            page_free_metrics: memory.page_free_metrics,
            network_tx_depth: network.tx_depth,
            network_rx_depth: network.rx_depth,
            accounted: accounting.totals,
            unattributed: accounting.unattributed,
            active_entities: accounting.active_buckets,
            quota_exceeded: accounting.quota_exceeded,
            quota_changes: accounting.quota_changes,
            attribution_failures: accounting.attribution_failures,
            invariant_violations: accounting.invariant_violations,
        };
        drop(accounting);
        Self::emit(
            "resource.snapshot",
            [
                snapshot.cpu_count as u64,
                snapshot.current_pid.unwrap_or(0) as u64,
                snapshot.accounted[ResourceKind::MemoryPages.index()],
                snapshot
                    .unattributed
                    .iter()
                    .fold(0u64, |sum, value| sum.saturating_add(*value)),
            ],
        );
        snapshot
    }

    pub fn set_quota(
        entity: AccountableEntity,
        kind: ResourceKind,
        quota: u64,
    ) -> Result<(), &'static str> {
        let used = {
            let mut accounting = ACCOUNTING.lock();
            let Some(idx) = accounting.bucket_index(entity, kind) else {
                accounting.invariant_violations = accounting.invariant_violations.saturating_add(1);
                return Err("resource: accounting bucket capacity exhausted");
            };
            accounting.buckets[idx].quota = quota;
            accounting.quota_changes = accounting.quota_changes.saturating_add(1);
            accounting.buckets[idx].used
        };
        Self::emit_accounting_event(
            crate::kds::KdsEventType::QuotaChanged,
            ObservationOutcome::Success,
            "resource.quota.changed",
            entity,
            kind,
            quota,
            used,
        );
        Ok(())
    }

    pub fn charge_current(kind: ResourceKind, amount: u64) -> Result<(), &'static str> {
        Self::charge(AttributionChain::current(), kind, amount)
    }

    pub fn charge(
        chain: AttributionChain,
        kind: ResourceKind,
        amount: u64,
    ) -> Result<(), &'static str> {
        if amount == 0 {
            return Ok(());
        }
        let entity = chain.accountable;
        let total = {
            let mut accounting = ACCOUNTING.lock();
            let Some(idx) = accounting.bucket_index(entity, kind) else {
                accounting.invariant_violations = accounting.invariant_violations.saturating_add(1);
                return Err("resource: accounting bucket capacity exhausted");
            };
            let bucket = &mut accounting.buckets[idx];
            let requested = bucket.used.saturating_add(amount);
            if requested > bucket.quota {
                let used = bucket.used;
                accounting.quota_exceeded = accounting.quota_exceeded.saturating_add(1);
                drop(accounting);
                Self::emit_accounting_event(
                    crate::kds::KdsEventType::ResourceQuotaExceeded,
                    ObservationOutcome::Denied,
                    "resource.quota.exceeded",
                    entity,
                    kind,
                    amount,
                    used,
                );
                return Err("resource: quota exceeded");
            }
            bucket.used = requested;
            accounting.totals[kind.index()] =
                accounting.totals[kind.index()].saturating_add(amount);
            if entity.kind == AccountableEntityKind::Unattributed {
                accounting.unattributed[kind.index()] =
                    accounting.unattributed[kind.index()].saturating_add(amount);
            }
            accounting.totals[kind.index()]
        };
        Self::emit_accounting_event(
            crate::kds::KdsEventType::ResourceAccountPeriod,
            ObservationOutcome::Success,
            "resource.account.commit",
            entity,
            kind,
            amount,
            total,
        );
        Ok(())
    }

    pub fn charge_unattributed(kind: ResourceKind, amount: u64) {
        {
            let mut accounting = ACCOUNTING.lock();
            accounting.attribution_failures = accounting.attribution_failures.saturating_add(1);
        }
        Self::emit_accounting_event(
            crate::kds::KdsEventType::AccountingAttributionFailure,
            ObservationOutcome::Failed,
            "resource.attribution.failure",
            AccountableEntity::UNATTRIBUTED,
            kind,
            amount,
            0,
        );
        let _ = Self::charge(
            AttributionChain {
                accountable: AccountableEntity::UNATTRIBUTED,
                acting_pid: crate::process::current_pid(),
                correlation_id: ObservabilityContract::current_correlation_id(),
                evidence_event_id: 0,
            },
            kind,
            amount,
        );
    }

    pub fn release(entity: AccountableEntity, kind: ResourceKind, amount: u64) {
        if amount == 0 {
            return;
        }
        let released = {
            let mut accounting = ACCOUNTING.lock();
            let Some(idx) = accounting.bucket_index(entity, kind) else {
                accounting.invariant_violations = accounting.invariant_violations.saturating_add(1);
                return;
            };
            let bucket = &mut accounting.buckets[idx];
            let released = amount.min(bucket.used);
            bucket.used -= released;
            accounting.totals[kind.index()] =
                accounting.totals[kind.index()].saturating_sub(released);
            if entity.kind == AccountableEntityKind::Unattributed {
                accounting.unattributed[kind.index()] =
                    accounting.unattributed[kind.index()].saturating_sub(released);
            }
            released
        };
        Self::emit_accounting_event(
            crate::kds::KdsEventType::ResourceAccountPeriod,
            ObservationOutcome::Success,
            "resource.account.release",
            entity,
            kind,
            released,
            0,
        );
    }

    pub fn validate_accounting_invariants() -> Result<(), &'static str> {
        let violation = {
            let mut accounting = ACCOUNTING.lock();
            let mut violation = None;
            for kind_idx in 0..RESOURCE_KIND_COUNT {
                let kind = resource_kind_from_index(kind_idx);
                let sum = accounting.used_sum(kind);
                if sum != accounting.totals[kind_idx] {
                    accounting.invariant_violations =
                        accounting.invariant_violations.saturating_add(1);
                    violation = Some((kind, sum, accounting.totals[kind_idx]));
                    break;
                }
            }
            violation
        };

        if let Some((kind, sum, total)) = violation {
            Self::emit_accounting_event(
                crate::kds::KdsEventType::AccountingInvariantViolated,
                ObservationOutcome::Failed,
                "resource.accounting.invariant",
                AccountableEntity::KERNEL,
                kind,
                sum,
                total,
            );
            return Err("resource: accounting invariant violated");
        }
        Ok(())
    }

    fn emit(reason: &'static str, evidence: [u64; 4]) {
        ObservabilityContract::emit(EventRecord {
            event: ObservableEvent::Snapshot,
            contract: ContractId::Resource,
            tag: ObservationTag::Snapshot,
            reason,
            outcome: ObservationOutcome::Success,
            resource: ResourceClass::Resource,
            owner: ResourceOwner::Unknown,
            cpu: Some(crate::process::table::cpu_idx()),
            pid: crate::process::current_pid(),
            correlation_id: ObservabilityContract::current_correlation_id(),
            evidence,
        });
    }

    fn emit_accounting_event(
        event_type: crate::kds::KdsEventType,
        outcome: ObservationOutcome,
        reason: &'static str,
        entity: AccountableEntity,
        kind: ResourceKind,
        amount: u64,
        total: u64,
    ) {
        ObservabilityContract::emit_as_kds_event(
            EventRecord {
                event: ObservableEvent::ResourceDelta,
                contract: ContractId::Resource,
                tag: ObservationTag::ResourceDelta,
                reason,
                outcome,
                resource: ResourceClass::Resource,
                owner: entity.owner(),
                cpu: Some(crate::process::table::cpu_idx()),
                pid: crate::process::current_pid(),
                correlation_id: ObservabilityContract::current_correlation_id(),
                evidence: [entity.word(), kind as u64, amount, total],
            },
            event_type,
            match outcome {
                ObservationOutcome::Success => crate::kds::KdsSeverity::Info,
                ObservationOutcome::Denied => crate::kds::KdsSeverity::Warn,
                _ => crate::kds::KdsSeverity::Error,
            },
        );
    }
}

fn current_accountable_entity() -> AccountableEntity {
    crate::process::current_pid()
        .map(AccountableEntity::process)
        .unwrap_or(AccountableEntity::KERNEL)
}

const fn resource_kind_from_index(index: usize) -> ResourceKind {
    match index {
        0 => ResourceKind::MemoryPages,
        1 => ResourceKind::VirtualMappings,
        2 => ResourceKind::NetworkBytes,
        3 => ResourceKind::NetworkPackets,
        4 => ResourceKind::StorageBytes,
        5 => ResourceKind::IpcObjects,
        6 => ResourceKind::IpcBytes,
        7 => ResourceKind::DriverResources,
        8 => ResourceKind::CpuTimeNs,
        _ => ResourceKind::PowerUnits,
    }
}
