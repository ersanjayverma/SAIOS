# SAIOS Kernel Architecture

Status: Frozen architectural contract
Owner: Kernel architecture
Last updated: 2026-07-02

## Purpose

This document defines the kernel-level architecture boundaries and dependency laws for SAIOS.

It formalizes how Managers, Providers, Services, and HAL interact, and freezes boot sequencing, execution context rules, and lifecycle semantics.

## Structural Model

```text
                        SAIOS Kernel
                              |
    +--------------+----------+----------+--------------+
    |              |                     |              |
 Managers      Providers             Services        HAL
```

## Kernel Core (0.2 Freeze)

Frozen kernel core subsystems for SAIOS 0.2:

- HAL
- Memory Manager
- Object Manager
- Service Manager
- Provider Manager
- Event Bus
- Query Engine
- SAIFS
- Scheduler
- SNSH

All new subsystem work must integrate through these contracts.

## 1. Managers (Own State)

Managers own resources and enforce lifecycle and integrity for their domain.

Reference manager domains:

- MemoryManager
- ObjectManager
- ProcessManager
- ThreadManager
- SchedulerManager
- DeviceManager
- DriverManager
- SecurityManager
- StorageManager
- NetworkManager
- ServiceManager

Rule:

- Exactly one manager owns each object.

## 2. Providers (Expose Data)

Providers are adapters over manager-owned data.

Rules:

- Providers do not own objects.
- Providers contribute namespaces and discovery surfaces into SIF and SAIFS.
- Providers do not depend on each other.

## 3. Services (Kernel Facilities)

Services perform work over objects and events.

Service lifecycle orchestration is owned by KSF (Kernel Service Framework).

KSF responsibilities:

- register services
- resolve dependency-aware startup order
- track service lifecycle states
- expose runtime service control APIs for shell and tooling

Reference services:

- Logger
- EventBus
- QueryEngine
- HealthEngine
- MetricsEngine
- SecurityEngine
- NotificationEngine
- AIEngine

Rules:

- Services never own objects.
- Services consume manager/provider contracts.

## 4. HAL

HAL is the only hardware-facing layer.

Reference HAL domains:

- CPU
- Interrupt
- Timer
- Display
- Storage
- PCI
- USB
- Power

Rule:

- Hardware-specific logic must not cross above HAL boundaries.

## Dependency Law

Dependencies point downward only.

```text
Applications
-> Shell
-> SIF
-> Providers
-> Managers
-> HAL
-> Hardware
```

Forbidden examples:

- Manager depending on Shell
- HAL depending on ObjectManager
- Provider depending on Provider

## Boot Sequence (Frozen)

```text
Firmware
-> Bootloader
-> HAL
-> MemoryManager
-> ObjectManager
-> ProviderRegistry
-> SIF
-> SAIFS
-> KSF (ServiceManager + service startup)
-> Scheduler Service Running
-> Input Service Running
-> Shell Service Thread Spawned
-> DeviceManager
-> Drivers
-> Services Running
-> Idle Runtime
-> User Space

Shell placement rule:

- Shell is a KSF-managed service and scheduled task.
- Boot code must not enter shell loops directly.

## Memory Manager Status (Implemented)

Current memory stack is split into PMM and VMM responsibilities:

- PMM (`pmm` over `driver/memory`) owns physical-page accounting and allocation.
- VMM (`vmm`) owns virtual mappings and hardware page-table updates.

Implemented PMM capabilities:

- 4 KiB page allocation (`alloc_page`, `alloc_pages`)
- single-page and range free helpers
- reserve API passthrough for physical ranges
- page and byte accounting (`total/free/used`)

Implemented VMM capabilities:

- kernel VMM initialization from boot PML4
- map / unmap / translate contracts
- owned mappings with physical release on unmap
- higher-half kernel mapping policy
- hardware-backed page-table operations through recursive mapping
- per-page TLB invalidation on map/unmap

Operational note:

- VMM no longer acts as metadata-only bookkeeping; map/unmap now writes live page tables.

## Process and Loader Status (Implemented Slice)

Current process/runtime slice includes:

- process lifecycle APIs: `spawn`, `exec`, `exit`, `wait`
- executable resolution via explicit path and `/bin/<name>` search
- binary metadata-based loader entry selection
- PIE metadata support with load bias computation
- dynamic-link metadata support with interpreter, needed libraries, and symbol resolution checks

Package image now seeds executable and shared-object metadata required for this path.
```

## Execution Context Model

Allowed execution contexts must be explicit in API contracts.

Contexts:

- Boot Context
- Interrupt Context
- Scheduler Context
- Worker Context
- User Context

Example policy shape:

- Memory allocation: Boot, Scheduler, Worker allowed
- Memory allocation: Interrupt disallowed

## Resource Lifecycle (Unified)

All resources share one lifecycle contract:

- Create
- Initialize
- Register
- Active
- Suspended
- Stopping
- Destroyed

Managers enforce lifecycle transitions.

## Global Kernel Facilities

These services are baseline and should not be duplicated by subsystems:

- Clock
- Random
- Logger
- EventBus
- Metrics
- Health
- Configuration

## Kernel Constitution

1. Everything is a kernel object.
2. Every object is owned by exactly one manager.
3. Providers expose objects but never own them.
4. Services operate on objects but never own them.
5. Hardware is accessed only through the HAL.
6. Dependencies flow downward only.
7. Every object participates in events, health, metrics, and discovery.
8. Every public kernel API declares its execution context.
9. Every resource follows the same lifecycle.
10. Identity, storage, and representation are separate concerns.

## Relationship to SIF and SNOM

- SNOM defines native object ABI and reflection contract.
- SIF defines information infrastructure.
- SAIFS provides namespace/filesystem services within SIF.
- Kernel architecture in this document governs all of them.

## Code Contract Reference

The corresponding code-level contract module is:

- [seed/saios/src/kernel_arch.rs](../seed/saios/src/kernel_arch.rs)
- [seed/saios/src/ksf.rs](../seed/saios/src/ksf.rs)

Related architecture document:

- [docs/KSF.md](KSF.md)
- [docs/SAIOS-0.2-Foundation.md](SAIOS-0.2-Foundation.md)
- [docs/adr/ADR-0015-saios-0.2-foundation-freeze-and-validation.md](adr/ADR-0015-saios-0.2-foundation-freeze-and-validation.md)
