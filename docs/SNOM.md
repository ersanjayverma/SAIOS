# SAIOS Native Object Model (SNOM)

Status: Frozen foundational contract
Owner: Kernel architecture
Last updated: 2026-07-02

## Purpose

SNOM defines what an object is in SAIOS, independent of filesystem or namespace representation.

SNOM is the native object ABI and reflection surface for providers, SAIFS, query, events, diagnostics, and future AI integration.

## Position in Architecture

Object Manager
-> Object Registry
-> SNOM
-> Provider Framework
-> SAIFS

SNOM is below the Object Registry and above providers.

## Core Interface

All managed kernel entities implement the same object contract.

```rust
pub trait SaiObject {
    fn header(&self) -> &ObjectHeader;
    fn metadata(&self) -> &Metadata;
    fn properties(&self) -> PropertyStore;
    fn operations(&self) -> OperationTable;
    fn relationships(&self) -> RelationshipTable;
}
```

No subsystem-specific exception path is allowed.

## Metadata Model

Metadata is immutable or kernel-managed.

Mandatory metadata fields:

- ObjectId
- ProviderId
- Created
- Modified
- Version
- Class
- State
- Health
- Flags

User-space and provider code must not mutate metadata directly.

## Property Model

Properties are typed and extensible.

Examples:

- cpu.frequency
- memory.used
- driver.version
- network.ip
- storage.serial

Property value contract:

```rust
enum PropertyValue {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Object(ObjectId),
    Duration(Duration),
}
```

## Operation Model

Operations are discoverable and invokable by ID.

Flow:

- Enumerate supported operations
- Validate capability and policy
- Invoke by OperationId

Example:

```rust
object.invoke(OperationId::Reset)?;
```

## Relationship Graph

Relationships are first-class SNOM data.

Examples:

- Driver manages Device
- Device emits Interrupt
- Filesystem mounted_on Volume
- Process owns Thread
- Thread uses Socket

Relationship queries and traversal are part of discovery, not ad hoc subsystem logic.

## Query Model

Object discovery is query-first, not path-only.

Canonical query examples:

- kind=Driver
- health=Warning
- provider=Storage
- parent=Process:42
- relationship=depends_on

## Transaction Model

Object changes are transactional.

Lifecycle:

- Begin
- Modify
- Commit

or

- Begin
- Modify
- Rollback

Transactions are required for safe configuration, auditing, and future distributed providers.

## Event Model

All object changes emit standardized events.

Mandatory event kinds:

- ObjectCreated
- ObjectDestroyed
- PropertyChanged
- RelationshipAdded
- RelationshipRemoved
- OperationExecuted
- HealthChanged

Providers must not define private event buses for object lifecycle/state changes.

## Reflection

Every object can self-describe.

Describe returns:

- Properties
- Operations
- Capabilities
- Relationships

Reflection is required for CLI inspectors, GUI tooling, remote management, and AI reasoning.

## Frozen Object ABI

The following ABI types are stable and compatibility-protected:

- ObjectHeader
- ObjectId
- PropertyValue
- Operation
- Relationship
- ProviderId
- Handle

These types are versioned and backward-compatible across releases.

Versioning policy is defined in [docs/adr/ADR-0013-object-abi-versioning-policy.md](adr/ADR-0013-object-abi-versioning-policy.md).

## Architectural Rules

1. Everything is an object.
2. ObjectId is stable for object lifetime.
3. SAIFS exposes object namespace views and never owns identity.
4. Objects advertise capabilities and operations.
5. Health, diagnostics, events, and relationships are default participation requirements.

## Roadmap Dependency Order

SNOM (Object ABI)
-> Object Manager
-> Provider Registry
-> SAIFS
-> Event Bus
-> Query Engine
-> Native SAIOS API (libsai)
-> POSIX Compatibility Layer

## SIF Naming Model

SIF (SAIOS Information Framework) is the umbrella information architecture.

SIF contains:

- Object Manager
- Provider Framework
- Query Engine
- Event Bus
- Relationship Graph
- SAIFS (namespace and filesystem)

SAIFS is a component of SIF, not the umbrella itself.

Code references:

- SNOM contracts: [seed/saios/src/snom.rs](../seed/saios/src/snom.rs)
- SIF facade: [seed/saios/src/sif.rs](../seed/saios/src/sif.rs)
- Kernel architecture contracts: [seed/saios/src/kernel_arch.rs](../seed/saios/src/kernel_arch.rs)
