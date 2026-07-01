# SAIOS Architecture

Status: Draft for freeze candidate
Owner: Kernel architecture
Last updated: 2026-07-02

## Purpose

This document defines the architecture contracts for SAIOS Abstract Information and File System (SAIFS) anchored on the SAIOS Object Model (SOM).

This is a contract-first document. It defines what must be true before additional subsystem features are added.

See [docs/SOM.md](SOM.md) for the SOM baseline object contract and [docs/SNOM.md](SNOM.md) for the frozen native object ABI and reflection contract.

SNOM ABI version compatibility policy: [docs/adr/ADR-0013-object-abi-versioning-policy.md](adr/ADR-0013-object-abi-versioning-policy.md).

Kernel architecture freeze: [docs/KernelArchitecture.md](KernelArchitecture.md).
Kernel layering ADR: [docs/adr/ADR-0014-kernel-managers-providers-services-architecture.md](adr/ADR-0014-kernel-managers-providers-services-architecture.md).
SNSH shell architecture: [docs/SNSH.md](SNSH.md).

## Framework Naming

SIF (SAIOS Information Framework) is the architecture umbrella.

SIF includes:

- Object Manager
- Provider Framework
- Query Engine
- Event Bus
- Relationship Graph
- SAIFS (namespace and filesystem service)

SAIFS is a component of SIF, not the umbrella itself.

## Foundational Rule

Everything is an object. Some objects are persistent. Some objects are exposed through a filesystem.

Corollary:

- Objects are identity and truth.
- Paths are representations of objects.
- Internal subsystem operations use handles, not paths.

## Core Components

- SOM: universal object header, taxonomy, capabilities, operations, relationships, lifecycle, and event semantics.
- SNOM: frozen object ABI, metadata model, operation model, relationship model, and reflection contracts.
- Object Manager: global object identity, registry, lifecycle, metadata baseline.
- Provider Framework: subsystem discovery and object contribution through provider contracts.
- Query Engine: first-class object discovery service over kind, health, provider, and graph context.
- SAIFS API: open and operate on handles, not paths.
- Namespace Manager: namespace view composition and lookup.
- Mount Manager: mount-point routing to providers.
- Provider Registry: provider registration and discovery.
- Namespace Providers: source of objects and namespace entries.
- Observer System: event publication and subscription.

## Phase 0: SOM Foundation Contract

SOM is a prerequisite to SAIFS. SAIFS and Object Manager must use SOM object identity and object contract semantics.

Frozen principles:

1. Everything is an object.
2. Every object has a stable ObjectId for its lifetime.
3. SAIFS exposes namespace views of objects and never owns object identity.
4. Objects advertise capabilities and operations instead of being identified only by concrete type.
5. Every object participates in health, diagnostics, events, and relationships by default.

## Phase 1: Core Types Contract

No subsystem may invent alternate identity primitives for objects, providers, handles, or operation IDs.

```rust
pub struct ObjectId(pub u64);
pub struct ProviderId(pub u64);
pub struct HandleId(pub u64);
pub struct OperationId(pub u32);

pub enum ObjectType {
    Kernel,
    Process,
    Thread,
    Device,
    Driver,
    MemoryRegion,
    Volume,
    File,
    Directory,
    Service,
    Network,
    Event,
    Log,
    Ai,
    Other,
}
```

## Phase 2: Kernel Object Contract

Objects are filesystem-agnostic and describe identity plus capabilities.

```rust
pub trait KernelObject {
    fn id(&self) -> ObjectId;
    fn kind(&self) -> ObjectType;
    fn name(&self) -> &str;
    fn state(&self) -> ObjectState;
    fn properties(&self) -> PropertySet;
    fn operations(&self) -> &'static [OperationId];
}
```

Required support types:

```rust
pub enum ObjectState {
    Online,
    Busy,
    Warning,
    Faulted,
    Offline,
}

pub struct Property {
    pub key: String,
    pub value: String,
}

pub type PropertySet = Vec<Property>;
```

## Phase 3: Namespace Provider Contract

Providers expose namespace views and object resolution. SAIFS never hardcodes subsystem-specific path logic.

```rust
pub trait NamespaceProvider {
    fn id(&self) -> ProviderId;
    fn name(&self) -> &str;

    fn lookup(&self, ctx: &LookupContext, path: &str) -> Result<LookupResult, SaifsError>;
    fn enumerate(&self, ctx: &LookupContext, path: &str) -> Result<Vec<DirEntry>, SaifsError>;

    fn create(&self, ctx: &LookupContext, path: &str, kind: CreateKind) -> Result<ObjectId, SaifsError>;
    fn remove(&self, ctx: &LookupContext, path: &str) -> Result<(), SaifsError>;
}
```

## Phase 4: Handle Contract

After path resolution, all internal operations use handles.

```rust
pub trait Handle {
    fn id(&self) -> HandleId;
    fn object_id(&self) -> ObjectId;
    fn provider_id(&self) -> ProviderId;

    fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, SaifsError>;
    fn write(&self, offset: u64, data: &[u8]) -> Result<usize, SaifsError>;

    fn query(&self, key: &str) -> Result<Property, SaifsError>;
    fn properties(&self) -> Result<PropertySet, SaifsError>;
    fn health(&self) -> Result<Health, SaifsError>;
    fn children(&self) -> Result<Vec<ObjectId>, SaifsError>;
}
```

Path to object flow happens exactly once:

- Path
- Provider lookup
- ObjectId
- Handle
- Operations

## Phase 5: Operations Contract

Capabilities are object-advertised. SAIFS dispatches by operation ID.

```rust
pub trait OperationDispatcher {
    fn supports(&self, object: ObjectId, op: OperationId) -> bool;
    fn invoke(&self, handle: HandleId, op: OperationId, args: &[u8]) -> Result<Vec<u8>, SaifsError>;
}
```

Examples:

- Process: read, pause, resume, kill, explain, diagnose
- Device: read, write, reset, health, benchmark
- Network: connect, disconnect, statistics, diagnose

## Phase 6: Metadata Contract

Every object must expose baseline metadata fields. Providers may extend attributes, not replace baseline fields.

Mandatory metadata keys:

- id
- type
- name
- owner
- created
- modified
- health
- status
- provider
- labels
- attributes

Notes:

- labels is a list-like metadata property.
- attributes is a provider-specific map namespace.

## Phase 7: Provider Model Contract

Providers are first-class and hot-pluggable through registration.

Implemented baseline contract in code:

```rust
pub trait Provider {
    fn id(&self) -> ProviderId;
    fn name(&self) -> &str;
    fn provider_type(&self) -> ProviderType;
    fn namespace(&self) -> &str;
    fn initialize(&mut self);
    fn shutdown(&mut self);
    fn enumerate(&self) -> Vec<ProviderObject>;
    fn lookup(&self, id: ObjectId) -> Option<ProviderObject>;
}
```

```rust
pub trait ProviderRegistry {
    fn register(&self, provider: &'static dyn NamespaceProvider) -> Result<ProviderId, SaifsError>;
    fn unregister(&self, provider: ProviderId) -> Result<(), SaifsError>;
    fn get(&self, provider: ProviderId) -> Option<&'static dyn NamespaceProvider>;
    fn list(&self) -> Vec<ProviderId>;
}
```

Target providers include:

- StorageProvider
- ObjectProvider
- ProcessProvider
- DeviceProvider
- DriverProvider
- MemoryProvider
- NetworkProvider
- LogProvider
- ServiceProvider
- AiProvider

## Phase 8: Mount Manager Contract

Mount table composes global namespace from providers.

```rust
pub struct MountPoint {
    pub path: String,
    pub provider: ProviderId,
    pub flags: MountFlags,
}

pub trait MountManager {
    fn mount(&self, mount: MountPoint) -> Result<(), SaifsError>;
    fn unmount(&self, path: &str) -> Result<(), SaifsError>;
    fn resolve_provider(&self, path: &str) -> Result<ProviderId, SaifsError>;
    fn mounts(&self) -> Vec<MountPoint>;
}
```

Reference topology:

- /boot -> FatProvider
- /tmp -> TmpProvider
- /sys -> ObjectProvider
- /proc -> ProcessProvider
- /dev -> DeviceProvider
- /logs -> LogProvider
- /ai -> AiProvider

## Phase 9: Object Registry Contract

Object Manager owns object identity and lifecycle.

```rust
pub trait ObjectRegistry {
    fn insert(&self, object: &'static dyn KernelObject) -> Result<ObjectId, SaifsError>;
    fn remove(&self, id: ObjectId) -> Result<(), SaifsError>;
    fn get(&self, id: ObjectId) -> Option<&'static dyn KernelObject>;
    fn find_by_name(&self, kind: ObjectType, name: &str) -> Option<ObjectId>;
    fn list_by_kind(&self, kind: ObjectType) -> Vec<ObjectId>;
}
```

## Phase 10: Observer System Contract

State change propagation is event-driven. Polling-based state sync is prohibited for core subsystems.

```rust
pub struct EventId(pub u64);

pub enum EventType {
    ObjectCreated,
    ObjectRemoved,
    PropertyChanged,
    HealthChanged,
    Mounted,
    Unmounted,
    DriverLoaded,
    DeviceAttached,
    MemoryAllocated,
}

pub struct Event {
    pub id: EventId,
    pub event_type: EventType,
    pub object: Option<ObjectId>,
    pub timestamp_ns: u64,
    pub payload: Vec<u8>,
}

pub trait EventBus {
    fn publish(&self, event: Event) -> Result<(), SaifsError>;
    fn subscribe(&self, filter: EventFilter) -> Result<SubscriptionId, SaifsError>;
    fn unsubscribe(&self, id: SubscriptionId) -> Result<(), SaifsError>;
}
```

## Query Service Contract

Object discovery is not path-only. Query is a first-class service and can back shell commands.

Current baseline query filters:

- `kind=<ObjectType>`
- `health=<HealthState>` and `health!=<HealthState>`
- `provider=<ProviderName>`
- `parent=<ParentObjectName>`

Implemented query behavior:

- Multiple predicates supported as comma-separated expressions (AND semantics)
- Example: `kind=Device,health!=Healthy,provider=devices`

Provider discovery surface:

- Native shell command: `providers`
- Namespace view: `/sys/providers`

## Layer Responsibilities (Frozen)

- Object Manager: identity, registry, baseline metadata, lifecycle source of truth.
- SAIFS: naming, routing, provider dispatch, handle lifecycle, access control integration point.
- Providers: namespace contribution and domain operations.
- VFS compatibility layer: optional POSIX-oriented view over SAIFS handles and providers.

## Invariants (Must Hold)

1. Objects exist without paths.
2. Paths can change without object identity change.
3. Internal operations do not require path re-resolution once a handle exists.
4. Providers can be added without SAIFS core redesign.
5. Baseline metadata schema is stable across object types.
6. Event publication occurs on all object lifecycle and mount lifecycle changes.

## Error Model

```rust
pub enum SaifsError {
    NotFound,
    AlreadyExists,
    InvalidPath,
    InvalidHandle,
    UnsupportedOperation,
    AccessDenied,
    ProviderUnavailable,
    Busy,
    Corrupt,
    Internal,
}
```

## Security and Policy Hooks

Policy is out of scope for this phase, but SAIFS contracts reserve hooks for:

- ownership checks
- label-based policy
- capability gating by operation ID

## Compatibility

POSIX-style APIs are adapters above SAIFS.

- open path resolves to SAIFS handle
- read or write maps to operation dispatch
- stat maps to baseline metadata

## Implementation Guidance (Non-Normative)

- Keep contracts in dedicated modules under seed/saios/src/saifs.
- Generate provider stubs first, then route current tmp and sys behavior through provider interfaces.
- Treat existing VFS code as an adapter that migrates toward SAIFS provider contracts.
