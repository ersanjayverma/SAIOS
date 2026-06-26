//! Capability contract owner surface.

use crate::observability_contract::{
    ContractId, EventRecord, ObservabilityContract, ObservableEvent, ObservationOutcome,
    ObservationTag, ResourceClass,
};

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Admin = 1,
    ConfigureSystem = 2,
    PowerControl = 3,
    NetworkAdmin = 4,
}

pub struct CapabilityContract;

impl CapabilityContract {
    pub fn current_has(capability: Capability) -> bool {
        let allowed = match capability {
            Capability::Admin
            | Capability::ConfigureSystem
            | Capability::PowerControl
            | Capability::NetworkAdmin => {
                crate::identity_contract::IdentityContract::is_superuser()
            }
        };
        Self::emit(
            "capability.check",
            if allowed {
                ObservationOutcome::Success
            } else {
                ObservationOutcome::Denied
            },
            [
                capability as u64,
                allowed as u64,
                crate::process::current_pid().unwrap_or(0) as u64,
                0,
            ],
        );
        allowed
    }

    pub fn require(capability: Capability) -> Result<(), &'static str> {
        if Self::current_has(capability) {
            Ok(())
        } else {
            Err("capability: denied")
        }
    }

    fn emit(reason: &'static str, outcome: ObservationOutcome, evidence: [u64; 4]) {
        ObservabilityContract::emit(EventRecord {
            event: ObservableEvent::ValidationFailure,
            contract: ContractId::Capability,
            tag: ObservationTag::ValidationFailure,
            reason,
            outcome,
            resource: ResourceClass::Capability,
            owner: ObservabilityContract::current_pid_owner(),
            cpu: Some(crate::process::table::cpu_idx()),
            pid: crate::process::current_pid(),
            correlation_id: ObservabilityContract::current_correlation_id(),
            evidence,
        });
    }
}
