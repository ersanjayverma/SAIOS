# SAIOS ExecutionContract Specification
**Document ID:** DOC-05_ExecutionContract.txt
**Layer:** Core Kernel Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01; authoritative over per-CPU state, GDT, TSS, context ownership, and GS boundaries

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt EXECUTIONCONTRACT; GDT and TSS subsection; DOC-01 constitutional invariants 4, 5, and 6.

## OWNERSHIP

ExecutionContract owns the current CPU abstraction, current process per CPU, kernel stack, saved kernel stack pointer, user register context, CR3 activation boundary, TSS ring-zero stack pointer, and GS/TLS boundaries. It provides the state SchedulerContract and InterruptContract rely on; it does not implement scheduler policy or syscall dispatch.

## PER-CPU STATE STRUCTURE

The per-CPU state is cache-line aligned. Required fields are: cpu_id, current_pid where zero means idle, current_pcb pointer, kernel_stack_top, saved_kernel_stack_pointer, tss_ring_zero_stack_pointer, user_gs_base, kernel_gs_base, interrupt_nesting_depth, preemption_disabled_count, kds_emitting recursion guard, and padding to a full cache line.

Each field has a single purpose. current_pid and current_pcb identify the CPU-owned process. kernel_stack_top, saved_kernel_stack_pointer, and tss_ring_zero_stack_pointer bind interrupt/syscall return safety. user_gs_base and kernel_gs_base prevent ambiguous active-MSR inference. interrupt_nesting_depth and preemption_disabled_count prevent illegal sleep/preemption paths. kds_emitting prevents recursive KDS emission.

## INVARIANTS

1. TSS ring-zero stack pointer, syscall CPU state kernel stack pointer, and process kernel stack top are identical for the current PID on each CPU.
2. User GS and kernel GS are never inferred from the active MSR alone.
3. CR3 is a hardware mirror, not the owner of address-space identity.
4. No two CPUs share the same current slot.

## FAILURE MODES

| Failure | Outcome |
|---|---|
| Null current slot outside idle path | Red Ring critical |
| Two CPUs with same current PID/current slot ownership | Red Ring critical |
| TSS ring-zero stack pointer mismatch | Red Ring critical |
| Kernel stack overflow | Red Ring non-recoverable |
| Non-canonical user stack pointer | SIGSEGV to process, not Red Ring |
| Non-canonical user instruction pointer on interrupt return | SIGSEGV to process, not Red Ring |
| CR3 mismatch with current address-space handle | Red Ring critical |

Each Red Ring path first emits CONTRACT_VIOLATION with contract=ExecutionContract, invariant_id, CPU, PID, and evidence fields when KDS is available.

## INTERRUPT-CONTEXT CONSTRAINT

ExecutionContract paths never sleep, never allocate, and never acquire locks below Priority One in the global lock order. Any code path needing sleep, allocation, or lower-priority locks is not an ExecutionContract path.

## GDT AND TSS

The GDT contains: null entry, kernel code segment, kernel data segment, user code segment ring 3, user data segment ring 3, and a TSS entry split across two descriptor slots in 64-bit mode.

The 64-bit TSS contains the ring-zero stack pointer, three IST entries for NMI, double fault, and critical exceptions, and the I/O map base. The ring-zero stack pointer is kept synchronized with the current process kernel stack top.

## CR3 RULE

ExecutionContract observes CR3 consistency but does not own page-table identity. CR3 writes are performed only by AddressSpaceContract through its activation API. CR3 is atomic at the hardware level and is validated against the current address-space handle.

## KDS EVENTS

ExecutionContract emits or causes: EXECUTION_CPU_ONLINE, EXECUTION_CURRENT_SET, EXECUTION_STACK_MISMATCH, EXECUTION_GS_BOUNDARY_ERROR, and CONTRACT_VIOLATION. Mandatory payloads include cpu_id, pid, current_slot, stack pointers where relevant, gs bases where relevant, and invariant_id for violations.

## SCOPE BOUNDARY

This document does not define scheduler selection, syscall entry mechanics, interrupt dispatch, or KDS ring buffer internals. Those belong to DOC-08, DOC-09, and DOC-10.

## COMPLETION CHECK

A developer can write the per-CPU state struct and GDT/TSS setup without consulting another document, and every invariant has a deterministic Red Ring or signal outcome.
