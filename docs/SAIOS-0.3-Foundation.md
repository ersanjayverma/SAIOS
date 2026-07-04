# SAIOS 0.3 Foundation

Status: Active milestone
Owner: Kernel correctness and reliability
Last updated: 2026-07-05

## Recent Findings (2026-07-05)

Real-hardware bring-up produced additional correctness findings relevant to v0.3 gates:

1. Late CR3 preflight must validate full descriptor-table ranges, not just bases.
	- Checking only `idt_base`/`gdt_base` can miss cross-page gaps.
	- Preflight now verifies IDT/GDT base plus end (`base + limit`) before switch.

2. Packed descriptor-pointer access in low-level x86 paths is fragile.
	- `sidt/sgdt/lgdt/lidt` handling was hardened by using raw 10-byte buffers with explicit decode/encode.
	- This avoids alignment-sensitive packed-field access in critical descriptor-table paths.

3. Prompt-time input recovery loops can fault in fallback-mode runtime.
	- A prompt-time panic was observed with page fault `cr2=0xffff800000001014` after introducing aggressive runtime recovery logic.
	- Recovery loops were removed from `poll_input`; fallback-safe init path is preferred.

4. Fallback-mode input policy is stability-first.
	- PS/2 remains primary in fallback mode.
	- USB HID probing/rescan loops are not run continuously from the shell poll path in fallback mode.

5. Shell startup should not erase triage context before first interaction.
	- Automatic startup `clear` was removed so operators can retain visible boot/input diagnostics.

6. Foreground storage scan remains required and validated.
	- Storage discovery runs synchronously in boot service startup and reports completion status deterministically.

## Milestone Goal

Stabilize kernel correctness primitives before user-space expansion.

This milestone treats interrupting, timing, fault containment, I/O streams, and core filesystem operations as non-optional readiness gates. New feature work for ELF/Linux ABI/network/filesystem expansion should wait until these gates are green.

## Single-Core Execution Policy (Current)

SAIOS currently runs in a single-core bring-up posture. Until SMP scheduling is fully enabled and validated, kernel paths must not depend on background workers for critical correctness behavior.

Rules:

- Critical kernel scans and initialization paths must be deterministic and foreground-safe.
- Storage discovery and publication must complete in the caller thread.
- User-facing commands must not rely on deferred worker completion to report correctness status.
- Concurrency assumptions must be explicit and feature-gated before enabling background jobs.

Current application:

- Storage rescan executes synchronously.
- Diagnostics and object publication are completed before command return.

## Exit Criteria (Required)

All items below must be functionally complete and pass boot-time validation.

- Interrupts: 100%
- PIT/APIC timer delivery: 100%
- Tick accounting: 100%
- Sleep/Wakeup semantics: 100%
- Timer scheduler wake path: 100%
- Invalid pointer handling: 100%
- stderr stream separation: 100%
- Surface allocation lifecycle: 100%
- VFS rename: 100%
- VFS move semantics: 100%
- Keyboard input path: 100%
- Mouse input path: required for GUI readiness

## Recommended Implementation Order

### Phase 1: Interrupt Infrastructure

Scope:

- Complete IDT coverage
- Exception handlers with correct stack-frame handling
- IRQ dispatch and ownership boundaries
- PIC/APIC acknowledge correctness
- Nested interrupt safety

Minimum tests:

- interrupt delivery smoke
- divide by zero
- invalid opcode
- page fault
- timer irq
- keyboard irq

Exit:

- 1000 timer interrupts observed
- 0 lost interrupts
- 0 double faults

### Phase 2: Timer

Scope:

- ticks
- uptime
- monotonic clock contract

Required behavior:

- timer interrupt path increments global tick counter exactly once per delivered tick

Minimum tests:

- ticks increase monotonically
- uptime grows consistently with ticks

### Phase 3: Sleep and Wake

Scope:

- sleep(ms)
- sleep(ticks)
- yield_until_tick()
- ordered sleeping queue by wake_tick

Timer IRQ behavior:

- current_tick increments
- all sleepers with wake_tick less than or equal to current_tick are transitioned to ready

Exit:

- sleep(100) returns after 100 ticks
- 100 sleeping threads wake correctly

### Phase 4: Timer Scheduler

Scope:

- replace busy-wait sleep with scheduler-mediated blocking
- wake by timer IRQ into ready queue
- idle CPU uses hlt when no runnable threads exist

Exit:

- no spin-loop based sleep in core scheduler timing path
- idle loops use hlt consistently

### Phase 5: Invalid Pointer Handling

Scope:

- robust page-fault handler
- fault policy split by address-space ownership

Policy:

- invalid user pointer: terminate current process (SIGSEGV-equivalent policy)
- invalid kernel pointer: panic with diagnostics
- no reboot and no triple fault on user-memory faults

Exit:

- null and canonical-invalid user accesses are contained
- kernel remains alive after user-space fault termination

### Phase 6: stderr

Scope:

- distinct stdout and stderr channels
- foundation for fd 0, fd 1, fd 2 semantics

Exit:

- stderr path is separately routable from stdout
- shell-visible diagnostics can target stderr explicitly

### Phase 7: Surface Allocation

Scope:

- surface manager with allocate/destroy/resize
- reference-counted ownership
- safety checks for double free, invalid free, and out-of-memory

Exit:

- surface lifecycle tests pass under normal and error paths

### Phase 8: Rename and Move

Scope:

- VFS rename()
- VFS move semantics across directories
- explicit replacement policy when destination exists
- forward-compatible behavior for renameat/renameat2 style semantics

Exit:

- rename and move work for files and directories across directory boundaries

### Phase 9: Keyboard

Scope:

- keyboard IRQ path
- PS/2 decode to key events
- input queue to shell
- modifiers: shift, ctrl, alt, caps, repeat

Exit:

- interactive shell input remains reliable under sustained key traffic

### Phase 10: Mouse

Scope:

- IRQ12 handling
- packet decoder
- cursor state and button state
- wheel support staged after 3-byte baseline

Exit:

- stable pointer movement and button state reporting

## Required Boot Regression Gates

Every boot must run and report these checks:

- PASS interrupt delivery
- PASS timer ticks
- PASS scheduler sleep
- PASS wake queue
- PASS page fault
- PASS invalid pointer handling
- PASS stderr
- PASS rename
- PASS move
- PASS keyboard
- PASS mouse
- PASS storage controller scan timeout behavior
- PASS storage scan graceful degradation (reports 0 disks, no stall)
- PASS object namespace publication for discovered storage devices

If any required gate fails, kernel status must be reported as not ready and the system must not claim healthy runtime readiness.

## Kernel Readiness Contract

Expected operator-facing summary format:

- Interrupts: PASS or FAIL
- Timer: PASS or FAIL
- Sleep: PASS or FAIL
- Wake: PASS or FAIL
- Page Fault: PASS or FAIL
- Invalid Pointer: PASS or FAIL
- stderr: PASS or FAIL
- Rename: PASS or FAIL
- Move: PASS or FAIL
- Keyboard: PASS or FAIL
- Mouse: PASS or FAIL
- Storage Scan: PASS or FAIL
- Storage Objects: PASS or FAIL
- Kernel READY or Kernel NOT READY

Rule:

- Kernel READY is valid only if all required gates pass.

## Definition of Done for v0.3

A v0.3 kernel is done when it can:

- boot reliably on real hardware and virtual machines
- handle CPU exceptions and hardware interrupts without hangs or triple faults
- maintain accurate monotonic tick accounting
- sleep and wake threads at correct ticks without busy-wait
- terminate invalid user-memory accesses safely while preserving kernel stability
- provide distinct stdin/stdout/stderr foundations
- allocate, resize, and free rendering surfaces safely
- support file and directory rename/move through VFS
- accept reliable keyboard and functional mouse input
- pass automated kernel self-tests for all required gates on every boot
- complete storage scans without shell stalls in single-core mode
- publish discovered storage devices consistently under object namespace views

## Out of Scope for v0.3

The following are intentionally deferred until correctness gates are stable:

- Linux ABI completeness
- broad ELF user-space expansion
- advanced networking feature set
- broad filesystem feature expansion beyond required move/rename correctness
