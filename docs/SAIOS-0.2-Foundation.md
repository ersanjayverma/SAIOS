# SAIOS 0.2 Foundation

Status: Active milestone
Owner: Kernel architecture and reliability
Last updated: 2026-07-02

## Milestone Goal

Stability, contracts, and continuous validation before major subsystem expansion.

## Frozen Kernel Core

The following subsystems are now declared kernel core for the 0.2 milestone:

- HAL
- Memory Manager
- Object Manager
- Service Manager
- Provider Manager
- Event Bus
- Query Engine
- SAIFS
- Scheduler
- SNSH

All new subsystems must integrate through these contracts instead of bypassing them.

## Validation Stack

SAIOS 0.2 introduces a unified validation stack:

1. KTF (Kernel Test Framework): subsystem tests and test runner.
2. KVF (Kernel Verification Framework): runtime invariant verification.
3. BST (Boot Self-Test): debug-boot health and readiness checks.
4. Health Engine surfaces for operator visibility.

## Continuous Validation Rule

Every new subsystem is incomplete until all five are done:

1. Implementation complete.
2. Tests added and registered in KTF.
3. Runtime verify routine added in KVF.
4. Health surfaced through shell and object views.
5. Registered in boot self-test path.

## Shell Workflow

Primary validation workflow in SNSH:

- test
- test memory
- test scheduler
- test console
- test object
- test all
- verify memory
- verify scheduler
- verify console
- verify object
- verify service
- verify saifs
- verify all

## Target Outcome

Development loop shifts from "write and hope" to:

Write subsystem -> Test -> Verify -> Health-check -> Merge

## Serviceized Shell Milestone

This milestone is now part of 0.2 foundation:

- SNSH/SISH is started by KSF as a kernel service.
- Shell runtime is a scheduled thread with session-owned state.
- Boot and seed paths no longer invoke shell loops directly.

Serviceized runtime shape:

Boot
-> Kernel init
-> Initialize managers
-> KSF start services
-> Spawn SISH thread
-> Scheduler + idle

This establishes continuity for future transition from in-kernel shell service to user-mode system process with minimal shell-engine rewrite.

## Self-Hosting Vertical Slice Roadmap

After reliable boot-to-shell, subsystem growth should follow a self-hosting sequence.
Priority is operator-visible capabilities, validated live in terminal, before expansion into networking or graphics.

### Phase 1: Stable Shell

Scope:

- REPL lifecycle and prompt stability
- Line editing and input reliability
- Built-in command reliability
- Command history and dispatch correctness

Demo gate:

- Boots to shell prompt without manual recovery
- Accepts repeated command execution without hangs
- Handles unknown commands and editing behavior predictably

### Phase 2: Process Execution

Scope:

- `exec` and program launch path
- ELF argument passing
- Environment variables
- Exit codes

Demo gate:

- Launches at least one nontrivial ELF program
- Returns deterministic exit code
- Preserves shell session integrity across launches

Status update:

- `exec`, `spawn`, `exit`, and `wait` process APIs are implemented and integrated.
- Program resolution uses explicit path and `/bin/<name>` probing.
- Runtime process lifecycle is recorded in Process Manager with PID and exit status.
- Binary metadata path now supports PIE and dynamic-link contracts.

### Phase 3: Filesystem Usability

Scope:

- `ls`, `cd`, `pwd` path semantics
- `cat`, `mkdir`, `rm`, `touch`
- Reliable read/write behavior

Demo gate:

- Relative and absolute paths behave consistently
- File create/read/remove sequence works end to end

### Phase 4: Process Management

Scope:

- `ps`
- `kill`
- Background jobs (`&`)
- `wait`
- Signal behavior

Demo gate:

- Foreground and background jobs are both observable and controllable
- Process termination and wait semantics are deterministic

### Phase 5: Scripting

Scope:

- Batch execution
- Startup script (`/etc/profile` equivalent)
- Variable expansion
- Pipes deferred to a later phase

Demo gate:

- Boots and executes startup script deterministically
- Executes repeatable task script without operator intervention

### Phase 6: Developer Experience

Scope:

- `dmesg`, `mem`, `cpu`, `mount`, `uname`, `time`

Demo gate:

- Every command reports actionable data tied to runtime state
- Output is stable enough for troubleshooting sessions

### Phase 7: SAIRU Integration

Scope:

- `sairu health`
- `sairu diagnose`
- `sairu explain`
- `sairu trace`

Demo gate:

- Operator can diagnose at least one injected failure from shell-only workflow

## Daily Driver Alpha Gate

Milestone objective after Phase 7:

Boot -> Login -> SISH -> Edit files -> Run programs -> Compile software -> Debug software -> Network access

This is the first point where SAIOS behaves like an operating system environment rather than a kernel-first prototype.

## Progress Measurement Rule

Track progress by terminal-demonstrable capabilities, not code volume:

- It boots.
- It accepts commands.
- It runs programs.
- It manages processes.
- It diagnoses itself.

Each milestone must end with a live shell demonstration script that can be replayed in the same order on every boot.

## Immediate Next Sprint (Post Boot-to-Shell)

1. Close shell/path semantics and command reliability regressions.
2. Add process lifecycle controls (`ps`, `kill`, `wait`, background jobs) hardening.
3. Add startup script execution path and deterministic boot script behavior hardening.
4. Add VMM intermediate page-table frame reclamation when tables become empty.
5. Add one end-to-end terminal demo checklist and keep it green on each change.
