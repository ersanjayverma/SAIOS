# ADR-0015: SAIOS 0.2 Foundation Freeze and Validation Discipline

- Status: Accepted
- Date: 2026-07-02
- Complements: ADR-0010, ADR-0011, ADR-0012, ADR-0013, ADR-0014

## Context

SAIOS architecture has expanded quickly. Reliability confidence must now catch up with feature growth.

Without a freeze and strict validation discipline, architecture drift and regressions will compound.

## Decision

Adopt SAIOS 0.2 Foundation as a stability-first milestone.

Freeze kernel core around:

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

Introduce and require:

- KTF for subsystem test registration and execution
- KVF for runtime invariants and live state checks
- BST for debug boot readiness checks

## Policy

A subsystem is not complete until it has all of the following:

1. implementation
2. tests
3. verify routine
4. health reporting
5. self-test registration

## Consequences

Positive:

- Earlier defect detection
- Lower integration risk
- More predictable system behavior under change

Trade-offs:

- More up-front engineering effort per feature
- Slightly slower feature throughput in the short term

## Related Documents

- [docs/SAIOS-0.2-Foundation.md](../SAIOS-0.2-Foundation.md)
- [docs/KernelArchitecture.md](../KernelArchitecture.md)
- [docs/Architecture.md](../Architecture.md)
