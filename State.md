# SAIOS Runtime State Snapshot

Last updated: 2026-07-10

## Latest Observed Validation Output

- Profile: v0.4 readiness
- Summary: PASS
- Gates: 8/8 PASS
- Skipped gate: none
- Kernel status in that run: Kernel READY

## Latest Observed Session Outcome

- `busybox ash` is now a genuinely usable interactive login shell, verified end-to-end
  in **both QEMU and VirtualBox**: login -> real `/root #` prompt -> type a command with
  live echo -> command executes and prints output -> back to a fresh prompt, repeatable,
  zero crashes. The VBox-specific triple fault (`VMState=gurumeditation`) is fully
  root-caused and fixed (IST-forced stack switch on ring0-mode exceptions failing under
  VBox's NEM backend specifically), and two more real bugs the fix exposed are also
  fixed: a huge-page-split path that destroyed unrelated kernel heap memory sharing its
  2 MiB window, and a `read(0, ...)` stdin stub that always returned EOF immediately
  (which is why ash could never stay interactive even before the crash was fixed).
  `ls /` hits a separate, minor, non-crashing VFS permission bug -- noted as a follow-up,
  not blocking. Full detail in `status.md`.

## Latest Applied Correction

- Fixed three bugs blocking ring3 `busybox ash`: (1) `rdx` left as the stale COM1 debug
  marker value (`0x3F8`) instead of `0` at ring3 entry in
  `hal/src/arch/x86_64/seed_support.rs`; (2) the auxv terminator (`AT_NULL`) was pushed
  in the wrong stack order in `elf_loader.rs::map_initial_user_stack`, hiding every real
  auxv entry (including `AT_RANDOM`) from libc and causing a NULL-pointer dereference in
  musl's canary-init; (3) extra argv entries (`["ash"]`) were accepted by
  `process::spawn_from` but never forwarded into `elf_loader::load_and_run`.
- Fixed a real architectural bug in `vmm::clone_current_address_space_root`: it deep-
  cloned the kernel high half of the page tables instead of sharing it by reference, so
  kernel heap growth after a process was cloned was invisible to that process, risking
  a `#PF` on valid kernel memory during syscall handling. Now shares the high half like
  `create_user_address_space_root` does; `destroy_address_space_root` updated to match
  (only frees the low half it owns). Full detail in `status.md`.

## Latest Applied Correction (continued)

- Fixed `linux_stat_mode` reporting 0 execute permission on every regular file, which
  made `ash`'s own PATH-search pre-check refuse to run anything (including
  `/bin/busybox`) without ever calling `execve`. Fixed `vfs::seed_standard_tree` to stop
  seeding dead 0-byte coreutils placeholders and redirect those names to busybox instead
  (`process::resolve_program_name`), and to give the SAIOS-native demo programs that
  actually have handlers (`hello`, `calc`, `stress`, `cc`) real stub content so they're
  runnable for the first time. Removed the debug-marker/trace-scaffolding noise added
  during this session's crash investigations (raw COM1 byte markers, per-syscall trace
  prints, per-load `elf:`/`vmm:` prints) now that the bugs they were added to diagnose
  are fixed.
- Found (not yet fixed) a separate, deeper bug: any command that requires `ash` to
  fork+exec a child (as opposed to a shell builtin) produces no output at all --
  `execve` never appears in the syscall trace after `vfork`, and the child's syscalls
  are still attributed to the parent's pid. Needs its own investigation into
  `process::fork_from`/`active_linux_pid`. Full detail in `status.md`.
- Implemented the candidate fix for that fork/exec attribution bug: newly spawned
  child user threads now start with `saved_active_exec_pid = Some(child_pid)`, so the
  scheduler restores the child's pid before the child resumes ring3 execution after
  `fork`/`vfork`. Build validation passed (`cargo check`, release loader/kernel build,
  ISO rebuild); live `ash -> ls` verification remains pending because no QEMU/VBox CLI
  is available on PATH in this environment.

## Canonical Tracker

- See status.md for full v0.4 blocker list and active workboard.
