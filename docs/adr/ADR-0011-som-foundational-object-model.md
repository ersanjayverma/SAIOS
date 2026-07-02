# ADR-0011: SOM Foundational Object Model

- Status: Accepted
- Date: 2026-07-02
- Supersedes: none
- Complements: ADR-0010

## Context

SAIFS and namespace architecture were defined in ADR-0010. The next risk is ambiguity in what an object is across kernel subsystems.

Without a strict and universal object model, identity, lifecycle, capabilities, and diagnostics can diverge by subsystem and create long-term architectural drift.

## Decision

Adopt SOM (SAIOS Object Model) as the mandatory pre-SAIFS foundation.

SOM defines:

- Universal object header
- Stable class taxonomy
- Capability model
- Property bag baseline
- Operation descriptors
- Relationship model
- Unified lifecycle
- Unified object event semantics
- AI-oriented introspection surface

## Immutable Rules

1. Everything is an object.
2. Every object has a stable ObjectId for its lifetime.
3. SAIFS exposes namespace views of objects and never owns object identity.
4. Objects advertise capabilities and operations instead of being identified only by concrete type.
5. Every object participates in health, diagnostics, events, and relationships by default.

## Consequences

Positive:

- Stable platform for storage, processes, devices, network, logs, and AI features
- Strong separation of identity from representation
- Consistent diagnostics and observability behavior across subsystems
- Reduced future refactoring cost for namespace and compatibility layers

Trade-offs:

- Up-front schema and lifecycle discipline required
- Migration needed for legacy subsystem-local identity patterns

## Compliance Checklist

A subsystem is SOM-compliant when:

- It defines objects with the universal object header fields
- It references peer objects by ObjectId
- It exposes baseline properties and operation IDs
- It emits standard object events for lifecycle and state changes
- It does not redefine lifecycle or health semantics independently

## Related Documents

- [docs/SOM.md](../SOM.md)
- [docs/SNOM.md](../SNOM.md)
- [docs/Architecture.md](../Architecture.md)
- [docs/adr/ADR-0010-saifs-object-namespace-architecture.md](ADR-0010-saifs-object-namespace-architecture.md)
- [docs/adr/ADR-0012-snom-object-abi-and-sif-umbrella.md](ADR-0012-snom-object-abi-and-sif-umbrella.md)
