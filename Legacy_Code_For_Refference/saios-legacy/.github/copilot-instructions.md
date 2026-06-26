# SAIOS AI Assistant Coding Guide

Use these project rules when changing or debugging SAIOS. This is a Rust `no_std` x86_64 OS kernel, so small state mistakes in scheduler, syscall, TLS, and interrupt paths can become double faults.

## Validation

- Start from the freshest `seriallog.txt` and the current boot artifact timestamps when debugging runtime failures.
- Resolve kernel RIPs against the current release ELF before patching fault paths.
- Use `cargo check` for quick root-crate validation.
- For kernel target validation, pass the target/build-std flags explicitly:
  `cargo check --target x86_64-unknown-none -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem`.
- For release/codegen-sensitive paths, run:
  `cargo build --release --target x86_64-unknown-none -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem`.
- Do not set `[build].target` or global `build-std` in `.cargo/config.toml`; it breaks host-side Cargo commands.

## Process And Scheduler State

- Treat `process::CURRENT` as a syscall/direct-run shadow. At interrupt, fault, scheduler, and user-return boundaries, use `ProcessTable::current_ref()` or `current_pid()` for canonical CPU-current ownership.
- Blocking syscalls such as `wait4` and futex waits must operate on the process table, not only on `CURRENT`.
- Blocking a process must remove stale run-queue entries; scheduler selection must pick only Ready/Running processes and must skip processes already marked `on_cpu`.
- Scheduler finish-switch bookkeeping must be non-lossy. Do not silently skip cleanup with `try_lock()` in finish-switch paths.
- First-run synthetic contexts such as `fork_child_trampoline` and `kthread_trampoline` must run finish-switch bookkeeping before enabling interrupts or entering their real body.
- After a blocking syscall schedule returns on a waiter kernel stack, restore the waiter as the CPU current process, mark it Running/on_cpu, update TSS/RSP0, and refresh `CURRENT`.

## User Return And Syscall GS Rules

- Normal `iretq` entry to user mode must leave `GS_BASE` as the user GS value, including zero, and `KERNEL_GS_BASE` as the per-CPU syscall state.
- Syscall-origin `iretq` paths, including `execve`, signal return, and fork-child returns from syscall-blocked parents, must restore user GS through the syscall-origin path and execute `swapgs` before `iretq`.
- `sys_exit` runs after syscall-entry `swapgs` but does not return through the syscall epilogue. It must `swapgs` back before scheduler handoff and clear any syscall-GS-active tracking.
- `arch_prctl(ARCH_SET_GS)` runs inside syscall-swapped state. If kernel GS is active, update the saved user GS through `KERNEL_GS_BASE`, not active `GS_BASE`, or the next syscall can load a bad stack from `%gs:0`.
- Fork children must restore the full syscall-visible user register image with `RAX=0`. Preserve `RDI`, `RSI`, `RDX`, `R8`, `R9`, `R10`, and callee-saved registers for compatibility syscall wrappers.
- Inline asm near user-register restore is release-codegen sensitive. If an asm operand is a pointer used for bookkeeping, consume it before restoring/clobbering user registers because LLVM may allocate it in `rax` or another soon-restored register.

## SMP And CPU-Local State

- Maintain per-CPU TSS/RSP0, syscall scratch, and SYSCALL MSR initialization. Do not reintroduce global syscall scratch or BSP-only user affinity as the long-term model.
- With the `x86_64` crate GDT, each TSS consumes two descriptor slots. Use one GDT per CPU with one TSS selector rather than packing all TSS descriptors into a shared GDT.

## Memory And ELF Loading

- OOM cleanup after `alloc_frames(n)` must release the whole contiguous run, not only pages that mapped successfully.
- If mappings were installed before failure, clear those PTEs before returning frames.
- Fork/COW failure paths must retain shared-page refs before exposing child mappings and destroy the child address space before returning an error.
- The validation binary writes a scratch window starting at `USER_BRK_BASE` before calling `brk()`. ELF load must map an initial heap window there and set `proc.brk` accordingly.

## Interrupt-Shared Locks And Output

- IRQ-shared input wait structures must only be touched with local interrupts disabled from thread context.
- SAIOS native serial output syscalls must not take `driver::serial::SERIAL` with interrupts enabled. Use the established serial print path or interrupt masking.

## Validation Binary Contracts

- Check the active validation binary/source before changing kernel behavior for `validate` failures.
- `validate` phase 6 uses raw Linux ABI `sys_write(0, buf, len)` and accepts success or exactly `-1`; fd-0 writes should return `-1` for that contract.
- Kernel panics render the red ring of death through the panic handler in `src/main.rs`; keep panic rendering no-allocation and fault-path safe.