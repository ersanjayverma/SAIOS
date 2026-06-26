# SAIOS ReliabilityContract and Red Ring Specification
**Document ID:** DOC-16_ReliabilityContract_Red_Ring.txt
**Layer:** Subsystem Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 only; operationalises constitutional enforcement

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt RELIABILITYCONTRACT AND THE RED RING; PROGRESSCONTRACT non-intervention; BOOT SEQUENCE Gate 2 and Gate 14.

## OWNERSHIP

ReliabilityContract owns lock acquisition order validation and contract violation detection. It owns the Red Ring entry point and live invariant enforcement path.

## GLOBAL LOCK ORDER

Priority 1 ObservabilityContract. Priority 2 ExecutionContract. Priority 3 MemoryContract. Priority 4 SchedulerContract. Priority 5 ReliabilityContract. Priority 6 SecurityContract. Priority 7 VfsContract. Priority 8 DriverContract. Priority 9 NetworkContract. Priority 10 CosmeticsContract.

Acquiring a lower-priority lock while holding a higher-priority lock is permitted. Reverse acquisition is forbidden and is a contract violation. Constitutional contracts are interrupt-context contracts: they never sleep, never block on lower-priority lock, never allocate while holding a lock, and access KDS only through the lock-free per-CPU path.

## BOOT VALIDATION

The lock order validator is installed at Gate 2 before subsystem initialisation. Any registered acquisition that violates order halts boot with a serial diagnostic. Live validation becomes active at Gate 14.

## RED RING DEFINITION

Red Ring is a controlled halt with maximum evidence preservation. It is not an error handler and not a recovery mechanism. It is a halt that preserves understanding.

## TRIGGER CONDITIONS

Triggers include any contract invariant violation, any kernel panic, non-recoverable hardware fault, MCE in kernel frame, double fault, triple fault evidence after reset, runtime lock order violation, KDS reserved memory accessed by non-KDS path, SAIRU Policy Engine rejection followed by attempted proceed, and any subsystem directly mutating canonical state owned by another contract.

## SIX-STEP SEQUENCE

1. Detection: only valid entry is ReliabilityContract::red_ring with cause and evidence_event_id.
2. Broadcast Halt: NMI to all CPUs; each completes in-flight KDS write if active, halts below KDS write path, acquires no locks, and services no interrupts except NMI.
3. KDS Seal: ObservabilityContract marks KDS post-halt read-only and emits RED_RING_SEALED with timestamp, cause, triggering_cpu, triggering_pid, and evidence_event_id.
4. SAIRU Activation: SAIRU activates on sealed KDS using reserved memory and independent execution path established at boot.
5. Red Ring Display: display shows Red Ring signal; context and diagnosis are built from sealed evidence.
6. Human Query Surface: operator receives structured output and can query full KDS history; no action occurs without human approval.

## WHAT STAYS ALIVE

KDS remains readable. SAIRU remains available through its independent path. Red Ring display remains available. Everything else is halted.

## POST-RED RING OUTPUT STRUCTURE

Output includes trigger, owning_contract, invariant_violated, KDS event ID, timestamp, CPU ID, PID, confidence score, causal chain event list, human-readable root cause, affected contracts and subsystems, ordered recommended steps, prevention recommendation, and full KDS query availability.

## PROGRESS NON-INTERVENTION

ProgressContract emits evidence. SAIRU reads evidence and produces diagnosis and guidance. Intervention requires human approval. Automated intervention without human approval has caused more outages than it has prevented. This is a design decision.

## COMPLETION CHECK

A developer can implement lock-order validation, Red Ring entry, NMI broadcast, KDS seal, and the exact halt/read-only boundary without ambiguity.
