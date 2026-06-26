# SAIOS InterruptContract and SyscallContract Specification
**Document ID:** DOC-09_InterruptContract_SyscallContract.txt
**Layer:** Core Kernel Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 and DOC-05; authoritative over IDT, interrupt dispatch, fault recovery, syscall entry/exit, and signal delivery

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt INTERRUPTCONTRACT; SYSCALLCONTRACT; BOOT SEQUENCE Gate 8 and Gate 9.

## INTERRUPTCONTRACT OWNERSHIP

InterruptContract owns IDT adapter entry, fault and IRQ classification, end-of-interrupt policy, scheduler handoff, exception recovery, and NMI Red Ring broadcast integration.

## INTERRUPT INVARIANTS

Every IDT handler completes end-of-interrupt before returning. Faults that cannot be recovered deliver a signal or trigger Red Ring; they are never silently ignored. NMI handlers never acquire locks.

## INTERRUPT CLASSIFICATION

IRQ is a hardware interrupt. Fault is a synchronous exception from an instruction. Trap is a deliberate software exception. NMI is non-maskable and used for Red Ring broadcast. Vector ranges are assigned by IDT setup at Gate 8, with architecture exceptions occupying reserved CPU vectors and device IRQs remapped away from them.

## MACHINE CHECK EXCEPTION

MCE handler is installed at Gate 1. MCE in a user frame poisons the frame, delivers SIGBUS, blacklists the frame, and emits MCE_USER_FRAME. MCE in a kernel frame is non-recoverable and triggers Red Ring with MCE_KERNEL_FRAME.

## INTERRUPT FAILURE MODES AND CORNER CASES

Double fault triggers Red Ring. Triple fault causes hardware reset; SAIRU reads last sealed KDS state post-reset. Spurious interrupt during CR3 update is safe because CR3 update is hardware-atomic; acknowledge and ignore. NMI during contract lock is safe because NMI reads only per-CPU data and takes no lock. IRQ storm is detected at above 80 percent CPU for 5 seconds and rate-limited. Kernel pointer page fault is Red Ring. User page fault attempts resolution then SIGSEGV.

The window between kernel stack switch and TSS RSP0 update is eliminated by ExecutionContract construction. NMI during Red Ring broadcast checks the Red Ring flag and swallows if already active. Interrupt handlers never call VfsContract and never allocate memory; they defer sleeping work to kernel threads or fail fast.

## SYSCALLCONTRACT OWNERSHIP

SyscallContract owns syscall entry validation, dispatch, signal processing, outcome selection, syscall exit, and per-CPU syscall state.

## SYSCALL INVARIANTS

On entry: GS is kernel-active, GS offset zero points to this CPU's syscall state, current PID resolves through ExecutionContract, and the saved user frame is complete.

On exit: return image is canonical, pending signals are processed exactly once, GS-active state matches the chosen return path, and kernel stack mirrors current process. Exactly one return path is taken per syscall: sysret or iretq, never both and never neither.

## PORTABILITY

64-bit systems configure LSTAR, CSTAR, and SFMASK for syscall/sysret. 32-bit Tier 0 systems configure INT 0x80. Both paths save the complete user frame and converge on the same internal dispatcher.

## SYSCALL FAILURE MODES

GS not kernel-active on entry is a security violation: SIGKILL plus audit event. Incomplete saved user frame is Red Ring. Signal processed twice on same exit is Red Ring high. Syscall number out of range returns ENOSYS and never panics. Non-returning syscall exits are intended termination paths. Syscall from kernel context is Red Ring if detected.

## SIGNAL DELIVERY

Pending signals are processed in priority order on every syscall exit boundary. SA_RESTART restarts eligible interrupted syscalls. SIGKILL kills the process; the syscall is abandoned and ProcessContract transitions the process to Zombie. Two pending signals remain queued and process in order.

## SYSCALL CORNER CASES

Signal arriving during syscall returns EINTR or restarts. Signal killing process during syscall abandons syscall, transitions to Zombie, and releases resources. User pointer page fault attempts resolution then SIGSEGV. Kernel pointer fault during syscall is Red Ring. execve during syscall atomically replaces the image and returns through the new image's return path.

## KDS EVENTS

IRQ_HANDLER includes irq_number, cpu, handler_time_ns, frequency. IRQ_STORM includes irq_number, cpu, utilisation_percent, duration_ns. SYSCALL_ENTER includes pid, syscall_number, args_hash. SYSCALL_EXIT includes pid, syscall_number, return_value, duration_ns. Signal events are PROCESS_SIGNAL through ProcessContract.

## COMPLETION CHECK

A developer can write the IDT table, syscall entry assembly stubs, signal delivery, and INT 0x80 fallback with deterministic Red Ring or signal outcomes for every failure mode.
