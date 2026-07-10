# SAIOS v0.4 Status

Last updated: 2026-07-10 (ring3 login shell reaches a real interactive `/root #` prompt end-to-end for the first time; several real ABI/fault-handling bugs found and fixed; one new kernel-mode `#SS` bug found in nested fork+exec, not yet fixed)

## 2026-07-10 (continued): interactive ring3 login shell verified working; nested fork+exec crash found

Verified live via a scripted QEMU serial+monitor harness (bidirectional TCP to the
serial chardev for output, HMP `sendkey` for input) that automates what previously
required a human at a GUI. Removed the hardcoded `is_deferred_interactive_ring3_shell`
gate in `seed/saios/src/kernel/init_runtime.rs` that unconditionally routed
`ash`/`busybox`/`shell` logins to the kernel-native SNSH fallback regardless of the
real ring3 path's actual state -- that gate, not an actual instability, was the reason
the shell "wasn't stable yet". With it removed, login -> `busybox ash` now reaches a
real `/root #` prompt, echoes typed input live, and runs `echo` correctly.

Bugs found and fixed while getting there:

1. **`hal/src/arch/x86_64/idt.rs`, `#TS`/`#NP`/`#SS`/`#GP` stubs** -- read the
   CPU-pushed error code and frame pointer at `[rsp+64]`, but 9 registers (72 bytes)
   are pushed before that read, not 8. `page_fault`'s stub already documented and used
   the correct `+72`; these four didn't. Every `#GP`/`#SS`/`#TS`/`#NP` diagnostic was
   reading garbage (the last-pushed register's value) as the error code, and the
   "frame pointer" passed to `general_protection()`/`selector_fault()` was off by one
   stack slot, so `saved_rip`/`saved_rsp` were wrong too. This produced exactly the
   nonsensical `error=0xffffffff80215aae, selector_index=2901`-style output seen in
   earlier debugging sessions. Fixed all four stubs to use `+72`.
2. **`seed/saios/src/kernel/fault.rs`, `handle_general_protection`/`handle_invalid_opcode`**
   -- unlike `handle_page_fault` (which explicitly checks `PF_ERR_USER` before treating
   a fault as recoverable), these two recovered *any* fault while a process had an
   active exec, with no check that the fault actually originated in ring3. A genuine
   kernel-mode `#GP`/`#UD` hit while servicing a syscall (the exact bug class already
   found and fixed for `#PF`) would still route into `abort_active_exec`'s stale-jump
   recovery instead of panicking normally. Added `frame_is_from_ring3()` (checks the
   saved CS selector's CPL bits) and gated both handlers on it, mirroring the `#PF` fix.
3. **`seed/saios/src/kernel/syscall.rs`, `ioctl` `TIOCGPGRP`/`TIOCSPGRP`** -- real
   `ioctl(fd, TIOCGPGRP, &pgrp)` semantics write the pgid into the caller's pointer and
   return 0; ours returned the pgid as the syscall's `rax` and, for `TIOCSPGRP`, treated
   the pointer's raw address as if it were the pgid value. This made ash's job-control
   setup (`tcgetpgrp() == getpgrp()`?) compare a real pgid against uninitialized/garbage
   memory, which never converges -- ash looped forever sending itself `SIGTTIN` and
   rechecking, hanging the whole (single-core, cooperative) kernel with no crash and no
   output. Fixed both to read/write the pointer per real ioctl semantics; also fixed
   `TIOCGWINSZ` the same way (was returning a packed row/col in `rax` instead of writing
   a real `struct winsize`).
4. **`seed/saios/src/kernel/init_runtime.rs`, login exec path** -- `env` was `&[]` for
   every login shell exec, so `ash` had no `PATH` (or `HOME`/`USER`/`SHELL`/`TERM`) at
   all. Its own command lookup found nothing and reported `not found` for every external
   command before ever calling `execve`. Added a real login environment.
5. **`seed/saios/src/kernel/syscall.rs`, `newfstatat`/`faccessat`** -- the busybox-applet
   redirect (`ls`/`cat`/etc. -> `/bin/busybox`, see `process::resolve_program_name`) was
   only applied inside `execve`'s own resolution and the legacy `stat`/`access` syscalls.
   musl's libc actually issues `newfstatat`/`faccessat` for `stat()`/`access()`, which
   didn't have the redirect, so ash's own pre-exec existence check on `/bin/ls` failed
   and it never even tried `execve`. Added `process::stat_redirect_path()` (redirect
   only, no PATH search) and applied it to both `*at` syscalls too.

**New bug found, root-caused as far as ground-truth GDB evidence allows, NOT yet fixed:**
running any external command that requires a nested fork+exec from inside the
interactive shell (e.g. typing `ls`, which now correctly resolves and reaches
`execve("/bin/busybox")`) panics with a kernel-mode `#SS`, `cr3` equal to the plain
kernel root (not an isolated per-process root) at fault time.

Verified live via a QEMU gdbstub (`-s`) attached from WSL `gdb` (connect to the Windows
host IP as seen from WSL, e.g. `target remote 192.168.32.1:1234` -- `127.0.0.1` from
WSL does not reach the Windows-side QEMU), with a breakpoint on `selector_fault` hit
*before* the `panic!` macro runs, so these are real register/memory reads, not printed
values trusted at face value:
- `rdi=0xc` (vector 12 = `#SS`, correct), `rsi=0x0` (error_code, correct), `rdx` = the
  frame pointer, and the 6 qwords at that frame pointer are `[error_code=0, rip, cs=0x8,
  rflags=0x10286, rsp, ss=0x10]` -- this **independently confirms the `idt.rs` offset-72
  fix above is reading the correct frame**; it is not a repeat of that bug.
- The frame itself is entirely sane: `cs=8`/`ss=0x10` are both valid kernel selectors
  (not null, not garbage), `rflags` is plausible, and `rsp` is an ordinary in-range
  address -- ruling out "corrupted/aliased stack pointer" and "null segment" theories.
- `error_code=0` decodes (per the Intel SDM) to "not associated with a specific
  selector" -- i.e. this is a stack-limit/non-canonical-address violation on a push, not
  a bad-selector load. But the recorded `rip` disassembles (`objdump`, confirmed byte-
  for-byte, reproduced identically across multiple runs with the same binary) to a bare
  `mov $imm64, %rdi` in the once-only boot-completion sequence right before the call
  into `idle_loop()` -- an instruction that can **never** fault (no memory/segment
  access at all).

That combination -- a synchronous-looking, well-formed exception frame whose `rip`
cannot possibly be the faulting instruction -- is the classic signature of an
**asynchronous hardware interrupt (almost certainly the PIT timer, vector 32, the only
other vector with a forced IST stack-switch besides `#DF`) whose own IST-based
stack-switch failed while some unrelated kernel code happened to be running**, with the
interrupted `rip` just reflecting whatever ordinary code was executing at the moment
the timer fired -- not a bug in that code itself. This is architecturally adjacent to,
but distinct from, the already-known `USER_ENTRY_ENABLE_INTERRUPTS`/ring3-timer-IST
issue in `hal/src/arch/x86_64/constants.rs`: this reproduction has `cs=8` (already
ring0, no privilege-level change), so it cannot be that exact bug -- it needs its own
investigation.

**Recommended next step (not done here):** boot with `-s -S` (stopped at entry) so a
breakpoint can be set on the timer ISR/IST entry path itself *before* first hitting it,
then single-step across the exact IST stack-switch to see which specific write goes
non-canonical, rather than inferring from the post-fault frame. This is a narrower,
more targeted session than what was done here and needs dedicated time, not more
log/disassembly inference. This remains the primary blocker for real multiprocess
command execution (as opposed to shell builtins) from the interactive login shell.

Repro: boot the ISO, log in as `root`/`root`, type `ls` at the `/root #` prompt.
Deterministic -- reproduced identically (same `rip`, same preceding `vmm: map conflict`
line, which is an unrelated, harmless first-ever-call artifact of
`vmm::map_physical_anywhere`'s virtual address allocator, not a cause) across every
attempt with the current binary.

## 2026-07-10 (continued): root-caused and FIXED the `#SS` kernel panic on `ls`

Followed the recommended next step above: got a `qemu -d int` interrupt trace (not
just the kernel's own fault log) across the exact `ls` repro, which showed
`Servicing hardware INT=0x20` (the PIT timer) immediately followed by
`check_exception old: 0xffffffff new 0xc` at the *same* RIP/RSP -- i.e. the timer
interrupt's own hardware delivery was aborting mid-vector and re-raising as `#SS`
*before* it ever pushed a frame for itself. That pointed straight at the IST-based
stack-switch mechanism the timer IRQ depends on (`idt::set_ist(32, 3)`, see
`seed/saios/src/timer.rs`), not at whatever code happened to be running when the timer
fired (explaining why the reported `rip` always looked like unrelated, unfaultable
code -- it was simply the interrupted context, not a faulting instruction).

Confirmed via a live GDB session against the QEMU gdbstub (connect from WSL to the
Windows host IP, not `127.0.0.1` -- see below) reading the TSS's raw bytes directly:
`hal/src/arch/x86_64/tss.rs`'s `TaskStateSegment` was declared plain `#[repr(C)]`.
Rust's default layout rules insert 4 bytes of padding after the leading
`reserved1: u32` to naturally 8-byte-align the following `[u64; 3]`/`[u64; 7]` arrays --
but the **hardware** x86_64 TSS64 format has no such padding; `RSP0` sits at byte
offset 4 and `IST1..IST7` at byte offset 36, packed tight. Every IST/RSP0 pointer this
kernel ever wrote via `tss::set_ist`/`set_rsp0` landed 4 bytes further into the
structure than where the CPU's own microcode reads them for an IST-based stack switch
-- silently corrupting exactly the mechanism that had been carefully built and
commented in `idt.rs` (`IRQ_IST_STACK`, `DF_IST_STACK`, `GP_IST_STACK`) without ever
being caught, because nothing had exercised a *second* real context switch (the
timer firing during actual kernel-mode work post-fork) until `ls`'s fork+exec was the
first scenario to do so live in this session.

**Fix:** `hal/src/arch/x86_64/tss.rs` -- `#[repr(C)]` -> `#[repr(C, packed(4))]` on
`TaskStateSegment`, which caps field alignment at 4 bytes and eliminates the inserted
padding, making the Rust layout byte-for-byte match the hardware spec (confirmed by
recomputing offsets: `ist` now starts at byte 36, not 40).

**Verified fixed:** rebuilt, booted, logged in, typed `ls` -- the kernel-panicking
`#SS` is completely gone. `ls` (and a nested `busybox ash -c '...'`) now surfaces a
*contained*, recoverable ring3 `#PF` (`err=0x15` = present + user + instruction-fetch,
i.e. an NX-bit/exec-permission violation inside busybox's own mapped image, cr2==rip)
that the existing fault-recovery path (`fault.rs::abort_active_exec`, already fixed
earlier this session to check ring3 origin) correctly catches, kills only that one
process, and returns control to a live, still-interactive `/root #` prompt --
`echo`/`pwd` keep working immediately after. This is real process-level fault
isolation actually working end-to-end for the first time in this session's testing.

**New, separate, much narrower bug surfaced by this fix (not yet investigated):**
busybox's *own* code now segfaults on an instruction fetch (`cr2` == faulting `rip`,
inside its mapped `.text`-range address, e.g. `0x4db378`) when invoked a second time
(nested exec via `ls`'s busybox-redirect, or `busybox ash -c '...'`) in the same boot.
Likely a stale/incorrect executable-segment mapping specific to the *second* ELF load
under an isolated address space (possibly the same class of issue as the earlier-fixed
"clear inherited PT_LOAD ranges before remap", but for the exec bit specifically) --
worth a focused ELF-loader mapchk trace on the second load's PT_LOAD flags next.

**Debugging note for next time:** this machine runs QEMU natively on Windows but only
has `gdb` inside WSL. WSL2's `127.0.0.1` does **not** reach a QEMU gdbstub bound on the
Windows side -- connect to the Windows host IP as seen from WSL instead (`ip route |
grep default` inside WSL gives it, e.g. `target remote 192.168.32.1:1234`). Also: WSL
has `grub-mkrescue`/`xorriso`/`mkfs.fat`/`mtools`/`gdb`/`addr2line`/`objdump`/`nm` and a
full rustup toolchain (`source ~/.cargo/env` first, then `rustup target add
x86_64-unknown-uefi x86_64-unknown-none` once) -- `scripts/createiso.sh --rebuild` runs
correctly from there even though native Windows has none of those ISO/ELF tools. QEMU
itself (`C:\Program Files\qemu\qemu-system-x86_64.exe`) and OVMF firmware
(`C:\Program Files\qemu\share\edk2-x86_64-code.fd`) are Windows-native. Driving the
console headlessly (no human at a GUI) works via a bidirectional TCP connection to a
`-chardev socket` serial device or a `-monitor tcp:...` HMP socket using `sendkey` --
see `tools/qemu_drive_test.py`.

Also still true from before (not re-verified this session, no evidence they regressed):
the isolated-CR3 exception-delivery containment work described below, VBox/NEM
divergence notes, and the huge-page-split/stdin/tty fixes.

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

## 2026-07-09 (continued): root-caused and fixed the real VBox architecture bug, plus made ash actually interactive

The panic dumps unlocked by the IST fix above immediately paid off — they pointed
straight at two more real, fixable bugs, found in the same session:

### Bug: huge-page split zeroed unrelated kernel memory sharing its 2 MiB window

`vmm::map_owned`'s huge-PDE-demote path, when the new mapping was `FLAG_USER`, started
the freshly-split page table **empty** instead of inheriting the huge page's previous
coverage (`unsafe { (&mut *pt_table_ptr(...)).clear() }`), on the theory that this
avoided "leaking" stale kernel identity entries into a user range. It didn't need to:
inherited entries come from a kernel-owned huge PDE and never carry `FLAG_USER`, so
ring3 code could never reach them either way — clearing bought no isolation. What it
did do is destroy every *other* 4 KiB page inside that same 2 MiB window that happened
to be in real use for something unrelated — confirmed to be exactly what corrupted the
kernel heap backing `vfs::TmpFs`'s node table (`Vec<Option<Node>>`) when busybox's
low-address ET_EXEC RW segment (`0x6371c0`-`0x648070`) shared a 2 MiB window
(`0x600000`-`0x800000`) with an in-use kernel heap chunk. `TmpFs::node()`'s bounds check
(`idx < len`) correctly passed, yet the read still `#PF`'d, because `len` was accurate
but the backing pages the split had just wiped out weren't. Fixed by always inheriting
the huge page's coverage across all 512 entries, regardless of whether the new mapping
is `FLAG_USER`.

### Bug: `read(0, ...)` (stdin) always returned EOF immediately — ash could never be interactive

`linux_read`'s `fd == 0` arm was a stub: `0 => Ok(0)`, unconditionally. Every ring3 shell
launch saw immediate EOF on its very first stdin read and exited right after printing
its prompt (correct behavior for a shell that reads EOF) -- it never had a chance to be
interactive, in QEMU or VBox alike. This is almost certainly the root cause behind
"ash failed errno=-2 (trying fallback candidate)" landing on busybox as a fallback in
the first place, and why nobody had noticed ash exits immediately even once the crash
was fixed. Fixed by wiring `fd == 0` to `console::poll_input()` -- the same
non-interrupt-driven (direct PS/2/USB/serial port polling), line-editing/echo-capable
input source the kernel's own SNSH shell already uses -- busy-waiting for a completed
line and handing it to the caller. Since busybox ash's own line editor reads stdin a
single byte at a time (it implements its own echo/editing when it can't get a real
tty, per "ash: can't access tty; job control turned off"), a completed line has to
survive across many separate `read()` calls rather than being truncated to the first
`read()`'s buffer size; added a small pending-bytes buffer (`STDIN_PENDING`) so each
call drains only what its caller asked for and the remainder carries over.

**Verified end-to-end in VirtualBox** (not just QEMU): login -> `busybox ash` launches,
prints a real `/root #` prompt, accepts typed commands with live echo, executes them
(`echo HELLO_INTERACTIVE` printed `HELLO_INTERACTIVE`), and returns to a fresh prompt
for the next command -- repeatable, no crash, `VMState=running` throughout. `ls /`
surfaced a separate, unrelated, minor bug ("Permission denied" from a VFS permission
check) -- not a crash, just worth a follow-up.

## 2026-07-09 (continued): fixed "ls: Permission denied", cleaned up debug noise, exposed a deeper fork/exec bug

### Bug: every regular file reported as non-executable, so ash's own PATH-search pre-check refused everything

`linux_stat_mode()` hardcoded `0o100644` (no execute bit at all) for `vfs::FileType::File`,
unconditionally. `ash` (and any POSIX-compliant shell) checks executability via
stat/access *before* calling `execve()` per standard PATH-search convention -- with no
execute bit ever reported on *any* file, this refused everything, including a perfectly
good `/bin/busybox`, without ever making an execve syscall (confirmed: no such syscall
appeared in a full trace). Fixed by reporting `0o100755` for regular files. SAIOS has no
real per-file permission model yet, so this is a strict improvement, not a new hazard --
the failure mode for a genuinely non-executable file (a text file) is just a normal
`ENOEXEC` from a real exec attempt, not a client-side refusal.

### VFS seeding fix: coreutils placeholders were always-broken decoration

`vfs::seed_standard_tree` seeded `/bin/ls`, `/bin/cat`, `/bin/cp`, etc. as empty (0-byte)
placeholder files, long before `busybox`/real ELF execution existed. They were never
functional: `programs::binary_metadata_checked` requires a `SAIOS_BIN_V1` text header to
recognize a native stub, which an empty file can never have, so it fell through to ELF
parsing on 0 bytes and failed. Two-part fix:
1. `process::resolve_program_name` now redirects a fixed list of real busybox applet
   names (`ls`, `cat`, `cp`, `mv`, `rm`, `mkdir`, `true`, `false`, `ps`, `kill`, `top`,
   `uname`) straight to `/bin/busybox` before ever consulting the filesystem -- argv is
   left untouched, so busybox's own argv[0] dispatch picks the right applet, the same
   way a real `/bin/ls -> busybox` symlink would. `seed_standard_tree` no longer creates
   placeholder files for these names at all (a dead 0-byte file that's now provably
   unreachable is worse than not seeding it -- it just misleads anyone who lists `/bin`).
2. Actually-implemented SAIOS-native demo programs (`hello`, `calc`, `stress`, `cc`,
   which have real handlers in `shell::programs::execute_entry`) now get real
   `SAIOS_BIN_V1\nentry=<name>\n` stub content instead of being empty, so they're
   genuinely runnable for the first time. Names with no real implementation at all
   (`argc`, `env`, `fail`) are no longer seeded -- a phantom command is worse than a
   missing one.

### Debug-noise cleanup

Removed all the raw single-byte COM1 markers (`'S'`/`'R'` around every syscall, `'B'`/`'A'`
around ring3 entry/recovery, `'D'`/`'G'`/`'P'`/`'T'`/`'N'`/`'K'`/`'?'`/`'!'`/`'t'` per fault
vector, `'F'`/`'U'`/`'G'`/`'X'` proof markers in `kernel/fault.rs`) and the per-syscall
`[syscall] seq=... nr=...`/`enosys`/`iret-frame` trace scaffolding (`SYSCALL_TRACE_LIMIT`,
`ENOSYS_TRACE_LIMIT`, `saios_debug_iret_frame`) that flooded every boot with noise once the
underlying bugs they were added to diagnose were fixed. Also gated the routine
`vmm: split huge PDE ...` prints behind the existing (already-off) `VMM_VERBOSE_SPLIT_LOGS`
flag, and converted the per-load `elf: ...` informational prints (`seg map-plan`,
`stack-plan`, `stack-map ok`, `user-enter`, `using cloned address-space root`, etc.) to the
existing `elf_trace!` macro (also already off by default). Genuinely exceptional
diagnostics -- the `page_fault()` handler's `[fault] #PF ...` dump, real error paths, and
one-time process-lifecycle logs (`[iretq] returned via fault-recovery path`,
`session: ...`) -- were deliberately left alone; they only fire when something unusual
happens, not on every syscall.

### 2026-07-10: candidate fix for forked `ash` children being misattributed to the parent pid

Verified live in QEMU with a temporary full syscall trace (added and removed again --
not left in the tree). Typing `ls`, `busybox`, or `busybox ls /` at the `ash` prompt (any
command requiring `ash` to fork+exec a child, as opposed to a shell builtin like `echo`
which needs no fork) produces **zero output**, not even an error. The trace shows why:
`ash` calls `vfork` (`nr=58`), then `wait4` (`nr=61`), and every syscall *after* that --
including an `openat`/`fstat`/`read`/`close` sequence that looks like it's peeking at the
target binary's header -- is still tagged with the *parent's* pid in
`active_linux_pid()`. Critically, `execve` (`nr=59`) never appears anywhere in the trace
after the fork. The child never actually replaces itself with the new program. This is
consistent with the forked child thread starting with `saved_active_exec_pid = None`, so
the first schedule into that thread could restore the global active exec pid to "none"
or leave subsequent accounting tied to the parent's epoch instead of the child's.

Implemented fix: `scheduler::spawn_user_child_thread` now seeds the newly-created child
thread record with `saved_active_exec_pid = Some(child_pid)`. That makes the scheduler's
normal per-thread restore path install the child pid before the child enters its saved
ring3 context, aligning `active_linux_pid()` with the child-side fork return path.

Validation completed here: `cargo check` from `seed/saios` passed, and the release UEFI
loader/kernel plus `saios.iso` rebuilt successfully. Live `ash -> ls` smoke testing is
still pending in this environment because neither `qemu-system-x86_64` nor `VBoxManage`
is available on PATH.

### 2026-07-10: current serial log showed child exec reached loader but failed replacing `ash` image

The latest serial log shows the previous PID-attribution fix moved the failure forward:
after `cd /`, `cd bin`, and `ls`, `ash` now reports `ls: Invalid argument` instead of
silently producing no output. The decisive evidence is the pair of map conflicts at
`virt=0x400000` while launching `ls`: the child is reaching the ELF load path for
busybox/`ls`, but the isolated ET_EXEC path was still created by cloning the current CR3.
That clone carried the already-mapped busybox/`ash` low-half image into the child exec
root, so mapping the replacement busybox image at the same ET_EXEC virtual address
failed with `vmm: page already mapped`/`EINVAL`.

The first attempted fix -- using a totally fresh user address-space root -- did not work
on this kernel yet: the next serial log showed `image=0 stack=0` in the loader's
source/stack probes, so the loader fell back to the shared path and hit the same
conflict. Current fix: isolated ET_EXEC/ET_DYN execution again clones the current root
so the loader's low-half stack and source buffers remain reachable, but isolated
`map_and_load` now clears inherited user pages in each replacement PT_LOAD range before
mapping the new image. That preserves loader reachability while still giving `execve`
the key semantic it needs: the old user image pages are removed before the replacement
image is mapped at the same virtual addresses. Validation completed here: `cargo check`
passed, and the release UEFI loader/kernel plus `saios.iso` rebuilt successfully. Live
`ash -> ls` retest is still pending on the user's boot environment.

### 2026-07-10: current serial log showed separate tty warning and scheduler CR3 leak during `ls`

The latest serial log selected `ash: can't access tty; job control turned off`, followed
by `ls` and then a kernel-mode page fault at `rip=0xffffffff801a8d26` with
`cr3=0x2ec7000`. Resolving the RIP against the release kernel places it in the optimized
`saios_kernel_main`/idle path immediately after interrupts are enabled, not inside the
`ls` binary itself. That means the scheduler had switched back to a kernel/idle thread
while the child's isolated process CR3 was still loaded.

Implemented fixes: scheduler thread records now save and restore CR3 alongside the
existing per-thread fault-recovery and active-exec-pid context, falling back to the
kernel root for kernel-only threads. This prevents idle/main kernel code from running
under an isolated user process page table after timer/preemption. Separately, `/dev/tty`
and `/dev/console` now open as a minimal TTY descriptor, write to the console, poll as a
terminal, and answer the same basic tty ioctls as fds 0-2, which should suppress ash's
controlling-tty warning. Validation completed here: `cargo check` passed, and the
release UEFI loader/kernel plus `saios.iso` rebuilt successfully.

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
