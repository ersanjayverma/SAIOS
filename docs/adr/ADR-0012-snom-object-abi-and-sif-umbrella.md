# ADR-0012: SNOM Object ABI and SIF Umbrella Architecture

- Status: Accepted
- Date: 2026-07-02
- Complements: ADR-0010, ADR-0011

## Context

SAIOS now has early SAIFS, object, provider, and query implementations. Before additional subsystem expansion, a stable native object ABI and architecture boundary is required to prevent long-term divergence.

## Decision

Adopt SNOM as the frozen native object model and define SIF as the architecture umbrella.

- SNOM is the canonical object ABI and reflection contract.
- Object Manager and Object Registry enforce SNOM identity and metadata.
- Provider Framework contributes objects into SNOM.
- SAIFS is a SIF component that provides namespace and filesystem views over SNOM objects.

## Architecture Roles

- SIF (umbrella): Object Manager, Provider Framework, Query Engine, Event Bus, Relationship Graph, SAIFS
- SNOM: object header, metadata, properties, operations, relationships, lifecycle/event semantics
- SAIFS: naming, mount, namespace and filesystem access over object handles

## Frozen ABI Set

The following ABI contracts are compatibility-protected:

- ObjectHeader
- ObjectId
- PropertyValue
- Operation
- Relationship
- ProviderId
- Handle

## Consequences

Positive:

- Stable long-lived object identity and contract boundaries
- Consistent reflection and query capabilities for all providers
- Clear separation between object truth and namespace representation
- Better support for observability and AI-assisted diagnostics

Trade-offs:

- Stronger schema governance and versioning discipline required
- Reduced freedom for subsystem-specific custom identity/event mechanisms

## Compliance Requirements

A subsystem is compliant when:

- It registers objects conforming to SNOM header and metadata contracts
- It emits standard object events for object and relationship changes
- It exposes operations and properties through SNOM contracts
- It does not treat path strings as internal object identity

## Related Documents

- [docs/SNOM.md](../SNOM.md)
- [docs/SOM.md](../SOM.md)
- [docs/Architecture.md](../Architecture.md)
- [docs/adr/ADR-0010-saifs-object-namespace-architecture.md](ADR-0010-saifs-object-namespace-architecture.md)
- [docs/adr/ADR-0011-som-foundational-object-model.md](ADR-0011-som-foundational-object-model.md)
