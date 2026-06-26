//! Identity contract owner surface.

use crate::observability_contract::{
    ContractId, EventRecord, ObservabilityContract, ObservableEvent, ObservationOutcome,
    ObservationTag, ResourceClass, ResourceOwner,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityView {
    pub pid: u32,
    pub parent_pid: u32,
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub session_id: u32,
    pub pgid: u32,
}

pub struct IdentityContract;

impl IdentityContract {
    pub fn current_view() -> Option<IdentityView> {
        let view = crate::process::with_current_process(|proc| IdentityView {
            pid: proc.pid,
            parent_pid: proc.parent_pid,
            uid: proc.uid,
            gid: proc.gid,
            euid: proc.euid,
            egid: proc.egid,
            session_id: proc.session_id,
            pgid: proc.pgid,
        });
        if let Some(view) = view {
            Self::emit(
                "identity.current.snapshot",
                ResourceOwner::Pid(view.pid),
                [
                    view.pid as u64,
                    pack_ids(view.uid, view.gid),
                    pack_ids(view.euid, view.egid),
                    pack_ids(view.session_id, view.pgid),
                ],
            );
        }
        view
    }

    pub fn is_superuser() -> bool {
        Self::current_view()
            .map(|view| view.euid == 0)
            .unwrap_or(false)
    }

    fn emit(reason: &'static str, owner: ResourceOwner, evidence: [u64; 4]) {
        ObservabilityContract::emit(EventRecord {
            event: ObservableEvent::Snapshot,
            contract: ContractId::Identity,
            tag: ObservationTag::Snapshot,
            reason,
            outcome: ObservationOutcome::Success,
            resource: ResourceClass::Identity,
            owner,
            cpu: Some(crate::process::table::cpu_idx()),
            pid: crate::process::current_pid(),
            correlation_id: ObservabilityContract::current_correlation_id(),
            evidence,
        });
    }
}

fn pack_ids(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | b as u64
}
