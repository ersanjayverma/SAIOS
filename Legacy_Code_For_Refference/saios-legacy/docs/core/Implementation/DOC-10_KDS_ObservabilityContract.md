# SAIOS Knowledge Data Store and ObservabilityContract Specification
**Document ID:** DOC-10_KDS_ObservabilityContract.txt
**Layer:** Subsystem Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01; highest priority within subsystem layer

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt EVENT INTELLIGENCE SUBSYSTEM AND KNOWLEDGE DATA STORE; OBSERVABILITYCONTRACT; BOOT SEQUENCE Gate 4 and Gate 12; FLIGHT RECORDER ARCHITECTURE. SAIOS_SSOT_Part2.txt NUMA-AWARE KDS PLACEMENT; NUMA-AWARE FLIGHT RECORDER STORAGE.

## KDS DEFINITION

The KDS is the nervous system of SAIOS. Without KDS, SAIOS is just another operating system. With KDS, SAIOS becomes an intelligence operating system. The KDS is the product; SAIRU is its consumer.

The five fundamental properties are append-only, crash-safe, physically reserved, SAIRU-accessible always, and self-describing. SAIRU can read KDS after halt with no scheduler, no VFS, and no syscall layer. Every event carries a schema version.

## EVENT SCHEMA

Every KDS event contains: event_id UUID v7, event_type enumeration, timestamp_ns never zero, source_contract, severity, cpu_id, pid where zero means kernel context, optional correlation_id UUID v7, schema_version u16, typed payload, and up to eight context tags.

Event categories include process, memory, net, fs, sched, hw, driver, security, syscall, reliability, KDS self-events, NUMA, accounting, and intelligence. The mandatory category table in DOC-02 is the registry baseline. Contract documents define full payloads for their owned events.

## PER-CPU RING BUFFER

Each CPU has an SPSC ring buffer with write_head, read_tail, buffer_base, capacity, fixed_slot_size=256 bytes, slot_count, overflow_counter, and critical_loss_counter.

Write algorithm: load write_head relaxed; load read_tail acquire; check full; if full and non-critical, drop and increment overflow counter; if full and CRITICAL, block up to 1ms; if still full, record KDS_CRITICAL_LOSS and trigger Red Ring; compute slot address; serialise event; increment write_head with release ordering.

## RECURSION GUARD

Each CPU has a kds_emitting boolean. If already true, emission is suppressed silently. If false, set true, write, then set false. It is not a lock. It is per-CPU and requires no synchronisation. It is the only mechanism preventing infinite recursive KDS emission.

## PUBLIC EMIT API

emit_event is the primary path for all severities. emit_critical is restricted to CRITICAL severity and uses blocking overflow behaviour. Both APIs fill automatic fields, enforce recursion guard, and serialize into the owning CPU ring.

## DELIVERY GUARANTEES

Ordering is guaranteed within one CPU ring. Cross-core ordering is best-effort using Lamport clocks. CRITICAL and ERROR events have zero-loss intent through blocking overflow handling; DEBUG and INFO may be sampled or dropped under pressure. CRITICAL events are persisted to Flight Recorder before acknowledgement.

## NUMA-AWARE PLACEMENT

At Gate 0, KDS reserved memory is partitioned into per-node segments. Per-CPU rings allocate from the CPU-local node segment. Gate 4 emits NUMA_KDS_SEGMENT for each node. The Flight Recorder drains rings in node-affinity order.

## OBSERVABILITYCONTRACT OWNERSHIP

ObservabilityContract owns KDS schema, telemetry tiers, aggregate providers, trace correlation identifiers, diagnostic outputs, resource-attribution evidence, validation evidence, and freeze-recorder inputs. It observes and never repairs canonical state by side effect. Diagnostic output is never authoritative; the owning contract is.

## FAILURE MODES

Non-critical ring full drops event and emits KDS_OVERFLOW metric. CRITICAL ring full beyond 1ms emits KDS_CRITICAL_LOSS and triggers Red Ring. Schema mismatch on read rejects event, emits schema error metric, and never corrupts existing records. Diagnostic output contradicting contract state emits DIAGNOSTIC_MISMATCH; contract state is canonical. Observability side effect on canonical state is Red Ring.

## FLIGHT RECORDER

FR Daemon is a dedicated low-priority kernel thread on a SAIRU reserved core. It drains per-CPU rings round-robin in node-affinity order, serialises compressed binary blocks, writes append-only 64KB blocks checksummed with CRC-32C, completes the current block on Red Ring, then writes a final block containing sealed KDS contents.

NUMA-aware FR storage uses per-node write threads pinned to local CPUs. FR_NODE_ASSIGNMENT records node_id, KDS segment, and storage device. FR_STORAGE_FAILOVER records failed device, replacement, and reason. Sensitive process arguments and network payload metadata are encrypted at rest using a per-boot TPM-derived key when available.

## KDS SELF-EVENTS

KDS_OVERFLOW payload: cpu_id, ring_id, dropped_count, severity_dropped. KDS_CRITICAL_LOSS payload: cpu_id, ring_id, lost_event_id, wait_ns. KDS_WRITE_STALL payload: duration_ns, pending_count, last_write_timestamp.

## COMPLETION CHECK

A developer can implement the per-CPU ring, recursion guard, emit_event, emit_critical, and FR Daemon with complete schema and failure guarantees.
