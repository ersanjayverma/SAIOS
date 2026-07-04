# SAIOS Object Model (SOM)

Status: Frozen foundational contract
Owner: Kernel architecture
Last updated: 2026-07-05

## Why SOM Exists

SOM defines what an object is in SAIOS before namespace or filesystem concerns. SAIFS, Object Manager, diagnostics, observability, and AI integration depend on this model.

## Immutable Rules

1. Everything is an object.
2. Every object has a stable ObjectId for its lifetime.
3. SAIFS exposes namespace views of objects and never owns object identity.
4. Objects advertise capabilities and operations instead of being identified only by concrete type.
5. Every object participates in health, diagnostics, events, and relationships by default.

## Stable Object ID Contract

SAIOS ObjectId values are stable during object lifetime and never expose raw pointers.

Encoding:

- Type: 16 bits
- Namespace: 16 bits
- Sequence: 32 bits

Display labels are generated from object type prefixes, for example:

- `PROC-00000015`
- `DRV-00000003`
- `DEV-00000007`
- `VOL-00000001`

## Layer 0: Universal Object Header

Every kernel object begins with the same header shape.

```rust
pub struct ObjectHeader {
    pub id: ObjectId,
    pub class: ObjectClass,
    pub state: ObjectState,
    pub health: HealthState,

    pub name: SmallString<64>,

    pub owner: Option<ObjectId>,
    pub parent: Option<ObjectId>,

    pub created: Timestamp,
    pub modified: Timestamp,

    pub provider: ProviderId,

    pub flags: ObjectFlags,
}
```

No object is exempt.

## Layer 1: Stable Class Taxonomy

SOM class taxonomy is intentionally stable and changes rarely.

- Kernel
- System
- Storage
- Volume
- Filesystem
- Directory
- File
- Memory
- Region
- Page
- Process
- Thread
- Device
- Driver
- Network
- Interface
- Socket
- Route
- User
- Group
- Service
- Event
- Log
- AI
- Model
- Skill
- Task

## Layer 2: Capabilities

Objects are operated by capability, not by concrete file-like assumptions.

- READ
- WRITE
- EXECUTE
- ENUMERATE
- CREATE
- DELETE
- MOUNT
- STREAM
- DIAGNOSE
- EXPLAIN
- CONFIGURE
- SUBSCRIBE
- PERSIST

## Layer 3: Property Bag

Every object exposes a baseline property surface.

- name
- type
- state
- health
- owner
- labels
- attributes
- statistics
- metrics
- configuration

Subsystem-specific data extends the bag without replacing baseline keys.

## Layer 4: Operations

Objects expose operation descriptors with metadata.

Examples:

- Process: resume, pause, kill, debug, diagnose
- Storage: mount, repair, benchmark, trim
- Device: reset, enable, disable, identify

## Layer 5: Relationships

Objects encode graph relationships natively.

Examples:

- Scheduler -> Thread
- Disk -> Partition -> Filesystem -> Directory

Relationships power inspect and navigation without path dependence.

## Layer 6: References

Internal references use ObjectId only.

Forbidden internal identity references:

- path strings
- raw pointers as identity
- subsystem-local IDs as global IDs

## Layer 7: Lifecycle

All objects share lifecycle states:

- Created
- Initializing
- Ready
- Stopping
- Destroyed

Transition policy:

- Created -> Initializing
- Initializing -> Ready
- Initializing -> Stopping
- Ready -> Stopping
- Stopping -> Destroyed

No subsystem-specific lifecycle aliases should replace this core contract.

## Layer 8: Event Integration

Every object lifecycle and state transition emits standard object events.

- Created
- Deleted
- Updated
- Moved
- Mounted
- HealthChanged
- StateChanged

## Layer 9: AI Integration

Objects support explainability and diagnostics from day one, with room for future AI augmentation.

- Explain
- Diagnose
- Predict
- Recommend

## How SOM Connects to SAIFS

- Object Manager owns SOM object identity and lifecycle.
- SAIFS resolves names to handles that reference SOM objects.
- Namespace views such as /sys, /dev, /proc are representations over SOM objects.

## Architecture Position

```text
Kernel Objects -> Object Manager -> Property DB + Relationship DB + Event Bus -> SAIFS -> Namespace Views
```

## Contract Location

Reference implementation contracts live in [seed/saios/src/som.rs](../seed/saios/src/som.rs).
