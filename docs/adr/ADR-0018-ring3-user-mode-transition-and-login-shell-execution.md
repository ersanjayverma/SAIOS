# ADR-0018: Ring3 User-Mode Transition and Login Shell Execution

- Status: Accepted
- Date: 2026-07-05
- Complements: ADR-0014 (Kernel Managers/Providers/Services), ADR-0016 (Shell Serviceization and Session Runtime)
- Replaces: (None)

## Context

SAIOS reached a stage where ELF user binaries (notably BusyBox) were being loaded and jumped to, but execution still occurred in CPL0 due to a direct in-kernel handoff (`jmp` into user entry). This caused multiple architectural problems:

1. Fault origin ambiguity (`user` vs `kernel`) during exception handling
2. `syscall` behavior inconsistent with Linux userspace expectations
3. User process failures escalating to kernel panic/reset paths
4. Root login sessions effectively running in kernel-mode shell flow

Recent debugging confirmed:

- BusyBox first fault RIP pointed to `0f 05` (`syscall`) rather than corrupted text
- Existing entry path did not perform a privilege transition
- Login/session runtime required a ring3-first policy with safe fallback

## Decision

SAIOS will use a real privilege transition for userspace entry and a ring3-first login shell policy.

### 1. User Entry Must Transition via IRETQ

User ELF launch is changed from in-kernel `jmp` to a ring transition that builds and returns through an `iretq` frame:

- Push user `SS`, user `RSP`, `RFLAGS`, user `CS`, user `RIP`
- Execute `iretq` to enter CPL3
- Use GDT user selectors (`USER_CODE`, `USER_DATA`) for ring3 context

### 2. TSS RSP0 Must Be Set Before Drop to Ring3

Before entering userspace, kernel sets `TSS.rsp0` to a valid kernel stack top so CPU has a safe kernel stack for privilege-elevating events (exceptions/syscalls from CPL3).

### 3. Root Login Uses Ring3 Shell First

Login runtime attempts configured login shell through process execution path (ELF/user-mode path) first. If ring3 shell launch fails, runtime falls back to kernel SNSH to preserve operability.

### 4. Syscall MSR Bring-Up Is Required Foundation

Kernel initializes SYSCALL MSRs (`EFER.SCE`, `STAR`, `LSTAR`, `FMASK`) as architectural prerequisite for Linux-style userspace calling conventions.

## Scope of This ADR

This ADR standardizes transition and session policy. It does not yet claim full Linux syscall ABI parity.

Covered now:

- Privilege transition mechanics for ELF user entry
- Kernel stack handoff prerequisites (TSS `rsp0`)
- Ring3-first interactive login policy

Deferred:

- Complete syscall dispatcher bridge from hardware entry to kernel syscall service
- Signal frame delivery and full POSIX signal semantics
- Complete user/kernel context save/restore strategy for preemption

## Rationale

### Why IRETQ Transition

A direct `jmp` preserves CPL0 and invalidates user/kernel isolation assumptions. `iretq` entry enforces architectural privilege boundaries and makes exception semantics meaningful.

### Why Ring3-First Login

System behavior should reflect intended OS model: interactive root session is a user process, not a kernel loop. Ring3-first login also stress-tests user ABI earlier and exposes missing syscall pieces in controlled ways.

### Why Fallback to Kernel SNSH

Development velocity requires recoverability while ring3 ABI evolves. Fallback keeps system debuggable even when ring3 shell cannot complete startup.

## Trade-offs

Positive:

- Correct privilege boundary for userspace execution
- More accurate fault-domain classification
- Better foundation for syscall and signal architecture
- Login flow aligns with OS design goals

Negative:

- Increased complexity in entry/exception paths
- Additional failure modes during transition period
- Requires careful synchronization with IDT/TSS/syscall entry internals

## Implementation Notes

Primary implementation points:

- Ring3 entry helper in HAL (`enter_user_mode`) using `iretq`
- ELF loader handoff switched to ring3 transition path
- `TSS.rsp0` set before user entry
- Syscall MSR setup initialized during early boot
- Login runtime default shell policy set to ring3-first (`busybox`), fallback to kernel SNSH on failure

## Consequences

Short term:

- Root login attempts ring3 shell execution path first
- Faults from userspace should become easier to classify and contain
- Kernel remains debuggable via fallback path when ring3 shell fails

Medium term:

- Implement complete hardware syscall entry bridge to kernel syscall dispatcher
- Replace temporary syscall fallback behavior with proper syscall return semantics
- Expand user fault handling to robust process termination and scheduling continuation

Long term:

- Full user/kernel isolation model with stable ring3 process lifecycle
- Linux userspace compatibility improvements (incremental syscall coverage)

## Alternatives Considered

### A1: Keep CPL0 `jmp` Entry
Rejected because it is architecturally incorrect for user-mode process execution and causes persistent fault-domain confusion.

### A2: Delay Ring3 Until Full Syscall ABI Completion
Rejected because it blocks validation of transition correctness and postpones critical isolation work.

### A3: Run Login Shell Permanently in Kernel Space
Rejected because it contradicts user process model and weakens security/isolation goals.

## Validation Expectations

Expected runtime indicators after this ADR:

1. User entry transitions with ring3 selectors
2. Exception paths from userspace preserve kernel survivability goals
3. Login attempts ring3 shell path before fallback
4. Syscall instruction no longer fails solely due to missing MSR enablement

## Follow-up Work Items

1. Implement full syscall entry stub that bridges registers/frame into `kernel::syscall::dispatch`
2. Add explicit trap frame logging with saved `CS`, `SS`, `RIP`, `RSP` for first-fault diagnostics
3. Expand exception dispatcher policy for user-origin faults (`#PF`, `#UD`, `#GP`, `#DE`, `#SS`, `#NP`)
4. Remove kernel SNSH fallback once ring3 shell path is production-stable
