# SAIOS Runtime State Snapshot

Last updated: 2026-07-09

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

## Canonical Tracker

- See status.md for full v0.4 blocker list and active workboard.
