//! Central NUMA topology and capability authority.
//!
//! SMP owns CPU bring-up, but NUMA policy must be visible to every subsystem
//! that will eventually make placement decisions.

use crate::process::table::MAX_CPUS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumaNodeView {
    pub node_id: usize,
    pub cpu_mask: u64,
    pub online: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumaTopologyView {
    pub node_count: usize,
    pub scheduler_visible_cpu_mask: u64,
    pub single_node: bool,
    pub slit_available: bool,
    pub memory_ranges_available: bool,
    pub nodes: [NumaNodeView; MAX_NUMA_NODES],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumaCapabilityView {
    pub topology_contract: bool,
    pub scheduler_metadata: bool,
    pub memory_policy_metadata: bool,
    pub slab_allocator_policy: bool,
    pub page_cache_policy: bool,
    pub kds_placement_policy: bool,
    pub interrupt_routing_policy: bool,
    pub flight_recorder_storage_policy: bool,
    pub migration_engine: bool,
    pub locality_metrics: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumaKdsSegmentView {
    pub node_id: usize,
    pub cpu_mask: u64,
    pub segment_base: u64,
    pub segment_size: u64,
    pub ring_count: usize,
    pub slot_size: usize,
    pub storage_provider: crate::kds::KdsStorageProvider,
    pub flight_recorder_node_assignment: bool,
    pub flight_recorder_durable: bool,
    pub evidence_valid: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumaKdsEvidenceView {
    pub node_count: usize,
    pub assignment_count: usize,
    pub all_online_nodes_assigned: bool,
    pub durable_flight_recorder_assignments: usize,
    pub segments: [NumaKdsSegmentView; MAX_NUMA_NODES],
}

pub struct NumaContract;

pub const MAX_NUMA_NODES: usize = 8;

const EMPTY_KDS_SEGMENT_VIEW: NumaKdsSegmentView = NumaKdsSegmentView {
    node_id: 0,
    cpu_mask: 0,
    segment_base: 0,
    segment_size: 0,
    ring_count: 0,
    slot_size: 0,
    storage_provider: crate::kds::KdsStorageProvider::MemoryOnly,
    flight_recorder_node_assignment: false,
    flight_recorder_durable: false,
    evidence_valid: false,
};

impl NumaContract {
    pub fn topology_view() -> NumaTopologyView {
        let scheduler_visible_cpu_mask = crate::smp::scheduler_visible_mask();
        let mut nodes = [NumaNodeView {
            node_id: 0,
            cpu_mask: 0,
            online: false,
        }; MAX_NUMA_NODES];

        for cpu in 0..MAX_CPUS {
            if scheduler_visible_cpu_mask & (1u64 << cpu) == 0 {
                continue;
            }
            let node_id = crate::smp::numa_node_for_cpu(cpu)
                .unwrap_or(0)
                .min(MAX_NUMA_NODES - 1);
            nodes[node_id].node_id = node_id;
            nodes[node_id].cpu_mask |= 1u64 << cpu;
            nodes[node_id].online = true;
        }

        if nodes[0].cpu_mask == 0 {
            nodes[0].cpu_mask = scheduler_visible_cpu_mask.max(1);
            nodes[0].online = true;
        }

        let node_count = nodes.iter().filter(|node| node.online).count().max(1);
        NumaTopologyView {
            node_count,
            scheduler_visible_cpu_mask,
            single_node: node_count == 1,
            slit_available: false,
            memory_ranges_available: false,
            nodes,
        }
    }

    pub fn capability_view() -> NumaCapabilityView {
        let topology = Self::topology_view();
        let kds = crate::kds::stats();
        let evidence = Self::kds_segment_evidence();
        let kds_ready = kds.sealed
            && kds.reserved_base != 0
            && kds.reserved_size != 0
            && kds.cpu_rings > 0
            && kds.events.record_size == 256
            && kds.events.capacity > 0;
        let durable_flight_recorder = !matches!(
            kds.events.storage_provider,
            crate::kds::KdsStorageProvider::MemoryOnly
        );

        NumaCapabilityView {
            topology_contract: true,
            scheduler_metadata: true,
            memory_policy_metadata: true,
            slab_allocator_policy: false,
            page_cache_policy: false,
            kds_placement_policy: kds_ready
                && topology.single_node
                && evidence.all_online_nodes_assigned,
            interrupt_routing_policy: false,
            flight_recorder_storage_policy: kds_ready
                && topology.single_node
                && durable_flight_recorder
                && evidence.durable_flight_recorder_assignments == evidence.node_count,
            migration_engine: false,
            locality_metrics: false,
        }
    }

    pub fn kds_segment_evidence() -> NumaKdsEvidenceView {
        let topology = Self::topology_view();
        let stats = crate::kds::stats();
        let provider = stats.events.storage_provider;
        let durable_provider = !matches!(provider, crate::kds::KdsStorageProvider::MemoryOnly);
        let mut segments = [EMPTY_KDS_SEGMENT_VIEW; MAX_NUMA_NODES];
        let mut assignment_count = 0usize;
        let mut durable_flight_recorder_assignments = 0usize;

        for node in topology.nodes.iter().filter(|node| node.online) {
            let mut segment_base = u64::MAX;
            let mut segment_end = 0u64;
            let mut ring_count = 0usize;
            for cpu in 0..MAX_CPUS {
                if node.cpu_mask & (1u64 << cpu) == 0 {
                    continue;
                }
                if let Some(ring) = crate::kds::ring_assignment(cpu) {
                    segment_base = segment_base.min(ring.base);
                    segment_end = segment_end.max(ring.base.saturating_add(ring.size));
                    ring_count = ring_count.saturating_add(1);
                }
            }

            let evidence_valid = stats.sealed && ring_count > 0 && segment_base != u64::MAX;
            let flight_recorder_node_assignment = evidence_valid;
            let flight_recorder_durable = flight_recorder_node_assignment && durable_provider;
            if evidence_valid {
                assignment_count = assignment_count.saturating_add(1);
            }
            if flight_recorder_durable {
                durable_flight_recorder_assignments =
                    durable_flight_recorder_assignments.saturating_add(1);
            }

            segments[node.node_id] = NumaKdsSegmentView {
                node_id: node.node_id,
                cpu_mask: node.cpu_mask,
                segment_base: if evidence_valid { segment_base } else { 0 },
                segment_size: if evidence_valid {
                    segment_end.saturating_sub(segment_base)
                } else {
                    0
                },
                ring_count,
                slot_size: crate::kds::slot_size(),
                storage_provider: provider,
                flight_recorder_node_assignment,
                flight_recorder_durable,
                evidence_valid,
            };
        }

        NumaKdsEvidenceView {
            node_count: topology.node_count,
            assignment_count,
            all_online_nodes_assigned: assignment_count == topology.node_count,
            durable_flight_recorder_assignments,
            segments,
        }
    }

    pub fn emit_kds_segment_evidence() -> usize {
        let evidence = Self::kds_segment_evidence();
        let mut emitted = 0usize;
        for segment in evidence
            .segments
            .iter()
            .filter(|segment| segment.evidence_valid)
        {
            crate::kds::kds_event(
                crate::kds::KdsSubsystem::Smp,
                crate::kds::KdsEventType::NumaKdsSegment,
                crate::kds::KdsSeverity::Info,
                [
                    segment.node_id as u64,
                    segment.segment_base,
                    segment.segment_size,
                    segment.cpu_mask,
                ],
            );
            crate::kds::kds_event(
                crate::kds::KdsSubsystem::Smp,
                crate::kds::KdsEventType::FrNodeAssignment,
                if segment.flight_recorder_durable {
                    crate::kds::KdsSeverity::Info
                } else {
                    crate::kds::KdsSeverity::Warn
                },
                [
                    segment.node_id as u64,
                    segment.segment_base,
                    segment.storage_provider as u64,
                    segment.flight_recorder_durable as u64,
                ],
            );
            emitted = emitted.saturating_add(2);
        }
        emitted
    }
}
