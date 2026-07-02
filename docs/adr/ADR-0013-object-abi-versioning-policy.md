# ADR-0013: SNOM Object ABI Versioning and Compatibility Policy

- Status: Accepted
- Date: 2026-07-02
- Complements: ADR-0010, ADR-0011, ADR-0012

## Context

SNOM freezes the object ABI, but long-term stability requires explicit compatibility rules and version signaling.

Without a versioning policy, providers and tooling can drift and silently break interoperability.

## Decision

Adopt semantic object ABI versioning for SNOM with compatibility enforced by major and minor version semantics.

## Policy

- ABI tuple: major.minor.patch
- Compatibility rule: major must match, provider minor must be greater than or equal to required minor
- Patch does not break compatibility

## Stable ABI Types

The following remain compatibility-protected:

- ObjectHeader
- ObjectId
- PropertyValue
- Operation
- Relationship
- ProviderId
- Handle

## Change Rules

Allowed without major bump:

- Additive fields with safe defaults in extension structures
- New optional operations
- New optional properties
- New relationship kinds with backward-safe behavior

Require major bump:

- Removing fields from stable ABI types
- Changing existing field semantics
- Changing binary layout of stable ABI types
- Breaking operation invocation contract

## Runtime Requirements

- SNOM exposes current ABI version at runtime.
- Components may reject incompatible versions.
- Compatibility checks are mandatory for external provider registration.

## Related Documents

- [docs/SNOM.md](../SNOM.md)
- [docs/Architecture.md](../Architecture.md)
- [docs/adr/ADR-0012-snom-object-abi-and-sif-umbrella.md](ADR-0012-snom-object-abi-and-sif-umbrella.md)
