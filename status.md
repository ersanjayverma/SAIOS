# SAIOS v0.4 Status

Last updated: 2026-07-07 (v0.4 still open; blockers clarified)

## Objective
Finish v0.4 foundation with stable static ELF execution, realistic Linux ABI behavior, and init/session correctness.

## Current Release Risk Summary
- Critical: Dynamic ELF interpreter path (`PT_INTERP`) is unsupported.
- Critical: Syscall ABI still uses selector-based pseudo-arguments for core file I/O.
- High: Ring3 shell fallback to kernel SNSH remains in normal runtime path.
- High: ET_EXEC and ET_DYN isolated address-space paths are disabled pending VMM safety fixes.
- Medium: Validation gates still aligned with v0.3 readiness framing.

## Completion Status
v0.4 is not complete.

Current blocking reasons:
- `/bin/sh` in the v0.4 DoD still depends on `PT_INTERP`/dynamic-loader support that is not implemented.
- Syscall ABI migration away from selector-based pseudo-arguments is still incomplete.
- Session/runtime behavior is improved, but kernel SNSH fallback still exists as a fallback path rather than a fully eliminated normal-path dependency.
- ET_EXEC and ET_DYN isolated address-space execution remain disabled pending VMM safety fixes.

## Workboard
- [x] Diagnose current ELF panic with deep instrumentation.
- [x] Stop low-half collateral unmap in user stack setup.
- [x] Stabilize static ELF user stack mapping for normal execution.
- [x] Define/implement v0.4 validation gate profile.
- [ ] Replace selector-based syscall arguments with pointer-buffer path for core file syscalls.
- [ ] Tighten process/session behavior for ring3 shell without SNSH fallback as normal path.
- [ ] Implement PT_INTERP handoff strategy (or explicitly defer v0.4 DoD scope).

## Active Task
Replace selector-based syscall arguments with pointer-buffer path for core file syscalls.

## Latest Observed Runtime Snapshot
- Validation summary: `PASS` (all readiness gates passed; no skips).
- Readiness gates: 8/8 PASS.
- Kernel status in latest validation run: `Kernel READY`.
- Storage boot path: foreground storage scan completed deterministically.

## Remaining Minimum To Call v0.4 Complete
- Finish pointer/buffer-based syscall ABI conversion for core file and process launch paths.
- Decide and implement `PT_INTERP` support, or formally narrow the v0.4 DoD to static-only userland.
- Re-test ring3 shell/session flow until SNSH is only an emergency path and not part of expected operator workflow.
- Re-enable or formally defer ET_DYN isolated address-space work with documented rationale.
- Re-enable or formally defer isolated ET_EXEC/ET_DYN address-space work with documented rationale.
- Run and pass a v0.4 validation sweep aligned with the actual DoD workflows.

## Notes
- Keep this file updated after every substantial change.
- `status.md` is the canonical v0.4 closure tracker; `State.md` mirrors concise runtime snapshots.
- Completed in this step: added central version source at `seed/saios/src/version.rs` and repointed product banners, shell version output, uname version fields, driver registration, and KSF service version strings to shared constants.
- Completed in this step: moved `USER_STACK_BASE` to high user VA (`0x0000_0000_7000_0000`) and verified `cargo check` passes.
- Completed in this step: added validation readiness profiles (`--ready` => v0.3, `--ready-v04` => v0.4) and wired profile-aware readiness reporting.
- In progress: syscall ABI migration started. `dispatch` now accepts pointer-based path arguments for `open/stat/getdents64` with selector fallback, and supports pointer-buffer mode for `read/write` while retaining legacy selector behavior.
- In progress: syscall ABI migration expanded. `exec`/`spawn` now accept user C-string program names (legacy selector IDs still supported for compatibility).
- In progress: syscall ABI migration expanded again. Custom `exec`/`spawn` now also accept optional user `argv` pointers (NULL-terminated `char**`) with bounded parsing.
- In progress: syscall ABI migration expanded again. Custom `fstat` now supports optional user stat-buffer output (LinuxStat layout) while preserving legacy size return.
- In progress: syscall ABI migration expanded again. Custom `getdents64` now prefers real fd + user-buffer semantics when a live descriptor is supplied, with legacy path-based fallback retained for compatibility.
- In progress: ring3 shell launch hardening added. Init now tries multiple ring3 shell candidates (`configured`, `busybox ash`, `/bin/sh`, `/bin/ash`) before SNSH fallback.
- In progress: PT_INTERP strategy refined. Ring3 shell launch now preflights absolute-path candidates and explicitly defers interpreter-backed binaries with clear logging instead of opaque exec failure loops.
- In progress: Linux errno mapping improved for exec failures. PT_INTERP/ELF-format failures now map to `ENOEXEC` rather than generic `EINVAL` for clearer user-space semantics.
- In progress: session control flow hardened. SNSH fallback is now conditional (only when ring3 shell launch fails); successful ring3 shell runs now return to login prompt instead of always dropping into SNSH.
- In progress: ring3 launch fallback widened to include built-in `shell`/`/bin/shell` candidates before SNSH, improving non-PT_INTERP session viability.
- In progress: isolated ET_EXEC execution is now disabled alongside ET_DYN to keep login/session flow on the shared bring-up path until cloned-CR3 low-half faults are fixed.
- In progress: shared-path low-half ET_EXEC loads are now rejected deterministically instead of being allowed to split/corrupt the low-half identity window during login-shell launch.
- In progress: v0.4 validation process gates were corrected for current runtime semantics (`wait(pid)` no longer self-mutates the target process, and process-creation validation now checks persistent process records instead of transient job-count growth).
- In progress: v0.4 storage mount readiness gate semantics were corrected to validate active SAIFS/VFS mount topology, skip only with no detected storage, and fail when detected storage remains unmounted.
- In progress: shell mount command now defaults to read-only (`ro`) unless explicit `rw` is requested, to reduce post-mount runtime instability while native write-path hardening continues.
- In progress: readiness process gates were decoupled from unstable ELF user-mode launch path by using deterministic `SAIOS_BIN_V1` validation fixtures, preventing post-mount validation deadlocks/failout during `validate`.
- In progress: login-shell launch planner now excludes unstable `shell`/`/bin/shell` ET_EXEC stub candidates to prevent pre-shell deadlock during ring3 fallback sequencing.
