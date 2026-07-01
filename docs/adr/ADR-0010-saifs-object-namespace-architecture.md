# ADR-0010: SAIFS Object and Namespace Architecture

- Status: Accepted
- Date: 2026-07-02
- Decision drivers: long-term extensibility, self-aware diagnostics, unified object model, compatibility layering

## Context

SAIOS is transitioning from feature-first subsystem growth to architecture-first contracts. Current and future subsystems include storage, devices, drivers, processes, memory, network, logs, and AI-facing capabilities.

The platform needs stable rules that avoid coupling object identity to path representations or filesystem implementation details.

## Decision

Adopt SAIFS as a platform architecture with Object Manager as identity source of truth and provider-based namespace composition.

### Constitutional Rules

1. Objects are the source of truth.
2. Paths are namespace views over objects.
3. Handles, not paths, are used internally after lookup.
4. Providers own namespaces and expose objects.
5. Object Manager owns identity and lifecycle registry.
6. SAIFS owns naming, routing, and access flow.
7. All core kernel subsystems register as providers and objects.

## Architecture Shape

- Applications -> Shell or APIs -> SAIFS API
- SAIFS API -> Object Manager + Namespace Manager + Mount Manager
- Namespace and mount routing -> Provider Registry
- Providers -> Storage, Processes, Devices, Drivers, Memory, Network, Logs, Services, AI

## Consequences

Positive:

- Stable object identity across renames and remounts
- Multi-view namespaces over the same object
- Extensible operation model without SAIFS core rewrites
- Better support for explain, diagnose, health, and observability
- Clear path for POSIX compatibility adapters

Trade-offs:

- Additional up-front contract design complexity
- Requires disciplined provider registration and event publication
- Migration effort for existing direct-path code paths

## Rejected Alternatives

- Everything is a file as primary architecture
- Everything is a device as primary architecture
- Single VFS-centric authority for identity and namespace

These alternatives were rejected because they make identity, diagnostics, and future capability dispatch less explicit and less extensible for SAIOS goals.

## Compliance Checklist

A subsystem is SAIFS-compliant when:

- It registers objects with Object Manager
- It exposes a provider contract where applicable
- It supports handle-based internal operation paths
- It emits lifecycle and state events to the observer system
- It does not invent parallel identity types

## Migration Notes

- Existing VFS behavior is treated as an adapter layer.
- Existing object inspection commands remain supported.
- New features must target SAIFS contracts first, then adapters.

## Related Documents

- [docs/Architecture.md](../Architecture.md)
- [docs/SOM.md](../SOM.md)
- [docs/adr/ADR-0011-som-foundational-object-model.md](ADR-0011-som-foundational-object-model.md)
