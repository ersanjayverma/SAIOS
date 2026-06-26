# SAIOS Glossary and System Taxonomy
**Document ID:** DOC-02_SAIOS_Glossary_and_Taxonomy.txt
**Layer:** Foundation
**Version:** 1.0.0
**Authority:** Reference document subordinate to DOC-01

## SOURCE TRACEABILITY

Primary sources:
- SAIOS_SSOT.txt: GLOSSARY
- SAIOS_SSOT.txt: CROSS-SUBSYSTEM EVENT TAXONOMY
- SAIOS_SSOT_Part2.txt: PART XV, UPDATED GLOSSARY ADDITIONS
- SAIOS_SSOT_Part2.txt: SECURITY MODEL, capability definitions

No document below this one may define a term that contradicts this glossary. If a lower document needs a new term, the term must be added here or marked for glossary review.

## TAXONOMY LAW

Event names are globally unique in the SAIOS event namespace. No event name may be reused across contracts. Every meaningful kernel action emits a structured KDS event with event name, source contract, severity, and mandatory payload fields.

Severity vocabulary is DEBUG, INFO, WARN, ERROR, and CRITICAL. CRITICAL security, reliability, and constitutional events are never silently dropped.

## ACRONYMS AND TERMS

ADR: Architecture Decision Record. A formal record of a significant design choice.
AMX: Advanced Matrix Extensions. Intel matrix acceleration instruction set.
APIC: Advanced Programmable Interrupt Controller.
AVX: Advanced Vector Extensions.
BPF/eBPF: Extended Berkeley Packet Filter, a restricted in-kernel program execution environment.
CE: Correlation Engine. The SAIRU component that builds causal graphs from event streams.
CFS: Completely Fair Scheduler, the base normal scheduling algorithm.
COW: Copy-On-Write, the memory sharing mechanism used by fork and private mappings.
CR3: x86 register holding the physical address of the active top-level page table. In SAIOS it is a mirror, not the owner of address-space identity.
DeviceContract: Contract owning device registration, state machine, resource arbitration, binding, power management, and telemetry.
DIS: Diagnostic Intelligence Subsystem.
DMA: Direct Memory Access.
EIS: Event Intelligence Subsystem, the structured event system built around KDS.
FR: Flight Recorder, the persistent archive of KDS events.
GDT: Global Descriptor Table.
GS: x86 segment register used for per-CPU data and TLS boundaries.
HAL: Hardware Abstraction Layer.
IDT: Interrupt Descriptor Table.
IOMMU: Input-Output Memory Management Unit.
IPEC: SAIOS Inter-Process Event Channel, a native lock-free shared-memory IPC channel.
IRQ: Interrupt Request.
KDS: Knowledge Data Store, the append-only crash-safe evidence store that is the nervous system and product of SAIOS.
KGS: Knowledge Graph Service, the live directed graph of entities and relationships derived from KDS events.
LSM: Linux Security Module-compatible hook framework.
MCE: Machine Check Exception.
MSR: Model-Specific Register.
NMI: Non-Maskable Interrupt.
NUMA: Non-Uniform Memory Access.
OIS: Optimization Intelligence Subsystem.
OOM: Out Of Memory.
PAE: Physical Address Extension.
PIC: Programmable Interrupt Controller.
PID: Process Identifier.
PIS: Predictive Intelligence Subsystem.
RAF: Resource Accounting Framework.
Red Ring: Controlled halt with maximum evidence preservation.
SAIRU: SAIOS Runtime Intelligence Unit. Consumer of KDS evidence; not owner of kernel state.
SGQL: SAIOS Graph Query Language, a Cypher-inspired query language for KGS and FR-backed history.
SMAP: SAIOS Mandatory Access Policy, the default MAC policy engine.
TSC: Time Stamp Counter.
TSS: Task State Segment.
VFS: Virtual Filesystem layer owned by VfsContract.
VMA: Virtual Memory Area.
XDP: eXpress Data Path, driver-level packet processing before the networking stack.

## SAIOS-SPECIFIC CAPABILITIES

CAP_SAIOS_INTELLIGENCE: Grants read access to the SAIRU query interface and SGQL. Restricted telemetry fields still require CAP_SAIOS_TELEMETRY.
CAP_SAIOS_TELEMETRY: Grants access to restricted telemetry fields, including process arguments and network payload metadata.
CAP_SAIOS_ORCHESTRATE: Grants the ability to submit approved SAIRU tasks for execution through contract APIs. It does not permit bypassing contract ownership.
CAP_SAIOS_POLICY: Grants the ability to modify SAIRU Policy Engine rules. It does not permit weakening kernel-level SecurityContract enforcement.

## CROSS-SUBSYSTEM EVENT TAXONOMY

Every row lists event name, owning contract, baseline severity, and mandatory payload fields. Contract-specific documents may add fields, but may not remove these fields.

| Category | Event | Owner | Severity | Mandatory payload fields |
|---|---|---|---|---|
| Boot | BOOT_KDS_READY | ObservabilityContract | INFO | tsc_frequency, kds_size, cpu_count |
| Boot | BOOT_GATE_PASSED | Boot/owning contract | INFO | gate_number, gate_name, duration_ns |
| Boot | BOOT_GATE_FAILED | Boot/owning contract | CRITICAL | gate_number, gate_name, reason |
| Boot | BOOT_COMPLETE | Boot/SAIRU init | INFO | boot_duration_ns, processor_features_summary, kds_event_count |
| Boot | NUMA_TOPOLOGY_DISCOVERED | HAL | INFO | node_count, cpu_count, source |
| Boot | DEVICE_REGISTERED | DeviceContract | INFO | device_id, class, bus_type, bus_address |
| Boot | DRIVER_REGISTER | DriverContract | INFO | driver_id, driver_name, version |
| Process | PROCESS_CREATE | ProcessContract | INFO | pid, parent_pid, executable_path, argv_hash, env_hash, cgroup |
| Process | PROCESS_EXEC | ProcessContract | INFO | pid, executable_path, elf_architecture, interpreter_path, memory_layout |
| Process | PROCESS_TERMINATE | ProcessContract | INFO | pid, exit_code, signal, cpu_time, memory_peak |
| Process | PROCESS_SIGNAL | ProcessContract | INFO | pid, signal_number, sender_pid |
| Memory | MEMORY_ALLOC | MemoryContract | DEBUG | pid, virtual_address, size, flags, stack_trace_hash |
| Memory | MEMORY_FREE | MemoryContract | DEBUG | pid, virtual_address, size, owner_before_free |
| Memory | PAGE_FAULT | MemoryContract | INFO | pid, address, fault_type, fault_class, resolution, latency_ns |
| Memory | OOM_PRESSURE | MemoryContract/ProgressContract | WARN | pressure_percent, duration_ns, free_frames |
| Memory | OOM_KILL | MemoryContract | ERROR | victim_pid, score, memory_reclaimed, skipped_candidates |
| Memory | MEMORY_LEAK_DETECTED | MemoryContract/ProgressContract | WARN | owner, bytes, duration_ns |
| Memory | NUMA_PAGE_MIGRATED | MemoryContract | INFO | pid, old_node, new_node, page_count |
| Memory | IOMMU_FAULT | DeviceContract/HAL | CRITICAL | device_id, dma_address, allowed_range, action |
| Scheduler | SCHED_CONTEXT_SWITCH | SchedulerContract | DEBUG | from_pid, to_pid, cpu, reason, latency_ns |
| Scheduler | SCHED_PREEMPT | SchedulerContract | DEBUG | pid, preempted_by, priority, run_queue_depth |
| Scheduler | SCHED_STALL | ProgressContract | ERROR | cpu, duration_ns, runnable_count |
| Scheduler | PRIORITY_INVERSION_DETECTED | SchedulerContract/ProgressContract | WARN | blocked_pid, lock_owner_pid, inherited_priority |
| Scheduler | SCHEDULER_STARVATION | SchedulerContract/ProgressContract | WARN | pid, wait_duration_ns, boost_applied |
| Scheduler | NUMA_REBALANCE | SchedulerContract | INFO | from_node, to_node, pid_count, reason |
| Scheduler | NUMA_REMOTE_SCHEDULE | SchedulerContract | INFO | from_node, to_node, pid, reason |
| Network | NET_CONNECT | NetContract | INFO | pid, source_ip, destination_ip, port, protocol, latency_ns |
| Network | NET_ERROR | NetContract | WARN | pid, error_type, retries, packet_loss_rate |
| Network | NET_CONGESTION | NetContract | WARN | interface, queue_depth, retransmit_rate |
| Network | NETWORK_ACCOUNT_PERIOD | NetContract/RAF | INFO | pid, socket_id, bytes_sent, bytes_received, period_ns |
| Filesystem | FS_OPEN | VfsContract | INFO | pid, path, flags, latency_ns |
| Filesystem | FS_WRITE | VfsContract | INFO | pid, inode, bytes, latency_ns, dirty_page_count |
| Filesystem | FS_MOUNT | VfsContract | INFO | filesystem_type, device, mount_point, options_hash |
| Filesystem | FS_ERROR | VfsContract | ERROR | filesystem, error_type, inode, operation |
| Driver | DRIVER_REGISTER | DriverContract | INFO | driver_id, driver_name, version |
| Driver | DRIVER_INIT | DriverContract | INFO | driver_id, device_id, duration_ns |
| Driver | DRIVER_START | DriverContract | INFO | driver_id, device_id |
| Driver | DRIVER_STOP | DriverContract | INFO | driver_id, device_id, reason |
| Driver | DRIVER_ERROR | DriverContract | ERROR | driver_id, device_id, error_code, recovery_action |
| Driver | DRIVER_RESET | DriverContract | WARN | driver_id, device_id, reason |
| Device | DEVICE_REGISTERED | DeviceContract | INFO | device_id, class, bus_type, bus_address |
| Device | DEVICE_STATE_CHANGE | DeviceContract | INFO | device_id, old_state, new_state, reason |
| Device | POWER_VETO | DeviceContract | WARN | device_id, requested_state, reason |
| Device | THERMAL_THROTTLE | DeviceContract | WARN | device_id, temperature, throttle_level |
| Interrupt | IRQ_HANDLER | InterruptContract | DEBUG | irq_number, cpu, handler_time_ns, frequency |
| Interrupt | IRQ_STORM | ProgressContract/InterruptContract | ERROR | cpu, irq_number, utilisation_percent, duration_ns |
| Interrupt | MCE_USER_FRAME | HAL/InterruptContract | ERROR | frame, pid, action |
| Interrupt | MCE_KERNEL_FRAME | HAL/InterruptContract | CRITICAL | frame, cpu, action |
| Security | SECURITY_SYSCALL_DENIED | SecurityContract | CRITICAL | pid, syscall_number, policy_id, action |
| Security | SECURITY_PRIVILEGE_ESCALATION | SecurityContract | CRITICAL | pid, old_credentials, new_credentials, operation |
| Security | SECURITY_NAMESPACE_ESCAPE | SecurityContract | CRITICAL | pid, namespace_type, target, action |
| Security | SECURITY_INTEGRITY_VIOLATION | SecurityContract | CRITICAL | subject, object, violation_type |
| Security | CONTAINER_CREATE | SecurityContract | INFO | container_id, root_pid, namespace_set |
| Security | CONTAINER_DESTROY | SecurityContract | INFO | container_id, exit_status |
| Syscall | SYSCALL_ENTER | SyscallContract | DEBUG | pid, syscall_number, args_hash |
| Syscall | SYSCALL_EXIT | SyscallContract | DEBUG | pid, syscall_number, return_value, duration_ns |
| Reliability | RED_RING_SEALED | ReliabilityContract/ObservabilityContract | CRITICAL | cause, triggering_cpu, triggering_pid, evidence_event_id |
| Reliability | CONTRACT_VIOLATION | ReliabilityContract | CRITICAL | contract, invariant_id, evidence_event_id |
| Reliability | LOCK_ORDER_VIOLATION | ReliabilityContract | CRITICAL | held_lock, requested_lock, held_priority, requested_priority |
| KDS Self | KDS_OVERFLOW | ObservabilityContract | WARN | cpu, ring_id, dropped_count |
| KDS Self | KDS_CRITICAL_LOSS | ObservabilityContract | CRITICAL | cpu, ring_id, lost_event_id |
| KDS Self | KDS_WRITE_STALL | ProgressContract/ObservabilityContract | ERROR | duration_ns, pending_count |
| NUMA | NUMA_NODE_MEMORY | HAL/MemoryContract | INFO | node_id, memory_range, bytes |
| NUMA | NUMA_NODE_CPU | HAL/SchedulerContract | INFO | node_id, cpu_id |
| NUMA | NUMA_TOPOLOGY_INCONSISTENT | HAL | ERROR | source_a, source_b, discrepancy |
| NUMA | NUMA_NODE_OFFLINE | HAL/SchedulerContract | WARN | node_id, reason |
| NUMA | NUMA_MIGRATION_FAILED | MemoryContract/SchedulerContract | WARN | pid, old_node, target_node, reason |
| NUMA | NUMA_LOCALITY_DEGRADED | MemoryContract/SchedulerContract | WARN | pid, score, threshold, duration_ns |
| Accounting | CPU_ACCOUNT_PERIOD | RAF | INFO | entity, cpu_time_ns, period_ns |
| Accounting | MEMORY_ACCOUNT_PERIOD | RAF | INFO | entity, bytes, period_ns |
| Accounting | STORAGE_ACCOUNT_PERIOD | RAF | INFO | entity, bytes_read, bytes_written, period_ns |
| Accounting | ACCOUNTING_INVARIANT_VIOLATED | RAF | CRITICAL | invariant_id, expected, observed |
| Intelligence | PREDICT_OOM | SAIRU/PIS | WARN | projected_exhaustion_time, confidence, evidence |
| Intelligence | PREDICT_FS_FULL | SAIRU/PIS | WARN | filesystem, projected_full_time, confidence |
| Intelligence | PREDICT_TSC_DIVERGENCE | SAIRU/PIS | WARN | cpu_a, cpu_b, drift_ns, confidence |
| Intelligence | PREDICT_DRIVER_DEGRADED | SAIRU/PIS | WARN | driver_id, evidence, confidence |
| Intelligence | PREDICT_MEMORY_FRAGMENTATION | SAIRU/PIS | WARN | fragmentation_score, confidence |
| Intelligence | DIAGNOSTIC_MISMATCH | ObservabilityContract | ERROR | diagnostic_id, contract_state_ref, mismatch |

## ADDITIONAL EVENTS INTRODUCED BY PART 2

Part 2 adds implementation-specific event names used by subsystem documents: VMA_OPERATION, SLAB_PRESSURE, SLAB_REMOTE_ALLOC, SLAB_CROSS_NODE_FREE, SLAB_DEFRAG_COMPLETE, PAGE_CACHE_WRITEBACK, PAGE_CACHE_EVICT, JOURNAL_COMMIT, JOURNAL_CHECKPOINT, JOURNAL_ERROR, IPC_OBJECT_DESTROYED, PIPE_CREATE, PIPE_STALL, UDS_CONNECT, UDS_SCM_RIGHTS, IPEC_CREATE, MQ_DEPTH_EXCEEDED, TCP_RETRANSMIT, TCP_RESET, TCP_STATE_CHANGE, SOCKET_CREATE, SOCKET_CLOSE, DNS_QUERY, INTERFACE_UP, INTERFACE_DOWN, ROUTE_CHANGE, SOCKET_BUFFER_PRESSURE, XDP_PROGRAM_LOADED, SECURITY_MAC_DENIED, SECURITY_NETWORK_POLICY_DENY, SECURITY_AUDIT_EXEC, NUMA_KDS_SEGMENT, DEVICE_IRQ_AFFINITY_SET, DEVICE_IRQ_AFFINITY_FALLBACK, FR_NODE_ASSIGNMENT, FR_STORAGE_FAILOVER, RESOURCE_QUOTA_EXCEEDED, QUOTA_CHANGED, ACCOUNTING_ATTRIBUTION_FAILURE, and COMPAT_SEMANTIC_DEVIATION.

These events inherit global uniqueness and must be defined with full payloads in their owning contract documents.

## COMPLETION CHECK

A developer searching for a SAIOS term, acronym, capability, or event name can find one unambiguous definition here. Terms may be expanded by subordinate documents only when the expansion does not contradict this reference.
