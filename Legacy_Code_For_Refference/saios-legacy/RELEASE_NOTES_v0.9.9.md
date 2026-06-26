# SAIOS v0.9.9 Release Notes

Release date: 2026-06-20

`v0.9.9` is the current repository metadata baseline. This file is the release-script and CI-facing notes file for tag `v0.9.9`; the detailed current state is tracked in `CURRENT-STATE-OF-SAIOS.md`.

## Highlights

- Runtime version metadata is aligned across `shared_version.rs`, `Cargo.toml`, root `README.md`, and this root release-notes file.
- Latest serial evidence reaches `BOOT_COMPLETE`, Gate 16 process infrastructure, login, keyboard username input, userspace `/bin/sh` spawn, and internal-shell bridge entry.
- Memory, heap, slab allocation, KDS reservation, SMP bring-up, IOAPIC, syscall MSR setup, PS/2 keyboard/mouse init, AHCI disk detection, ext4 root mount, VFS mounts, and e1000 device initialization are runtime-evidenced.
- Login process scheduling is runtime-proven: CPU0 picks BSP-affine PID 4 and enters `[login]`.
- Keyboard input at login is runtime-proven through decoded `root` scancodes.
- Userspace shell handoff is partially runtime-proven: login spawns `/bin/sh` as PID 12 and the userspace bridge enters the internal shell path.
- Rootfs metadata repair now replaces stale non-ELF `/bin/sh` and `/bin/bash` shims with the embedded ELF shell bridge.
- CI build hygiene is expected to cover target `cargo check`, host `cargo test`, `cargo fmt --check`, target `cargo clippy -D warnings`, release target check/build, metadata consistency, and documentation gates.

## What SAIOS Can Demonstrate In v0.9.9

- BIOS/UEFI build artifact generation through the project scripts.
- Boot through core initialization gates to `BOOT_COMPLETE`.
- Four-CPU SMP initialization with scheduler-visible CPU mask convergence.
- AHCI-backed disk discovery and ext4 root filesystem mount.
- VFS root/tmp/proc/dev filesystem setup.
- e1000 NIC device initialization with MAC/IP assignment.
- Kernel process creation and admission for boot thread, flight recorder, bgworker, login, AP idle threads, kworkers, and userspace shell.
- Scheduler activity after process infrastructure starts, including CPU0 dispatch of login PID 4.
- Login prompt input through the PS/2 keyboard pipeline.
- Userspace `/bin/sh` bridge into the internal SAIOS shell.

## Known Limits In This Release

- Full native userspace shell/readline is not complete; `/bin/sh` currently bridges into the internal kernel shell through a SAIOS-native syscall.
- Existing persistent rootfs images may contain older text `/bin/sh` or `/bin/bash` shims; current metadata repair treats those stale non-ELF files as replacement targets.
- External networking, ext4 write persistence, full Linux ABI validation, and runtime shell command coverage still need broader runtime validation.
- Windows support remains experimental scaffolding.
- CI validates build, lint, metadata, and documentation hygiene more strongly than runtime boot behavior.

## Recommended Companion Docs

For maintained current state and planning, use:

- `README.md`
- `CURRENT-STATE-OF-SAIOS.md`
- `docs/status/implementation-status.md`
- `docs/status/known-issues.md`
- `docs/status/release-status.md`
- `docs/plan/roadmap.md`
- `docs/status/test-results.md`
