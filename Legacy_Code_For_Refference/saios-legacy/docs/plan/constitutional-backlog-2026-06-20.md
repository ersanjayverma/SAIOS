# SAIOS New Constitutional Backlog - 2026-06-20

This backlog is ranked strictly by ROI for the shortest path to an A+ SAIOS release using the existing architecture. It excludes new features and focuses only on correctness, stability, reliability, testability, observability, and completing partially implemented systems already present in the tree.

## A+ Grading Contract

The release target is A+ across Boot, Storage, Installer, SMP, Userspace, Observability, Reliability, and Overall. A+ means more than passing the first happy path. It requires fresh retained runtime evidence, green tests, no unexplained warnings, no timeout/deferred/fallback paths, no stale repair paths, and repeatability across the release modes already implemented in the tree.

| Domain | A+ target |
| --- | --- |
| Boot | Every boot gate, bootselftest, init, login, and shell handoff is deterministic and warning-free in retained serial logs. |
| Storage | Disk, partition, ext4, VFS, rootfs seeding, executable reads, file growth, remount, and reboot persistence all pass. |
| Installer | Existing install/update/recovery/rollback flows execute with approval, KDS evidence, verification, and no unintended mutation. |
| SMP | All accepted CPUs publish scheduler ownership without lag/deferred release/stale repair and pass runtime SMP coverage. |
| Userspace | `/bin/sh` is the normal shell, wait/reap works, and every wired userspace/syscall test passes. |
| Observability | Every release-critical subsystem emits queryable, contract-owned KDS evidence with no missing memory/process/storage/reliability gaps. |
| Reliability | Red Ring, panic, watchdog, fault, lock-order, and post-fault evidence paths are proven and no-allocation safe. |
| Overall | Multiple fresh transcripts show all domains at A+, `testsaios` green, installer green, and no unresolved warnings or degraded paths. |

Anything below A+ remains OPEN, even if it would previously have been graded A or A-.

## CCB-001

Title: Fix post-mount storage validation inconsistency

Severity: BLOCKER
ROI Score: 100

Evidence:

- Runtime rootfs diagnostics show MBR valid, partition 2 type `0x83`, ext4 superblock magic `0xef53`, state `Root Mounted`, and `[fs] ext4 mounted as root`.
- Later `bootselftest` fails `storage probe readable`.
- Later `storage_matrix` fails `partition table detected`, `partition discovered`, and `filesystem probe` while passing `disk detected` and `root mount`.
- Implementation source: `block::diagnose()` and `block::validate_storage()` drive those checks from current block device reads and root mount state.

Root Cause:

Post-mount storage validation is not observing the same valid partition/probe state that boot used to mount ext4. The most likely local causes are block read instability after mount, diagnostic reads racing with ext4/rootfs operations, or diagnostic state being recomputed differently from the successful boot probe.

Required Fix:

Make `block::diagnose()` and `validate_storage()` return consistent post-mount evidence for the active root device. Preserve the successful rootfs probe result or make fresh block reads reliable under mounted ext4. Do not add new storage architecture.

Validation Criteria:

- Fresh serial log shows boot rootfs diagnostics and post-boot `storage_matrix` agree on partition table, partition discovery, filesystem probe, and root mount.
- `bootselftest` passes `storage probe readable`.
- `storage_matrix` passes all five low-level storage checks.

## CCB-002

Title: Make `/bin/sh` executable from mounted rootfs

Severity: BLOCKER
ROI Score: 98

Evidence:

- Runtime: `[login] spawning userspace shell path=/bin/sh` followed by `[spawn] vfs_exec err path='/bin/sh' errno=-5 error=Io` and `login: entering internal shell`.
- Implementation seeds `/bin/sh` from embedded `SAIOS_SHELL_ELF` in `saios::rootfs::initial_files()` and repairs stale metadata during `repair_rootfs_metadata()`.
- `process::spawn_with_args_env()` fails before ELF loading because `VfsContract::exec_image()` cannot read the executable image.

Root Cause:

The normal userspace shell path is blocked at VFS/ext4 executable read time, not at scheduler entry. It likely shares root cause with CCB-001: mounted ext4 is accepted as root but later executable reads return `EIO`.

Required Fix:

Fix rootfs file seeding, metadata repair, ext4 read, or VFS exec-image read so `/bin/sh` resolves, has execute permission, reads bytes beginning with ELF magic, and spawns from the persistent root.

Validation Criteria:

- Fresh boot log shows `[spawn] vfs_exec ok path='/bin/sh' ... magic=7f454c46`.
- Login starts userspace shell without falling back to the internal shell.
- Shell child exits or continues under wait/reap without losing scheduler ownership.

## CCB-003

Title: Restore MemoryContract runtime evidence

Severity: CRITICAL
ROI Score: 95

Evidence:

- Runtime: `architecture_matrix` fails `Memory contract active`.
- Test code requires `kds::count_metrics(KdsMetricId::PageAlloc) > 0`.
- `MemoryContract::diagnostic_view()` counts mmap/munmap/mprotect/COW/fault events and page alloc/free metrics, and SAIRU consumes the same view.

Root Cause:

Memory allocation occurs during boot, but the KDS-visible `PageAlloc` metric is absent, not flushed, emitted through a path bypassing MemoryContract, or lost before validation.

Required Fix:

Route release-relevant frame allocation/free paths through `MemoryContract` or emit equivalent aggregate metrics through the existing KDS path. Keep interrupt/fault constraints intact and avoid high-volume per-page event spam.

Validation Criteria:

- `architecture_matrix` passes `Memory contract active`.
- `sairu diagnose memory` prints nonzero MemoryContract evidence.
- `observability_activity_matrix` later passes memory mapping/fault evidence checks when the suite reaches it.

## CCB-004

Title: Repair SAIRU memory diagnosis

Severity: CRITICAL
ROI Score: 90

Evidence:

- Runtime: `SAIRU runtime available`, tools, skills, tasks, health, freeze, contract boundary, evidence citations, and deterministic output pass.
- Runtime: `FAIL SAIRU diagnose memory` and `FAIL SAIRU validation`.
- Source: `sairu::validate_runtime()` marks memory diagnosis true only when MemoryContract diagnostic view has mmap or page allocation evidence.

Root Cause:

SAIRU memory diagnosis is structurally present but starved of MemoryContract evidence. It fails because memory observability is incomplete, not because SAIRU needs a new intelligence feature.

Required Fix:

After CCB-003, ensure `sairu diagnose memory` consumes the restored evidence and reports deterministic, cited output. Keep Phase 1 model-free.

Validation Criteria:

- `architecture_matrix` passes `SAIRU diagnose memory` and `SAIRU validation`.
- Manual `sairu diagnose memory` in shell references MemoryContract/KDS evidence with nonzero counts.

## CCB-005

Title: Complete resource accounting owner coverage

Severity: HIGH
ROI Score: 84

Evidence:

- Runtime: `[testsaios] Resource accounting coverage: 6/10 implemented, 4 pending`.
- Runtime currently treats pending owners as tracked, but the Constitution and Part 2 define accounting as a pillar.

Root Cause:

Resource accounting has a deliberately partial owner/kind matrix. Runtime evidence admits four resource kinds remain pending.

Required Fix:

Implement missing owner coverage for existing ResourceContract resource kinds, or explicitly downgrade unsupported owners so release validation does not claim complete accounting. Do not add new accounting categories.

Validation Criteria:

- Runtime coverage reports all current resource kinds implemented or intentionally non-release-gated with documented evidence.
- No `pending` resource owners remain in release validation output.

## CCB-006

Title: Eliminate SMP scheduler visibility lag

Severity: HIGH
ROI Score: 82

Evidence:

- Runtime: `[smp] initialization lag: started=0xf initialized=0x7 missing_initialized=0x8; scheduler release blocked`, resolved after 5 ms.
- Runtime: `[smp] started=0xf initialized=0xf scheduler_visible=0x1 accepted=0xf; scheduler release deferred`.
- Runtime: `[smp] scheduler visibility lag ... missing_scheduler_visible=0xe`, resolved after 66 ms.
- Docs require one PID in exactly one execution place, current-slot correctness, and per-CPU scheduler ownership.

Root Cause:

AP initialization and scheduler-current publication are eventually consistent rather than atomically ready at release time. The current log proves convergence but also proves a release delay path.

Required Fix:

Stabilize AP idle registration and scheduler-visible publication so scheduler release happens after all accepted CPUs have valid current/idle ownership. Preserve the existing shared FIFO scheduler model.

Validation Criteria:

- Fresh boot log has no `scheduler release deferred`, no `scheduler visibility lag`, and no stale ownership repair warnings.
- SMP test proves workers execute on every CPU in the scheduler-visible mask.

## CCB-007

Title: Retain complete green `testsaios` transcript

Severity: HIGH
ROI Score: 80

Evidence:

- Runtime starts `TEST_START testsaios timeout_ms=120000` and begins matrices.
- Latest retained log ends after `/tmp/usertest` exits as a zombie and does not show the final `[testsaios] summary` or `TEST_PASS testsaios`.
- Many release-critical tests are wired after `usertest`: PIE, validate, fork ABI, execve, faults, memory permissions, libc, threads, futex, signals, wait/reap, pipes, syscall ABI, capability, and observability activity.

Root Cause:

The latest runtime proof is incomplete. Either the suite stopped, the log capture ended too early, or scheduler/wait completion did not return to the test runner after the child exited.

Required Fix:

Make `testsaios` run to a retained final summary and preserve the full serial transcript. If it stalls after usertest, repair the local wait/reap/scheduler continuation path rather than skipping tests.

Validation Criteria:

- Fresh serial log contains final `[testsaios] summary PASS=... FAIL=0 PANIC=0 TIMEOUT=0`.
- The log retains PASS lines for every runtime test wired in `run_testsaios_suite()`.

## CCB-008

Title: Prove installer, update, and recovery A+ execution

Severity: CRITICAL
ROI Score: 78

Evidence:

- Runtime storage platform validations pass advisory/simulation checks, but the latest log does not prove actual install, update, recovery, rollback, or post-install boot execution.
- Existing source contains install, update, recovery, rootfs population, boot repair, and override workflows; this is proof work for current architecture, not new installer scope.
- User target requires Installer A+ rather than merely A.

Root Cause:

Installer architecture exists but is not retained as end-to-end runtime proof. Advisory success does not prove that install media can safely mutate the selected disk, seed rootfs, install boot files, reboot, update, roll back, and recover.

Required Fix:

Drive existing install/update/recovery workflows to completion under explicit approval, with KDS evidence and verification after each stage. Repair only correctness gaps in the current installer/storage platform path.

Validation Criteria:

- Fresh retained transcript proves empty-disk install, installed-system boot, update with rollback evidence, recovery verification, and storage diagnostics after each operation.
- Installer output records target disk, selected partitions, rootfs population, bootloader result, KDS operation ID, and final verification status.
- No unintended disk mutation occurs outside the selected target, and failure paths leave actionable KDS/serial evidence.

## CCB-009

Title: Prove ReliabilityContract and Red Ring A+ behavior

Severity: CRITICAL
ROI Score: 76

Evidence:

- Runtime proves watchdog/freeze diagnostic slices, but not Red Ring trigger conditions, panic output, KDS preservation, no-allocation fault-path safety, or lock-order enforcement under failure.
- Docs require constitutional invariant violations and non-recoverable faults to enter ReliabilityContract Red Ring handling.
- User target requires Reliability A+.

Root Cause:

Reliability mechanisms are initialized, but the latest evidence proves only normal boot and a synthetic freeze diagnostic path. A+ requires retained destructive/fault-path proof, not just initialized contracts.

Required Fix:

Use existing fault, watchdog, panic, lock-order, and Red Ring paths to produce controlled validation transcripts. Repair any unsafe allocation, missing KDS event, missing serial output, or non-deterministic halt behavior discovered by those tests.

Validation Criteria:

- Fresh retained transcript proves Red Ring entry, serial diagnostic output, KDS event/flight-recorder preservation, halted unsafe execution, and readable post-fault evidence.
- Panic and fault tests complete through the intended ReliabilityContract path without double fault, allocator dependency, or lost diagnostic output.
- Lock-order and watchdog violations produce deterministic evidence and do not corrupt scheduler/process ownership.

## CCB-010

Title: Establish repeatable A+ release transcript set

Severity: HIGH
ROI Score: 74

Evidence:

- Latest retained runtime log ends before final `testsaios` summary and contains known failures/warnings.
- A+ Overall cannot be proven from a single partial transcript.
- Existing validation entry points already include shell `testsaios` and runtime validation scripts; no new test framework is required.

Root Cause:

SAIOS currently lacks a retained release evidence bundle that proves the same green behavior across cold boot, installed boot, live/tooling mode, installer, userspace, observability, and reliability paths.

Required Fix:

Define and retain the A+ release evidence bundle using existing boot scripts, serial logs, and runtime tests. Repair any local issue that prevents repeatable green transcripts.

Validation Criteria:

- At least two fresh independent runs retain matching green transcripts for boot, storage, installer, SMP, userspace, observability, reliability, and full `testsaios`.
- The release evidence bundle contains timestamps, artifact identity, serial logs, test summaries, and failure-free KDS/diagnostic excerpts.
- No unresolved `warning`, `timeout`, `fallback`, `deferred`, `failed`, `stale`, `not implemented`, or `pending` release-path strings remain in the A+ transcript set.

## CCB-011

Title: Finish current ext4 write allocation behavior

Severity: HIGH
ROI Score: 72

Evidence:

- Source: `src/fs/ext4/extent.rs` states `Full allocation (creating new extents) is TODO` and returns `ext4: write to sparse block NYI` for missing extents.
- Runtime rootfs and install workflows rely on ext4 persistence and rootfs seeding.

Root Cause:

ext4 supports enough structure to mount/read/write existing extents, but creating new extents remains partial. This can break rootfs seeding, file growth, package/install metadata, and shell persistence.

Required Fix:

Complete the existing ext4 extent allocation path for file growth and sparse writes within the current ext4 implementation. Do not introduce a new filesystem.

Validation Criteria:

- Runtime can create, grow, read back, and fsync files on ext4 root.
- Storage matrix and VFS file write/read tests pass on persistent root.
- `/bin/sh` and seeded metadata survive reboot or remount validation.

## CCB-012

Title: Remove unexplained PS/2 keyboard init warning

Severity: MEDIUM
ROI Score: 58

Evidence:

- Runtime: `[kbd] PS/2 keyboard init-warning config=0x77->0x75 irq1=true translation=true flushed=0 ack=0x00 config_ready=false ack_ready=false pending_scancode=true`.
- Runtime later proves keyboard IRQs and login input work.

Root Cause:

The keyboard controller handshake reports warning state even though input works. The warning may represent an ACK/config readiness race, stale pending scancode, or overly strict readiness check.

Required Fix:

Make keyboard initialization deterministic: either obtain the expected ACK/config readiness before declaring ready, or classify this exact state as a known compatible path with KDS evidence and no warning severity.

Validation Criteria:

- Fresh boot has no unexplained `init-warning` for PS/2 keyboard.
- Login input and shell input still work with increasing IRQ counts.

## CCB-013

Title: Truth-gate partial compatibility and driver placeholders

Severity: MEDIUM
ROI Score: 45

Evidence:

- Source marks USB HID as `PARTIAL` and logs handoff deferred.
- Source has compatibility placeholders for zstd, package extraction, Wi-Fi firmware/connect, WCL, UEFI OpenProtocol, and dynamic userland.
- Docs explicitly warn not to confuse future compatibility phases with current capability.

Root Cause:

Partial and future-facing surfaces exist in the source. They are acceptable scaffolding only if release validation and user-facing claims do not report them as working systems.

Required Fix:

Keep partial surfaces gated by existing CompatibilityContract/status language. Ensure release validation treats them as out-of-scope, not failed release blockers, unless they affect current boot/storage/userspace stability.

Validation Criteria:

- Runtime help/status/tests do not claim unsupported USB HID, Wi-Fi, WCL, package-manager, container, or AI-model capabilities as complete.
- Placeholder paths emit clear gated/deferred evidence if invoked.

## Explicitly Rejected As New Feature Work

The following are not part of this remediation backlog: new AI capabilities, new drivers, new filesystems, new scheduler policies, new GUI features, new package managers, new networking features, Windows compatibility expansion, Linux compatibility expansion, containers, and roadmap delivery beyond proving or repairing existing release paths.