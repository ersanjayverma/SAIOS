# SAIOS Kernel Constitution
**Document ID:** DOC-01_SAIOS_Kernel_Constitution.txt
**Layer:** Foundation
**Version:** 1.0.0
**Authority:** Highest authority in the SAIOS documentation hierarchy

## SOURCE TRACEABILITY

Primary sources:
- SAIOS_SSOT.txt: PART II, DESIGN PHILOSOPHY AND NON-NEGOTIABLE CONSTRAINTS
- SAIOS_SSOT.txt: PART II, NON-NEGOTIABLE INVARIANTS
- SAIOS_SSOT.txt: PART II, RUST KERNEL RULES
- SAIOS_SSOT.txt: DOCUMENT AUTHORITY AND CONFLICT RESOLUTION
- SAIOS_SSOT_Part2.txt: PART XII, FOURTH PILLAR DECLARATION
- SAIOS_SSOT_Part2.txt: PART XVI, CONFLICT RESOLUTION AND DOCUMENT AUTHORITY
- SAIOS_SSOT_Part2.txt: closing principle that SAIOS must remain an operating system that understands itself

This Constitution is the highest authority for all generated implementation documentation. If this document conflicts with any subordinate document, this document wins. If this document conflicts with the SSOT itself, the conflict is a constitutional defect and requires an amendment before implementation proceeds.

## FOUNDING BELIEF

The founding belief of SAIOS is that computers should understand the systems they run, not merely execute instructions. Every kernel subsystem serves execution and understanding simultaneously. Neither is secondary.

Understanding is a first-class operating system capability. It is not a dashboard, not a plugin, not a chatbot wrapper, and not a post-mortem convenience. It is a constitutional property of the kernel and the system architecture.

The governing principle is the Blackhatbadshah Principle:

Failure leads to Understanding leads to Resolution.

The operating system must reduce the distance between failure, understanding, and resolution. A subsystem that performs work but cannot explain what it did has failed its intelligence mandate.

## CONSTITUTIONAL INVARIANTS

The following invariants are constitutional law. They are stated without softening language. Implementation may refine enforcement mechanisms, but implementation may not weaken, defer, or bypass these invariants.

1. One process identifier is in exactly one execution place at all times: one CPU current slot, one run queue, one blocked wait structure, the zombie list, or dead.
2. A process identifier marked as on-CPU is present in exactly one current slot and absent from all run queues.
3. A queued process identifier is Ready, not on-CPU, and absent from all current slots.
4. The Task State Segment stack pointer for ring zero, the syscall CPU state kernel stack pointer, and the process kernel stack top are identical for the current process identifier on each CPU at all times.
5. User GS and kernel GS are never inferred from the active model-specific register alone.
6. CR3 is a hardware mirror of the current address-space handle and never the owner of address-space identity.
7. Copy-on-write reference count metadata and page table entry copy-on-write flags change together, always, without exception.
8. Process death releases address-space ownership exactly once.
9. Lock acquisition order follows the global priority order, always, without exception.
10. Knowledge Data Store reserved memory is never accessible to any path other than the ObservabilityContract.
11. SAIRU never bypasses a contract, modifies state directly, or takes action without human approval.
12. If the kernel is not in the fault zone, no user program can crash or hang it.

Every invariant violation emits a KDS event when the KDS write path is available and enters the ReliabilityContract Red Ring path when the violation is non-recoverable or constitutional.

## RUST KERNEL RULES

The kernel is written in Rust using the no-standard-library profile.

The following rules are mandatory:

- The kernel uses no_std.
- The panic strategy is panic=abort.
- Floating point is forbidden in kernel core; FPU state belongs to user processes.
- All event structures use repr(C) to guarantee a stable ABI for KDS consumers.
- Volatile reads and writes are used for memory-mapped I/O and ring buffer pointers where hardware visibility or compiler reordering matters.
- Sequentially consistent atomics are the default until a weaker ordering is proven safe for a specific path.
- Recursion is forbidden in KDS paths.
- Dynamic allocation is forbidden in interrupt context.
- Every unsafe block requires a safety comment explaining why the operation is sound.

These rules exist because fault paths, interrupt paths, and early boot paths cannot depend on heap allocation, unwinding, hidden FPU state, or ambiguous memory ordering.

## FOUR CONSTITUTIONAL PILLARS

SAIOS has four equal constitutional pillars.

Pillar 1: Execution. The system must correctly execute processes. This pillar is enforced by ExecutionContract, ProcessContract, SchedulerContract, SyscallContract, and InterruptContract.

Pillar 2: Memory. The system must correctly manage physical and virtual memory. This pillar is enforced by MemoryContract and AddressSpaceContract.

Pillar 3: Observability. The system must continuously produce structured evidence of its own behaviour. This pillar is enforced by the KDS, ObservabilityContract, and Flight Recorder.

Pillar 4: Accountability. The system must continuously attribute every consumed resource to the responsible entity. This pillar is enforced by the Resource Accounting Framework and the Accounting Constitution.

The pillars have equal weight. A system that executes correctly but cannot explain or attribute what happened is not a correct SAIOS system.

## DOCUMENT AUTHORITY HIERARCHY

The conflict resolution hierarchy is:

1. DOC-01: SAIOS Kernel Constitution
2. SAIOS_SSOT.txt, Part 1
3. SAIOS_SSOT_Part2.txt, Part 2, except where Part 2 explicitly supersedes a named Part 1 section
4. Generated subsystem and contract documents DOC-02 through DOC-18
5. Implementation code, tests, comments, and local design notes

No lower document may redefine a term, weaken an invariant, change ownership, or introduce a shortcut that contradicts a higher document.

## AMENDMENT PROCESS

A Constitutional amendment is required for any change that affects a constitutional invariant, a constitutional pillar, the authority hierarchy, the Red Ring enforcement principle, the SAIRU authority boundary, or the rule that understanding is a first-class operating system capability.

An SSOT update is required for any change that adds or changes a contract, event category, architecture layer, boot gate, compatibility phase, KDS schema field, or resource accounting invariant without changing constitutional law.

An ADR is required for an implementation choice that resolves an ambiguity while preserving the Constitution and SSOT. ADRs may document algorithms, data structures, fallback choices, and implementation tradeoffs. ADRs cannot override the Constitution or SSOT.

## KDS PRODUCT PRINCIPLE

SAIRU is not the product. SAIRU is the consumer of the product. The product is the KDS.

The KDS is the structured evidence store that makes understanding possible. SAIRU reads, correlates, explains, predicts, guides, and orchestrates from KDS evidence. SAIRU does not manufacture truth. It consumes evidence produced by contracts.

If SAIRU becomes just another AI assistant, SAIOS becomes another Linux distribution with an AI dashboard. That is a constitutional failure. The structural mitigation is KDS richness, contract-owned event emission, and deterministic evidence paths, not AI model selection.

## PRINCIPLE THAT MUST NOT BE VIOLATED

Understanding is a first-class operating system capability.

This sentence is binding. It means every meaningful kernel action must be observable as structured evidence; every contract must own and explain its state transitions; every failure mode must have a deterministic outcome; and every diagnosis must trace back to evidence rather than speculation.

The operating system that understands itself is the goal. Any architecture that preserves execution but demotes understanding to a tool, dashboard, assistant, or optional integration violates this Constitution.

## SCOPE BOUNDARY

This document does not specify contract implementation details, hardware-specific mechanisms, boot gate sequencing, intelligence engine internals, compatibility sequencing, or roadmap delivery. Those belong in DOC-03 through DOC-18 and remain subordinate to this document.

## COMPLETION CHECK

A developer reading only this document can state the twelve invariants, knows which document wins in a conflict, understands why understanding is a pillar and not a feature, and can identify that the KDS is the product while SAIRU is the consumer of KDS evidence.
