//! Security contract owner surface.

use crate::observability_contract::{
    ContractId, EventRecord, ObservabilityContract, ObservableEvent, ObservationOutcome,
    ObservationTag, ResourceClass, ResourceOwner,
};

pub struct SecurityContract;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityCapability {
    SysAdmin = 0,
    SysPtrace = 1,
    NetAdmin = 2,
    SysResource = 3,
    SaiosIntelligence = 32,
    SaiosTelemetry = 33,
    SaiosOrchestrate = 34,
    SaiosPolicy = 35,
}

impl SecurityCapability {
    const fn bit(self) -> u128 {
        1u128 << self as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilitySet {
    pub permitted: u128,
    pub inheritable: u128,
    pub effective: u128,
}

impl CapabilitySet {
    pub const EMPTY: Self = Self {
        permitted: 0,
        inheritable: 0,
        effective: 0,
    };

    pub const ROOT: Self = Self {
        permitted: u128::MAX,
        inheritable: u128::MAX,
        effective: u128::MAX,
    };

    pub const fn has_effective(self, capability: SecurityCapability) -> bool {
        self.effective & capability.bit() != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityLabel {
    pub type_id: u16,
    pub sensitivity: u8,
    pub categories: u64,
}

impl SecurityLabel {
    pub const KERNEL: Self = Self {
        type_id: 0,
        sensitivity: u8::MAX,
        categories: u64::MAX,
    };

    pub const USER_DEFAULT: Self = Self {
        type_id: 1,
        sensitivity: 0,
        categories: 0,
    };

    pub const fn current_subject() -> Self {
        Self::KERNEL
    }

    pub const fn public_object() -> Self {
        Self::USER_DEFAULT
    }

    pub const fn dominates(self, object: Self) -> bool {
        let type_compatible = self.type_id == 0 || self.type_id == object.type_id;
        let category_superset = (self.categories & object.categories) == object.categories;
        type_compatible && self.sensitivity >= object.sensitivity && category_superset
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityOperation {
    FileOpen = 1,
    FileWrite = 2,
    ProcessExec = 3,
    CapabilityUse = 4,
    NetworkConnect = 5,
    IpcCreate = 6,
}

impl SecurityContract {
    pub fn authorize_admin_action(action: &'static str) -> Result<(), &'static str> {
        Self::require_capability(SecurityCapability::SysAdmin, action)
    }

    pub fn require_capability(
        capability: SecurityCapability,
        action: &'static str,
    ) -> Result<(), &'static str> {
        let caps = Self::current_capabilities();
        let allowed = caps.has_effective(capability);
        Self::emit(
            if allowed {
                "security.authorize"
            } else {
                "security.deny"
            },
            if allowed {
                ObservationOutcome::Success
            } else {
                ObservationOutcome::Denied
            },
            [
                stable_hash(action),
                allowed as u64,
                crate::process::current_pid().unwrap_or(0) as u64,
                capability as u64,
            ],
        );
        if allowed {
            Ok(())
        } else {
            Self::emit_kds_security_event(
                crate::kds::KdsEventType::SecuritySyscallDenied,
                [
                    crate::process::current_pid().unwrap_or(0) as u64,
                    0,
                    stable_hash(action),
                    capability as u64,
                ],
            );
            Err("security: administrator identity required")
        }
    }

    pub fn current_capabilities() -> CapabilitySet {
        if crate::identity_contract::IdentityContract::is_superuser() {
            CapabilitySet::ROOT
        } else {
            CapabilitySet::EMPTY
        }
    }

    pub fn check_mac(
        operation: SecurityOperation,
        subject: SecurityLabel,
        object: SecurityLabel,
    ) -> Result<(), &'static str> {
        if subject.dominates(object) {
            Self::emit(
                "security.mac.allow",
                ObservationOutcome::Success,
                [
                    operation as u64,
                    subject.type_id as u64,
                    object.type_id as u64,
                    crate::process::current_pid().unwrap_or(0) as u64,
                ],
            );
            Ok(())
        } else {
            Self::emit(
                "security.mac.deny",
                ObservationOutcome::Denied,
                [
                    operation as u64,
                    subject.type_id as u64,
                    object.type_id as u64,
                    crate::process::current_pid().unwrap_or(0) as u64,
                ],
            );
            Self::emit_kds_security_event(
                crate::kds::KdsEventType::SecurityMacDenied,
                [
                    crate::process::current_pid().unwrap_or(0) as u64,
                    operation as u64,
                    object.sensitivity as u64,
                    object.categories,
                ],
            );
            Err("security: mandatory access policy denied operation")
        }
    }

    pub fn deny_namespace_escape(
        namespace_type: u64,
        target: u64,
        action: &'static str,
    ) -> Result<(), &'static str> {
        Self::emit(
            "security.namespace.escape",
            ObservationOutcome::Denied,
            [
                crate::process::current_pid().unwrap_or(0) as u64,
                namespace_type,
                target,
                stable_hash(action),
            ],
        );
        Self::emit_kds_security_event(
            crate::kds::KdsEventType::SecurityNamespaceEscape,
            [
                crate::process::current_pid().unwrap_or(0) as u64,
                namespace_type,
                target,
                stable_hash(action),
            ],
        );
        Err("security: namespace boundary violation")
    }

    pub fn audit(action: &'static str, outcome: ObservationOutcome) {
        Self::emit(
            "security.audit",
            outcome,
            [
                stable_hash(action),
                crate::process::current_pid().unwrap_or(0) as u64,
                0,
                0,
            ],
        );
    }

    fn emit(reason: &'static str, outcome: ObservationOutcome, evidence: [u64; 4]) {
        ObservabilityContract::emit(EventRecord {
            event: ObservableEvent::Transition,
            contract: ContractId::Security,
            tag: ObservationTag::Transition,
            reason,
            outcome,
            resource: ResourceClass::Security,
            owner: ObservabilityContract::current_pid_owner(),
            cpu: Some(crate::process::table::cpu_idx()),
            pid: crate::process::current_pid(),
            correlation_id: ObservabilityContract::current_correlation_id(),
            evidence,
        });
    }

    fn emit_kds_security_event(event_type: crate::kds::KdsEventType, evidence: [u64; 4]) {
        crate::kds::kds_event(
            crate::kds::KdsSubsystem::Security,
            event_type,
            crate::kds::KdsSeverity::Fatal,
            evidence,
        );
    }
}

fn stable_hash(name: &'static str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
