# SAIOS Kernel Validation Suite

`validate` is the SISH-facing kernel validation command. It is intended for
post-build and hardware bring-up checks, not for demonstrations or destructive
fault injection.

## Usage

```text
SISH> validate
SISH> validate -v
SISH> validate --perf
SISH> validate --stress
SISH> validate --json
SISH> validate --ready
SISH> validate --help
```

The default run executes non-destructive subsystem checks and prints a stable
human-readable summary. `--json` emits CI-friendly structured output. `--perf`
adds measurement-oriented tests. `--stress` adds bounded stress loops that keep
running after individual failures.

`--ready` runs the v0.3 required kernel-readiness gates only and emits a
`Kernel READY` or `Kernel NOT READY` status based on those required checks.

Boot policy during v0.3:

- the kernel executes the required readiness gate set during boot
- if required gates are not all `PASS`, boot does not transition to ready runtime state

## Subsystems

The suite currently covers:

- CPU
- Memory
- Scheduler
- Process
- Syscall
- Console
- Framebuffer
- Filesystem
- Storage
- Timer
- Drivers
- Performance tests, gated by `--perf`
- Stress tests, gated by `--stress`

Unsupported or intentionally destructive checks return `SKIP` with a reason.
They do not panic the kernel.

## v0.3 Readiness Gate

During the v0.3 milestone, a core subset of validation checks is treated as a
boot readiness contract, not optional diagnostics.

Required gates:

- interrupt delivery
- timer ticks and monotonic uptime
- scheduler sleep and wake queue behavior
- page fault and invalid pointer handling policy
- stderr path availability
- rename and move behavior in VFS
- keyboard input path
- mouse input path

If required gates fail, the kernel must report `Kernel NOT READY` and must not
claim a healthy runtime state.

## Extending

Validation tests live in `seed/saios/src/kernel/validation.rs`.

To add a test:

1. Add a small `fn test_name() -> Result<(), &'static str>`.
2. Return `Ok(())` for pass.
3. Return `Err("message")` for failure.
4. Return `Err("skip: reason")` for unsupported hardware or unsafe checks.
5. Register it in `core_tests`, `perf_tests`, or `stress_tests`.

Keep tests deterministic, non-destructive, and scoped to one kernel behavior.
Clean up SAIFS files and process state created by a test whenever the kernel API
allows cleanup.
