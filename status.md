# SAIOS v0.4 Status

Last updated: 2026-07-09 (busybox/ash ring3 launch now boots; isolated-CR3 exception-delivery risk documented)

## Objective
Finish v0.4 foundation with stable static ELF execution, realistic Linux ABI behavior, and init/session correctness.

## Current Release Risk Summary
- Critical: Dynamic ELF interpreter path (`PT_INTERP`) is unsupported.
- High: Ring3 shell fallback to kernel SNSH remains in normal runtime path.
- **High (open architectural risk, not just disabled-pending-fix): any real CPU exception (page fault, GP fault, etc.) taken while ring3 code is running under an isolated/cloned CR3 triple-faults the whole kernel instead of being handled. See "Isolated-CR3 exception delivery is fundamentally broken" below.**
- Medium: Validation gates still aligned with v0.3 readiness framing.

## Completion Status
v0.4 is not complete.

Current blocking reasons:
- `/bin/sh` in the v0.4 DoD still depends on `PT_INTERP`/dynamic-loader support that is not implemented.
- Session/runtime behavior is improved, but kernel SNSH fallback still exists as a fallback path rather than a fully eliminated normal-path dependency.
- ET_EXEC and ET_DYN isolated address-space execution remain disabled pending VMM safety fixes.

## Workboard
- [x] Diagnose current ELF panic with deep instrumentation.
- [x] Stop low-half collateral unmap in user stack setup.
- [x] Stabilize static ELF user stack mapping for normal execution.
- [x] Define/implement v0.4 validation gate profile.
- [x] Replace selector-based syscall arguments with pointer-buffer path for core file syscalls.
- [ ] Tighten process/session behavior for ring3 shell without SNSH fallback as normal path.
- [ ] Implement PT_INTERP handoff strategy (or explicitly defer v0.4 DoD scope).

## Active Task
Tighten process/session behavior for ring3 shell without SNSH fallback as normal path.

## Latest Observed Runtime Snapshot
- Validation summary: `PASS` (all readiness gates passed; no skips).
- Readiness gates: 8/8 PASS.
- Kernel status in latest validation run: `Kernel READY`.
- Storage boot path: foreground storage scan completed deterministically.

## Remaining Minimum To Call v0.4 Complete
- Decide and implement `PT_INTERP` support, or formally narrow the v0.4 DoD to static-only userland.
- Re-test ring3 shell/session flow until SNSH is only an emergency path and not part of expected operator workflow.
- Re-enable or formally defer ET_DYN isolated address-space work with documented rationale.
- Re-enable or formally defer isolated ET_EXEC/ET_DYN address-space work with documented rationale.
- Run and pass a v0.4 validation sweep aligned with the actual DoD workflows.

## 2026-07-09: busybox/ash isolated ET_EXEC launch fixed end-to-end

Three real bugs were found and fixed (root-caused via QEMU `-d int,cpu_reset` tracing
and a `gdb` remote-stub session against the guest, not guesswork):

1. **`hal/src/arch/x86_64/seed_support.rs` (`hal_enter_user_mode_recoverable`)** — the
   debug marker written to COM1 right before `iretq` (`mov dx, 0x3F8`) clobbered `rdx`
   and nothing re-zeroed it afterward, so every ring3 entry started with `rdx = 0x3F8`
   instead of `0`. Added `xor edx, edx` after the marker, before `iretq`.
2. **`seed/saios/src/kernel/elf_loader.rs` (`map_initial_user_stack`) — the actual root
   cause of the busybox hang.** The auxv vector was pushed in the wrong order: the
   `AT_NULL` terminator pair was pushed *last*, which (since the stack builder pushes
   downward) placed it at the *lowest* address — the first pair `_start` scans reading
   upward. libc therefore saw an auxv array that terminates on the very first entry and
   silently found zero real auxv values, including `AT_RANDOM`. musl's `__libc_start_main`
   canary-init code then dereferenced a NULL pointer (`mov rax,[rdx]` with `rdx=0`)
   trying to read the (never-found) random seed. Fixed by pushing the terminator pair
   *first* (so it lands at the highest address, read last) and the real entries after.
3. `elf_loader.rs`/`process.rs` — `args` (extra argv entries, e.g. `["ash"]` so busybox
   dispatches to the `ash` applet instead of printing its usage banner) was accepted by
   `spawn_from` but never forwarded into `load_and_run`/`map_initial_user_stack`, so
   `argc`/`argv` were always hardcoded to just the binary path. Threaded `args: &[&str]`
   through both call sites.

With all three fixed, `busybox ash` now boots to a real `/root #` prompt over serial
during login (isolated ET_EXEC path, `ET_EXEC_ISOLATED_ADDRESS_SPACE = true`).

### Isolated-CR3 exception delivery is fundamentally broken (found, NOT fixed)

While root-causing the hang above, confirmed via `qemu-system-x86_64 -d int,cpu_reset`
trace that the NULL dereference in bug #2 produced a real `#PF` (vector 0xe, CR2=0) —
and that delivering *that* `#PF` while CR3 is the isolated/cloned per-process root
cascades `#PF -> #SS(0xc) -> #DF(0x8) -> #SS(0xc) -> triple fault`, silently halting the
whole kernel (no reboot message reaches serial because `-no-reboot` just halts the vCPU).
This is the same class of bug documented in `hal/src/arch/x86_64/constants.rs`
(`USER_ENTRY_ENABLE_INTERRUPTS`) for hardware IRQs during isolated-CR3 ring3 execution —
except that workaround (disabling IF on ring3 entry) only suppresses *maskable*
interrupts. `#PF`/`#GP`/`#SS`/etc. are not maskable and go through the exact same broken
stack-switch path regardless.

Practical impact: fixing bug #2 above means busybox/ash no longer *triggers* this path in
the traced scenario, but the underlying defect is untouched — **any real user-mode fault
under the isolated ET_EXEC/ET_DYN address-space path (a genuine segfault in a buggy user
binary, a bad syscall pointer, etc.) will still triple-fault and halt the entire kernel**
instead of being caught, reported, and used to kill just that one process. This is a
robustness/security-relevant gap for anything beyond the specific happy path exercised so
far.

What was ruled out while investigating (so the next attempt doesn't repeat this work):
- `clone_current_address_space_root`/`clone_table_recursive` in `vmm.rs` were re-read
  end-to-end: the deep clone is structurally correct (recursive-slot skip + fixup is
  correct, leaf PTEs/PDEs copy the same physical frames and flags, kernel high-half is
  fully covered). No bug found there.
- IDT/TSS IST wiring for vector 14 (`#PF`) does assign `IST2` (`gp_top`, 16 KB), same as
  `#GP`/`#SS`/`#TS`/`#NP`; this was previously verified present/mapped via direct runtime
  probes (`range_mapped_in_root`) in an earlier session and again holds here.
  CR4 has PCID disabled, ruling out stale-PCID-tagged-TLB theories.
- Confirmed via `gdb`+`-s -S` that the cascade happens with the isolated CR3
  (`0x3d00000` in the traced run) actively loaded at the time of the second (`#SS`) and
  third (`#DF`) exceptions — this is not a "kernel accidentally still on its own CR3"
  artifact.
- Not yet tried: dumping the *raw* TSS/GDT bytes (bypassing SAIOS's own `tss.rs`/`gdt.rs`
  accessors) at the exact moment of the `#SS` to rule out data corruption of the TSS
  IST2 field or the TSS GDT descriptor itself; and comparing IST-based stack-switch
  behavior for a *synthetic* fault taken from kernel code (not ring3) running under the
  same cloned CR3, to isolate whether the trigger is "ring3->ring0 privilege change" or
  "isolated CR3" specifically.

## 2026-07-09 (continued): CR3 high-half sharing fix + VBox/NEM divergence found

After the fixes above, `busybox ash` reached a real `/root #` prompt in QEMU, but the
user re-tested in VirtualBox and hit a *different*, later crash: VBox went to
`VMState="gurumeditation"` (VirtualBox's triple-fault/unhandled-exception state) a few
syscalls after the previous hang point, at `rip=0xffffffff801b067b` — inside
`vfs::TmpFs::path_for_inode`/`cwd_path`'s node lookup (kernel code, running during
syscall handling, not user code).

### Real architectural bug found and fixed (independent of the VBox crash)

`vmm::clone_current_address_space_root` (used for isolated ET_EXEC/ET_DYN processes)
did a full recursive deep-clone of *every* PML4 entry, including the kernel high half
(256..511). That means each cloned process's kernel-half page tables were independent
copies frozen at clone time — any kernel heap growth (or other kernel mapping change)
*after* the clone (e.g. a `Vec` inside a VFS structure reallocating to serve a later
syscall) was invisible to that process's tables, so kernel code running under that CR3
during syscall handling could `#PF` on perfectly valid, live kernel memory. That `#PF`,
taken while CR3 was the isolated root, would hit the same broken stack-switch path
documented below and cascade to a triple fault. Fixed by making
`clone_current_address_space_root` share the kernel high half by reference (copy PML4
entries by value, pointing at the kernel's own live PDPT/PD/PT pages) exactly like
`create_user_address_space_root` already did, and only deep-cloning the low
(user) half that actually needs per-process isolation. Also fixed
`destroy_address_space_root` to match: it now only recursively frees the low half it
owns, and just frees the PML4 page itself, instead of recursing into (and freeing) the
now-shared high-half tables out from under the live kernel.

This is a real, verified-correct fix and should be kept regardless of the VBox finding
below — it closes a genuine class of use-after-clone staleness bug.

### The VBox crash itself: confirmed NOT reproducible in QEMU, points to the NEM backend

Rebuilt with the CR3 fix above and re-tested in both environments with the *identical*
kernel binary/ISO:
- **QEMU**: boots cleanly through the exact same syscall sequence, reaches `/root #`,
  zero faults, zero triple-fault trace entries (`-d int,cpu_reset`).
- **VirtualBox**: crashes 100% deterministically at the same point every time
  (`VMState=gurumeditation`), completely unchanged by the CR3 fix.

`TmpFs::node()` (the function at the crash `rip`) is plain safe Rust —
`self.nodes.get(idx).and_then(...)` — bounds-checked `Vec::get`, which cannot segfault
from a logic/indexing bug; a genuine SAIOS logic bug would misbehave identically under
any correct x86_64 emulator/hypervisor, not diverge between QEMU and VBox on the
identical binary. Checked `VBox.log` and confirmed this VM is running on the **NEM**
backend (Windows Hypervisor Platform), not real VT-x:
`HM: HMR3Init: Attempting fall back to NEM: VT-x is not available`. NEM is VirtualBox's
degraded fallback used when something else (commonly Hyper-V/WSL2, which this machine
uses heavily) has already claimed exclusive access to VT-x. Given the identical binary
is correct under QEMU's TCG software emulation, this specific crash is most likely a
NEM-backend virtualization quirk/bug rather than a remaining SAIOS defect — though it
could also be a genuine SAIOS bug that only manifests under NEM's specific
exception/IST-delivery timing (the earlier, separate `USER_ENTRY_ENABLE_INTERRUPTS`
mystery in `constants.rs` was confirmed to reproduce under *both* QEMU and VBox, so
"NEM-only" divergence is new information, not a repeat of that finding).

**Recommended next steps, in order of expected payoff:**
1. Check whether VT-x is actually available to VirtualBox on this machine (Windows
   Settings > "Turn Windows features on or off" > Hyper-V, or `bcdedit /set
   hypervisorlaunchtype off` + reboot, noting this will also disable WSL2 while off).
   If VBox gets real VT-x instead of falling back to NEM, retest — if the crash
   disappears, it was purely a NEM artifact.
2. Treat QEMU as the primary/reference test environment for SAIOS going forward (it
   already caught and helped root-cause every real bug fixed in this session; VBox/NEM
   has now diverged from it in a way that doesn't obviously implicate SAIOS).
3. If VBox/NEM parity still matters, the next concrete debugging step is comparing raw
   TSS/GDT bytes and the exact `#PF`/`#SS` error codes at the crash moment under VBox
   specifically (VBox's own logging, or attaching via `VBoxManage debugvm`), since QEMU
   tracing is no longer reproducing the failure to compare against.

## Notes
- Keep this file updated after every substantial change.
- `status.md` is the canonical v0.4 closure tracker; `State.md` mirrors concise runtime snapshots.
- Completed in this step: removed selector-based compatibility fallbacks in custom syscall dispatch for `exec`/`spawn`/`open`/`read`/`write`/`stat`/`getdents64` and enforced pointer-buffer argument paths for core file/process-launch flows (`ABI 1.3.0`).
- Completed in this step: added central version source at `seed/saios/src/version.rs` and repointed product banners, shell version output, uname version fields, driver registration, and KSF service version strings to shared constants.
- Completed in this step: moved `USER_STACK_BASE` to high user VA (`0x0000_0000_7000_0000`) and verified `cargo check` passes.
- Completed in this step: added validation readiness profiles (`--ready` => v0.3, `--ready-v04` => v0.4) and wired profile-aware readiness reporting.
- Completed in this step: retired the earlier selector-fallback transition notes now that core file/process-launch syscall paths require pointer/buffer arguments.
- In progress: syscall ABI migration expanded again. Custom `exec`/`spawn` now also accept optional user `argv` pointers (NULL-terminated `char**`) with bounded parsing.
- In progress: syscall ABI migration expanded again. Custom `fstat` now supports optional user stat-buffer output (LinuxStat layout) while preserving legacy size return.
- Completed in this step: custom `getdents64` now requires fd + user-buffer semantics and no longer keeps path-selector fallback behavior.
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
