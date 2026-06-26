# SAIOS A+ Release Dashboard - 2026-06-20

Branch: `main`
Audit Baseline: Constitutional Audit 2026-06-20
Policy: no new features until constitutional backlog is complete
Runtime Authority: retained live boot log `logs/batch1-live-proof-20260620-224110.txt`

## Overall Status

| Category | Status |
| --- | --- |
| Blockers | 2/3 complete |
| Critical | 0/4 complete |
| High | 0/5 complete |
| Medium | 0/2 complete |
| Total Backlog | 2/13 complete |
| A+ Release Ready | NO |

## Release Health

| Domain | Status | Note |
| --- | --- | --- |
| Boot | PARTIAL | Live boot reaches `BOOT_COMPLETE`, login, and pre-login `bootselftest` storage probe passes. Full gate still blocked by incomplete `testsaios`. |
| Storage | PASS FOR BATCH 1 | Live rootfs diagnostics, pre-login storage probe, and `TEST_PASS storage_matrix` agree in retained live proof. |
| Installer | OPEN | Not started in this batch. |
| SMP | OPEN | Live log still shows scheduler visibility lag/deferred release. |
| Userspace | PARTIAL | `/bin/sh` launches in live mode. `testsaios` still stalls at `TEST_STEP usertest wait_child`. |
| Observability | OPEN | Current live run shows architecture/storage progress, but Batch 2 has not started. |
| Reliability | OPEN | Not started. |
| Overall | NOT READY | Batch 1 is still in progress. |

## Batch Progress

| Batch | Description | Status |
| --- | --- | --- |
| Batch 1 | Release Blockers | IN PROGRESS |
| Batch 2 | Observability Completion | NOT STARTED |
| Batch 3 | Kernel Correctness | NOT STARTED |
| Batch 4 | Reliability Proof | NOT STARTED |
| Batch 5 | Installer A+ Validation | NOT STARTED |
| Batch 6 | Release Polish | NOT STARTED |

## Future Backlog

| ID | Title | Status | Priority | Timing |
| --- | --- | --- | --- | --- |
| FUTURE-001 | Move kernel ownership from ESP FAT partition to ext4 `/boot` hierarchy; keep ESP loader-only. | DEFERRED | ARCHITECTURE | After A+ stabilization and installer proof; must not interrupt CCB-007 or the constitutional remediation backlog. |

## Batch 1 Items

| ID | Title | Status | Evidence |
| --- | --- | --- | --- |
| CCB-001 | Fix post-mount storage validation inconsistency | LIVE PROVEN | Retained live proof shows `[boot] cmdline: saios.mode=live`, valid MBR/ext4 diagnostics, `PASS: storage probe readable`, `TEST_PASS bootselftest`, and `TEST_PASS storage_matrix`. |
| CCB-002 | Make `/bin/sh` executable from mounted rootfs | LIVE PROVEN | Retained live proof shows `[spawn] vfs_exec ok path='/bin/sh' type=RegularFile size=13848 bytes=13848 magic=7f454c46` and `[userspace-shell] entered /bin/sh`. |
| CCB-007 | Retain complete green `testsaios` transcript | OPEN | Retained live proof starts `TEST_START testsaios`, passes boot/storage matrices, then stalls at `TEST_STEP usertest wait_child`; no final green summary is retained. |

## Current Fixes Under Proof

| Area | Fix | Proof Status |
| --- | --- | --- |
| AHCI multi-sector reads | Copy each sector from the shared DMA buffer immediately after the sector command completes. | `cargo check` passed; live storage proof retained. |
| AHCI concurrent I/O | Serialize whole AHCI read/write/flush operations around the single shared command slot and DMA buffer. | `cargo check` passed; live storage proof retained. |
| Live root selection | In `saios.mode=live`, diagnose installed ext4 but boot from recovery tmpfs root to avoid mutating HDD root during live validation. | `cargo check` passed; retained live proof shows recovery rootfs populated and `/bin/sh` launched. |
| Early Windows scaffold init | Use serial output in early Windows compatibility scaffold init instead of the normal console path before heap/runtime console setup. | `cargo check` passed; retained live proof reaches Gate 5 and the live cmdline after this fix. |
| Process exit publication | Publish off-CPU exited children immediately so externally killed test children become waitable. | `cargo check` passed; full `testsaios` green proof still blocked at `usertest wait_child`. |
| Testsaios child wait | Route ordinary embedded probe waits through the ProcessContract child-waiter path; retain polling timeout mode for intentional non-exiting probes. | `cargo check` passed; retained live proof still stalls at `TEST_STEP usertest wait_child`, so this is not sufficient yet. |

## Live Evidence Snapshot

| Requirement | Latest Evidence | Status |
| --- | --- | --- |
| Live boot authority | `[boot] cmdline: saios.mode=live` | PASS |
| Rootfs diagnostics | MBR valid, ext4 partition valid, live recovery rootfs populated with 23 entries | PASS |
| Pre-login bootselftest storage | `PASS: storage probe readable` | PASS |
| `/bin/sh` executable | `[spawn] vfs_exec ok path='/bin/sh' ... magic=7f454c46` | PASS |
| Storage matrix | `TEST_PASS storage_matrix duration_ms=216` | PASS |
| Complete testsaios | Stalls at `TEST_STEP usertest wait_child`; no final green summary retained | OPEN |

## Open Issues Count

| Severity | Open |
| --- | ---: |
| BLOCKER | 1 |
| CRITICAL | 4 |
| HIGH | 5 |
| MEDIUM | 2 |
| Total | 12 |

## Gate Decision

Release remains blocked.

Batch 1 cannot be marked green until a fresh live boot retains:

- `PASS: storage probe readable` in pre-login `bootselftest`.
- `TEST_PASS bootselftest`.
- `TEST_PASS storage_matrix`.
- `/bin/sh` live shell launch with ELF magic.
- Final `[testsaios] summary ... FAIL=0 PANIC=0 TIMEOUT=0`.

The first four proof requirements are present in `logs/batch1-live-proof-20260620-224110.txt`. The remaining blocker is CCB-007: `testsaios` does not progress past `TEST_STEP usertest wait_child`.