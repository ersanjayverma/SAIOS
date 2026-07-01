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
