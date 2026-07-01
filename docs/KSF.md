# KSF: Kernel Service Framework

Status: Implemented baseline
Owner: Kernel architecture and runtime
Last updated: 2026-07-02

## Purpose

KSF provides one control plane for kernel service lifecycle.

It standardizes:

- service identity and metadata
- lifecycle states
- dependency-aware startup
- runtime service control surface for SNSH

KSF replaces ad hoc direct subsystem startup sequencing in early kernel paths.

## Scope

KSF governs service lifecycle only.

It does not replace manager ownership rules:

- managers still own objects and state
- providers still expose views over manager-owned objects
- services still operate over contracts and do not own objects

## Core Contract

Implemented service contract:

```rust
pub trait KernelService {
    fn id(&self) -> ServiceId;
    fn name(&self) -> &'static str;
    fn dependencies(&self) -> &'static [ServiceId];
    fn initialize(&mut self) -> Result<(), &'static str>;
    fn start(&mut self) -> Result<(), &'static str>;
    fn stop(&mut self);
    fn health(&self) -> HealthState;
}
```

Identity and state:

```rust
pub struct ServiceId(pub u16);

pub enum ServiceState {
    Registered,
    Initializing,
    Ready,
    Running,
    Paused,
    Stopping,
    Stopped,
    Failed,
}
```

## Startup Model

KSF startup uses dependency-aware progression:

1. Register all service adapters.
2. Repeatedly scan for services whose dependencies are in Ready or Running state.
3. Initialize and start eligible services.
4. Continue until all services are Running, or fail with unresolved/failed services.

This produces deterministic startup without hardcoding a single imperative init chain in multiple kernel locations.

## Built-in Services (Current)

Registered in bootstrap order:

- console
- memory
- object
- provider
- sif
- timer
- scheduler
- event
- health
- shell

Dependencies enforce startup constraints, for example:

- scheduler depends on timer
- sif depends on provider
- shell depends on console, sif, scheduler

## Boot Integration

Current boot path initializes PMM and heap, then transfers service bring-up to KSF bootstrap.

This establishes a single service orchestration point before entering scheduler and shell execution flow.

Code references:

- [seed/saios/src/ksf.rs](../seed/saios/src/ksf.rs)
- [seed/saios/src/main.rs](../seed/saios/src/main.rs)

## Runtime Operations

KSF exports runtime control APIs:

- list
- start(name)
- stop(name)
- restart(name)
- health
- info(name)

These are surfaced through SNSH native command family.

## SNSH Interface

Native shell command:

- service list
- service start <name>
- service stop <name>
- service restart <name>
- service health
- service info <name>

This enables operators to inspect lifecycle and health, and to control service state without manager-private access.

## Architectural Guarantees

- Service lifecycle is explicit and queryable.
- Dependency sequencing is centralized.
- Shell and boot paths use the same service runtime surface.
- Service ownership boundaries remain aligned with KernelArchitecture and ADR-0014.

## Current Limitations

- Memory service currently documents PMM/heap as pre-KSF bootstrap prerequisites.
- Service graph cycle diagnostics are currently coarse (failed start_all result).
- Stop/restart dependency safety is currently shallow and service-local.

These are expected baseline constraints for the current implemented phase.
