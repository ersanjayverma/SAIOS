# SAIOS ProcessContract Specification
**Document ID:** DOC-07_ProcessContract.txt
**Layer:** Core Kernel Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01; authoritative over process lifecycle, PID allocation, credentials, sessions, process groups, and FD inheritance

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt PROCESSCONTRACT; CROSS-SUBSYSTEM EVENT TAXONOMY process entries.

## OWNERSHIP

ProcessContract owns lifecycle state, PID allocation and release, process creation, fork, exec publication, zombie publication, dead cleanup, credentials, sessions, process groups, and file descriptor table inheritance. It does not own scheduling decisions or memory frame ownership.

## STATE MACHINE

The lifecycle has exactly six states: New, Ready, Running, Blocked, Zombie, Dead.

Permitted transitions: New to Ready on admission; Ready to Running when scheduled; Running to Ready on preemption; Running to Blocked when waiting; Blocked to Ready when woken; Running to Zombie on exit, fatal signal, or unrecoverable user fault; Zombie to Dead when reaped by the parent or reaper.

Forbidden transitions: Dead to any state; Zombie to Running; Zombie to Blocked; Ready to Dead without a terminal event; Blocked to Running without wake plus scheduling; New to Running without admission.

## INVARIANTS

1. PID is in exactly one state always.
2. Zombie is entered exactly once per PID.
3. Dead is entered exactly once per PID.
4. No PID transitions from Dead.
5. Waiters are woken exactly once on Zombie entry.

## FAILURE MODES

PID in two states simultaneously is Red Ring critical. Zombie entered twice is Red Ring critical. Waiter woken twice is Red Ring high. PID leak that never reaches Dead is a ResourceContract and ProgressContract evidence path. Credential change without audit event is Red Ring high. Fork producing non-unique PID is Red Ring critical.

## CORNER CASES

A process exiting while holding a kernel lock triggers Red Ring because locks must never outlive process ownership. OOM killer selecting Zombie skips it and emits an event. OOM killer selecting a process in kernel context sends SIGKILL and requires signal checks on every kernel exit boundary. Process group leader exit reparents members and emits audit evidence. PID 1 exit is non-recoverable Red Ring. Fork with no child stack memory causes the child COW fault to enter OOM handling; child receives SIGKILL and parent remains unaffected.

## KDS EVENTS

PROCESS_CREATE payload: pid, parent_pid, executable_path, argv_hash, env_hash, cgroup.
PROCESS_EXEC payload: pid, executable_path, elf_architecture, interpreter_path if dynamic, memory_layout.
PROCESS_TERMINATE payload: pid, exit_code, signal, cpu_time, memory_peak.
PROCESS_SIGNAL payload: pid, signal_number, sender_pid.
PROCESS_STATE_TRANSITION payload: pid, old_state, new_state, trigger, timestamp.

## FILE DESCRIPTOR INHERITANCE

fork inherits open file descriptors by duplicating descriptor table entries and incrementing references to shared file objects. O_CLOEXEC descriptors are closed during exec. FD table copying follows MemoryContract COW principles for table storage, but VfsContract owns file object semantics.

## CREDENTIAL MODEL

Each process has UID, GID, effective UID, effective GID, supplementary groups, and Permitted, Inheritable, and Effective capability sets. Credential changes require SECURITY_PRIVILEGE_ESCALATION and audit evidence before publication.

## COMPLETION CHECK

A developer can draw the six-state lifecycle with every permitted transition, event, and corner-case resolution path.
