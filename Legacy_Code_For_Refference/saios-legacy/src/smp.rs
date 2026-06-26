//! Compatibility shim for the x86_64 SMP implementation.

pub use crate::arch::smp::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmpDiagnosticSnapshot {
    pub started_mask: u64,
    pub initialized_mask: u64,
    pub scheduler_visible_mask: u64,
    pub numa_node_count: usize,
    pub numa_single_node: bool,
}

pub fn diagnostic_snapshot() -> SmpDiagnosticSnapshot {
    let numa = crate::numa_contract::NumaContract::topology_view();
    SmpDiagnosticSnapshot {
        started_mask: started_mask(),
        initialized_mask: initialized_mask(),
        scheduler_visible_mask: scheduler_visible_mask(),
        numa_node_count: numa.node_count,
        numa_single_node: numa.single_node,
    }
}
