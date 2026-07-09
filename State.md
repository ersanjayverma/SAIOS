# SAIOS Runtime State Snapshot

Last updated: 2026-07-09

## Latest Observed Validation Output

- Profile: v0.4 readiness
- Summary: PASS
- Gates: 8/8 PASS
- Skipped gate: none
- Kernel status in that run: Kernel READY

## Latest Observed Session Outcome

- `busybox ash` (isolated ET_EXEC ring3 launch) boots to a real `/root #` prompt and
  exits cleanly in **QEMU** (verified, zero faults). In **VirtualBox** it still crashes
  (`VMState=gurumeditation`) a few syscalls later, deterministically, on the identical
  binary — confirmed to be running on VBox's NEM fallback backend (not real VT-x), so
  this looks like a hypervisor-backend-specific issue rather than a remaining SAIOS bug.
  See `status.md` for the full evidence and recommended next steps.

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
