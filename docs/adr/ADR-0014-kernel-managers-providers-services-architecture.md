# ADR-0014: Kernel Managers, Providers, Services Architecture

- Status: Accepted
- Date: 2026-07-02
- Complements: ADR-0010, ADR-0011, ADR-0012, ADR-0013

## Context

SAIOS now has object and information-framework contracts. The missing architectural freeze is a kernel-wide layering model that all subsystems must obey.

Without this, subsystems can drift into cyclic dependencies and inconsistent ownership/lifecycle behavior.

## Decision

Adopt a kernel architecture split into Managers, Providers, Services, and HAL with downward-only dependencies.

## Core Rules

- Managers own state.
- Providers expose manager-owned objects and data.
- Services perform work but never own objects.
- HAL is the only hardware-facing layer.
- Dependencies flow downward only.

## Frozen Boot Order

Firmware -> Bootloader -> HAL -> MemoryManager -> ObjectManager -> ProviderRegistry -> SIF -> SAIFS -> Scheduler -> DeviceManager -> Drivers -> Services -> Shell -> User Space

## Execution Context Rule

Every public kernel API must declare supported execution contexts.

## Lifecycle Rule

All resources follow a unified lifecycle from Create to Destroyed, enforced by managers.

## Constitution

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

## Consequences

Positive:

- Stable layering for long-term subsystem growth
- Fewer architectural cycles and ownership ambiguity
- Predictable integration path for new providers and services

Trade-offs:

- Additional design discipline for subsystem APIs
- More explicit initialization and context-validation requirements

## Related Documents

- [docs/KernelArchitecture.md](../KernelArchitecture.md)
- [docs/Architecture.md](../Architecture.md)
- [docs/SNOM.md](../SNOM.md)
- [docs/adr/ADR-0012-snom-object-abi-and-sif-umbrella.md](ADR-0012-snom-object-abi-and-sif-umbrella.md)
