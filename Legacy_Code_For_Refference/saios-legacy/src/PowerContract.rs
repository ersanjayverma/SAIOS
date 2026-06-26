//! Power contract owner surface.

use crate::capability_contract::Capability;
use crate::observability_contract::{
    ContractId, EventRecord, ObservabilityContract, ObservableEvent, ObservationOutcome,
    ObservationTag, ResourceClass, ResourceOwner,
};

pub struct PowerContract;

impl PowerContract {
    pub fn reboot() -> ! {
        let _ = crate::capability_contract::CapabilityContract::require(Capability::PowerControl);
        let _ = crate::resource_contract::ResourceContract::charge_current(
            crate::resource_contract::ResourceKind::PowerUnits,
            1,
        );
        Self::emit(
            "power.reboot",
            ObservationOutcome::Success,
            [crate::process::current_pid().unwrap_or(0) as u64, 0, 0, 0],
        );
        let _ = crate::block::sync();
        crate::driver::acpi::reboot();
    }

    pub fn shutdown() -> ! {
        let _ = crate::capability_contract::CapabilityContract::require(Capability::PowerControl);
        let _ = crate::resource_contract::ResourceContract::charge_current(
            crate::resource_contract::ResourceKind::PowerUnits,
            1,
        );
        Self::emit(
            "power.shutdown",
            ObservationOutcome::Success,
            [crate::process::current_pid().unwrap_or(0) as u64, 0, 0, 0],
        );
        let _ = crate::block::sync();
        crate::driver::acpi::shutdown();
    }

    pub fn acpi_ready() -> bool {
        true
    }

    fn emit(reason: &'static str, outcome: ObservationOutcome, evidence: [u64; 4]) {
        ObservabilityContract::emit(EventRecord {
            event: ObservableEvent::Transition,
            contract: ContractId::Power,
            tag: ObservationTag::Transition,
            reason,
            outcome,
            resource: ResourceClass::Power,
            owner: ResourceOwner::Device("acpi"),
            cpu: Some(crate::process::table::cpu_idx()),
            pid: crate::process::current_pid(),
            correlation_id: ObservabilityContract::current_correlation_id(),
            evidence,
        });
    }
}
