# SAIOS - Self-Aware Intelligence Operating System

Runtime version: `v0.9.9`

SAIOS is an experimental Rust `no_std` x86 operating-system kernel built around a constitutional idea: the operating system should not only execute software, it should understand and explain its own behavior. Its source of truth is the documentation under [`docs/core`](docs/core), especially [`SAIOS_SSOT.md`](docs/core/SAIOS_SSOT.md), [`SAIOS_SSOT_Part2.md`](docs/core/SAIOS_SSOT_Part2.md), and the implementation specifications in [`docs/core/Implementation`](docs/core/Implementation).

The governing principle is: Failure leads to Understanding leads to Resolution.

## What SAIOS Is

SAIOS is designed as an Intelligence Operating System. Execution remains essential, but observability, diagnostics, correlation, prediction, accountability, and explanation are first-class system capabilities rather than external tools layered on afterward.

The long-term platform goal is to understand:

- what happened
- why it happened
- what is affected
- what may happen next
- what action is recommended
- which evidence justifies the conclusion

## Constitutional Architecture

The kernel is organized around contract-owned subsystems. Each contract owns specific state and invariants; other subsystems must interact through the owning contract rather than mutating canonical state directly.

Core constitutional areas include:

- [`DOC-01_SAIOS_Kernel_Constitution.md`](docs/core/Implementation/DOC-01_SAIOS_Kernel_Constitution.md) - kernel invariants and operating law
- [`DOC-04_Boot_Sequence_Specification.md`](docs/core/Implementation/DOC-04_Boot_Sequence_Specification.md) - sixteen boot validation gates
- [`DOC-05_ExecutionContract.md`](docs/core/Implementation/DOC-05_ExecutionContract.md) - CPU current state, kernel stacks, CR3, GS/TLS boundaries
- [`DOC-06_MemoryContract_Virtual_Memory.md`](docs/core/Implementation/DOC-06_MemoryContract_Virtual_Memory.md) - frame ownership, address spaces, COW, VM behavior
- [`DOC-07_ProcessContract.md`](docs/core/Implementation/DOC-07_ProcessContract.md) - process lifecycle, PID ownership, zombie/dead transitions
- [`DOC-08_SchedulerContract.md`](docs/core/Implementation/DOC-08_SchedulerContract.md) - run queues, CPU assignment, blocking, wakeup, scheduler invariants
- [`DOC-10_KDS_ObservabilityContract.md`](docs/core/Implementation/DOC-10_KDS_ObservabilityContract.md) - Knowledge Data Store and observability substrate
- [`DOC-15_SecurityContract.md`](docs/core/Implementation/DOC-15_SecurityContract.md) - capabilities, MAC enforcement, namespace boundaries
- [`DOC-16_ReliabilityContract_Red_Ring.md`](docs/core/Implementation/DOC-16_ReliabilityContract_Red_Ring.md) - lock order validation, contract violations, Red Ring halt
- [`DOC-17_SAIRU_Intelligence_Architecture.md`](docs/core/Implementation/DOC-17_SAIRU_Intelligence_Architecture.md) - deterministic SAIRU intelligence architecture
- [`DOC-18_Compatibility_Roadmap_Bootstrapping.md`](docs/core/Implementation/DOC-18_Compatibility_Roadmap_Bootstrapping.md) - phased compatibility and bootstrapping roadmap

## Core Concepts

### Knowledge Data Store

The Knowledge Data Store, or KDS, is the evidence substrate for SAIOS. It records structured system events so observability, diagnostics, Red Ring analysis, Flight Recorder output, and SAIRU intelligence have a common source of truth.

### Red Ring

The Red Ring is a controlled halt for maximum evidence preservation. It is not a recovery path. It exists so contract violations, panics, unrecoverable faults, and critical invariant failures halt the system with preserved context and explainable evidence.

### SAIRU

SAIRU is the SAI Runtime intelligence surface. Phase One is deterministic and must work without an AI model. Its documented architecture has seven engines: Context, Tool, Skill, Task, Knowledge, Planning, and Policy. SAIRU consumes KDS evidence and contract APIs; it does not own canonical kernel state or bypass subsystem authority.

### Resource Accounting

The Resource Accounting Framework is a constitutional pillar. Every consumed resource should be attributed to an accountable entity, and accounting failures must be visible through evidence rather than hidden.

## Project Layout

- [`src`](src) - kernel, contracts, architecture modules, drivers, memory, process, syscall, VFS, SAIRU, and userspace support surfaces
- [`docs/core`](docs/core) - authoritative SAIOS constitution and implementation specifications
- [`docs/plan`](docs/plan) - planning, audits, and backlog source material derived from the core documents
- [`docs/status`](docs/status) - live remediation dashboard and closeout records
- [`userspace`](userspace) - userspace test and validation programs
- [`tests`](tests) - validation scripts and test support
- [`uefi_stub`](uefi_stub) - UEFI boot support crate

## Building And Checking

SAIOS uses the Rust nightly toolchain declared in [`rust-toolchain.toml`](rust-toolchain.toml). The kernel target check should be run with explicit target and build-std flags:

For host-side static compatibility and contract-source audits that do not boot QEMU or run runtime validation:

```powershell
python .\tests\static_audit_suite.py
```

For the kernel target compile gate:

```powershell
cargo check --target x86_64-unknown-none -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem
```

For release/codegen-sensitive paths:

```powershell
cargo build --release --target x86_64-unknown-none -Z build-std=core,compiler_builtins,alloc -Z build-std-features=compiler-builtins-mem
```

Do not add a global `[build].target` or global `build-std` configuration to `.cargo/config.toml`; host-side Cargo commands must remain usable.

## Documentation Authority

When implementation, planning, or CI metadata disagrees with the project constitution, prefer this order:

1. [`docs/core`](docs/core)
2. [`docs/plan`](docs/plan)
3. [`docs/status`](docs/status)
4. source code

The files under `docs/core` are the reference point for architectural intent. Planning and status documents should track remediation against that authority, not redefine it.

## Current Scope

The repository is an active kernel implementation and constitutional remediation workspace. Some systems are mature enough to validate with `cargo check`; others are still architectural scaffolding moving toward the requirements in `docs/core`. Runtime behavior should be evaluated against the freshest boot artifacts and serial logs when runtime debugging is explicitly in scope.

## Runtime Status Mapping

Runtime boot governance is expressed as the sixteen validation gates from [`DOC-04_Boot_Sequence_Specification.md`](docs/core/Implementation/DOC-04_Boot_Sequence_Specification.md). Serial boot `segment` lines are implementation progress markers inside those gates; they are not roadmap phases.

Compatibility maturity is expressed through the phases in [`DOC-18_Compatibility_Roadmap_Bootstrapping.md`](docs/core/Implementation/DOC-18_Compatibility_Roadmap_Bootstrapping.md). The current source-owned status is Phase 3: Linux syscall ABI compatibility in progress.

## License

See [`LICENSE`](LICENSE).
