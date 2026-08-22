# SAIOS — Self-Aware Intelligence Operating System

> **An experimental x86_64 operating system built from first principles for observable, diagnosable computing.**

SAIOS is a systems-engineering project exploring what an operating system looks like when **observability, deterministic diagnostics, resource accounting, and failure explanation are architectural concerns rather than afterthoughts**.

It is intentionally ambitious: kernel, memory management, scheduling, processes, ELF execution, VFS, drivers, networking, Linux-compatible system calls, and a diagnostic/telemetry layer are developed together.

## Why SAIOS?

Traditional operating systems expose enormous amounts of low-level state, but diagnosing *why* a system failed often remains a manual exercise.

SAIOS experiments with a different model:

```text
                  ┌─────────────────────────────┐
                  │          Applications        │
                  └──────────────┬──────────────┘
                                 │
                  ┌──────────────▼──────────────┐
                  │       Linux-style ABI       │
                  │   processes / syscalls / FS │
                  └──────────────┬──────────────┘
                                 │
          ┌──────────────────────▼──────────────────────┐
          │                    Kernel                   │
          │  scheduler • memory • VFS • drivers • net │
          └──────────────┬─────────────────────────────┘
                         │
          ┌──────────────▼─────────────────────────────┐
          │       Observability / Diagnostic Layer     │
          │ events • telemetry • tracing • diagnosis  │
          └────────────────────────────────────────────┘
```

## Current engineering areas

- **x86_64 kernel** — boot, interrupts, paging and protected execution
- **Virtual memory** — page tables, address spaces, heap and memory protection
- **Process execution** — process lifecycle, fork/exec and ring-3 execution
- **ELF** — executable loading and user-space entry transfer
- **Scheduler** — task management, timers and wake/sleep paths
- **VFS** — virtual filesystem architecture and filesystem implementations
- **Storage** — block-device and filesystem work including ext4
- **Networking** — Ethernet, ARP, IPv4/IPv6, TCP, UDP, DNS and HTTP layers
- **Drivers** — hardware-facing subsystems developed inside the OS
- **Linux compatibility** — expanding syscall and userspace compatibility
- **Diagnostics** — kernel/system state designed to be inspectable and explainable
- **Shell** — native command-line environment for system bring-up and testing

The repository contains substantial experimental work, so capabilities and compatibility should be treated as **work in progress**, not production guarantees.

## Design principles

### 1. Explain failures

A crash should produce evidence, not just a fault code.

### 2. Account for resources

CPU time, memory, I/O and other resources should be observable and attributable.

### 3. Keep contracts explicit

Subsystem boundaries and invariants are documented so architectural drift can be detected early.

### 4. Build from first principles

Understand the machine before abstracting it away.

### 5. Compatibility is earned

Linux compatibility is approached incrementally through real programs and real failure paths rather than a compatibility checklist alone.

## Repository map

```text
seed/saios/     Kernel and system implementation
seed/           Boot/runtime project material
docs/           Architecture, milestones and engineering findings
```

The repository also contains historical engineering notes. They are retained because the failures and fixes are part of the project's technical record.

## Development status

SAIOS is an **active experimental operating-system project**.

The project has progressed through boot, memory, scheduling, process execution, filesystem, networking and userspace bring-up work, while several areas remain incomplete or under active development.

The most valuable parts of the repository are often the engineering investigations: page-fault analysis, address-space isolation, ELF loading, syscall compatibility, scheduler behavior and hardware bring-up.

## Build / run

SAIOS targets **x86_64** and is primarily developed using Rust tooling and virtualized/hardware test environments.

Because the build and boot pipeline evolves with the kernel, use the repository's current scripts and documentation as the source of truth for exact commands.

## Roadmap

- [ ] Broader Linux userspace compatibility
- [ ] Stronger process/thread/futex semantics
- [ ] More complete signal handling
- [ ] Mature dynamic linking support
- [ ] Expanded filesystem and storage support
- [ ] SMP scheduler hardening
- [ ] Deterministic boot/runtime validation
- [ ] Richer system diagnostics and flight recording
- [ ] Reproducible release images

## Engineering notes

SAIOS documents difficult failures instead of hiding them. Examples include:

- page-table and CR3 isolation failures
- user/kernel address-space assumptions
- ELF segment mapping and entry transfer
- syscall ABI edge cases
- scheduler ownership and wakeup bugs
- framebuffer and memory-mapping behavior

That record is deliberate: **the path to a reliable system is part of the system's engineering knowledge.**

## Contributing

The project is experimental and architecture-heavy. Contributions should come with a clear problem statement, evidence, tests where practical, and an explanation of the invariant being changed.

## License

See the repository's license and project documentation for the current terms.

---

**SAIOS**  
*Building an operating system that can explain itself.*

Part of the **Blackhatbadshah** engineering portfolio.
