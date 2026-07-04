//! SAIOS Native Object Model (SNOM) ABI types.
//!
//! SNOM defines the stable interface between the kernel's object model and
//! user-space: version negotiation, property stores, operation tables,
//! transactions and events. Types here are designed to be ABI-stable and are
//! the foundation for the [`SaiObject`] trait.

use alloc::string::String;
use alloc::vec::Vec;

use crate::provider::ProviderType;
use crate::som::{
    CapabilitySet, EventId, HandleId, HealthState, ObjectClass, ObjectFlags, ObjectHeader,
    ObjectId, OperationId, ProviderId, Relationship, Timestamp,
};

/// Three-component ABI version used for compatibility checks.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl AbiVersion {
    /// Constructs a new ABI version.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Returns true when `self` can satisfy a request for `target`.
    ///
    /// Compatibility requires the same major version and a minor version that
    /// is at least as high as the target's.
    pub const fn is_compatible_with(self, target: Self) -> bool {
        self.major == target.major && self.minor >= target.minor
    }
}

/// Current SNOM ABI version exported by the kernel.
pub const SNOM_ABI_VERSION: AbiVersion = AbiVersion::new(1, 0, 0);

/// Trait for objects that expose a stable ABI version.
pub trait AbiStable {
    /// Returns the ABI version implemented by this object.
    fn abi_version(&self) -> AbiVersion {
        SNOM_ABI_VERSION
    }

    /// Returns true if this object satisfies the requested ABI version.
    fn abi_compatible(&self, required: AbiVersion) -> bool {
        self.abi_version().is_compatible_with(required)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ObjectVersion {
    pub generation: u32,
    pub revision: u32,
}

/// Metadata describing a SNOM object instance.
#[derive(Clone)]
pub struct Metadata {
    /// Unique object identifier.
    pub object_id: ObjectId,
    /// Identifier of the provider that owns this object.
    pub provider_id: ProviderId,
    /// Category of the owning provider.
    pub provider_type: ProviderType,
    /// Creation timestamp.
    pub created: Timestamp,
    /// Last modification timestamp.
    pub modified: Timestamp,
    /// Object version (generation/revision).
    pub version: ObjectVersion,
    /// Object class discriminator.
    pub class: ObjectClass,
    /// Current lifecycle state.
    pub state: crate::som::ObjectState,
    /// Current health assessment.
    pub health: HealthState,
    /// Capability and behavior flags.
    pub flags: ObjectFlags,
}

/// Typed value stored in a SNOM property bag.
#[derive(Clone)]
pub enum PropertyValue {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Object(ObjectId),
    DurationNanos(u64),
}

/// A named property with a typed value.
#[derive(Clone)]
pub struct TypedProperty {
    /// Property name.
    pub key: String,
    /// Property value.
    pub value: PropertyValue,
}

/// Property bag returned by object introspection.
pub type PropertyStore = Vec<TypedProperty>;

/// Description of an operation that can be invoked on an object.
#[derive(Clone)]
pub struct Operation {
    /// Operation identifier.
    pub id: OperationId,
    /// Human-readable operation name.
    pub name: &'static str,
    /// Capabilities required to invoke the operation.
    pub required: CapabilitySet,
    /// Short description of the operation.
    pub description: &'static str,
}

/// Table of operations supported by an object.
pub type OperationTable = Vec<Operation>;
/// Table of relationships between objects.
pub type RelationshipTable = Vec<Relationship>;

/// Context passed to an object operation invocation.
#[derive(Clone)]
pub struct InvocationContext {
    /// Handle through which the operation was invoked, if any.
    pub handle: Option<HandleId>,
    /// Object identifier of the caller, if known.
    pub caller: Option<ObjectId>,
}

/// Argument value passed to an object operation.
#[derive(Clone)]
pub enum OperationArg {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Object(ObjectId),
}

/// Result value returned from an object operation.
#[derive(Clone)]
pub enum OperationResult {
    None,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Object(ObjectId),
    Properties(PropertyStore),
}

/// Trait implemented by every SNOM object.
///
/// `SaiObject` combines introspection (`properties`, `operations`,
/// `relationships`) with invocation. The default `invoke` implementation
/// rejects all operations; objects that support operations must override it.
pub trait SaiObject: AbiStable {
    /// Returns the object's header.
    fn header(&self) -> &ObjectHeader;
    /// Returns the object's metadata.
    fn metadata(&self) -> &Metadata;
    /// Returns the object's current property bag.
    fn properties(&self) -> PropertyStore;
    /// Returns the table of operations supported by the object.
    fn operations(&self) -> OperationTable;
    /// Returns the table of relationships the object participates in.
    fn relationships(&self) -> RelationshipTable;

    /// Invokes an operation on the object.
    ///
    /// The default implementation returns `Err("operation not supported")` so
    /// objects that do not implement any operations do not need to override
    /// this method.
    fn invoke(
        &self,
        _ctx: &InvocationContext,
        _op: OperationId,
        _args: &[OperationArg],
    ) -> Result<OperationResult, &'static str> {
        Err("operation not supported")
    }
}

/// Complete description of a SNOM object returned by reflection.
#[derive(Clone)]
pub struct ObjectDescription {
    /// Object header.
    pub header: ObjectHeader,
    /// Object metadata.
    pub metadata: Metadata,
    /// Current properties.
    pub properties: PropertyStore,
    /// Supported operations.
    pub operations: OperationTable,
    /// Capability set (currently always empty).
    pub capabilities: CapabilitySet,
    /// Object relationships.
    pub relationships: RelationshipTable,
}

/// Trait for objects that can produce a complete [`ObjectDescription`].
pub trait Reflect {
    /// Returns a full description of the object.
    fn describe(&self) -> ObjectDescription;
}

impl<T: SaiObject> Reflect for T {
    fn describe(&self) -> ObjectDescription {
        ObjectDescription {
            header: self.header().clone(),
            metadata: self.metadata().clone(),
            properties: self.properties(),
            operations: self.operations(),
            capabilities: CapabilitySet::NONE,
            relationships: self.relationships(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransactionId(pub u64);

/// Lifecycle state of a SNOM transaction.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransactionState {
    Begun,
    Committed,
    RolledBack,
}

/// A transaction affecting a single object.
#[derive(Clone)]
pub struct ObjectTransaction {
    /// Transaction identifier.
    pub id: TransactionId,
    /// Object affected by the transaction.
    pub object: ObjectId,
    /// Current transaction state.
    pub state: TransactionState,
    /// Timestamp when the transaction was created.
    pub created: Timestamp,
}

/// Kind of event emitted by the SNOM event bus.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SnomEventKind {
    ObjectCreated,
    ObjectDestroyed,
    PropertyChanged,
    RelationshipAdded,
    RelationshipRemoved,
    OperationExecuted,
    HealthChanged,
}

/// A single SNOM lifecycle or state-change event.
#[derive(Clone)]
pub struct SnomEvent {
    /// Event identifier.
    pub id: EventId,
    /// Event kind.
    pub kind: SnomEventKind,
    /// Object that the event concerns.
    pub object: ObjectId,
    /// Timestamp when the event occurred.
    pub at: Timestamp,
    /// Human-readable event summary.
    pub summary: String,
}

/// Returns the SNOM ABI version supported by this kernel build.
pub fn abi_version() -> AbiVersion {
    SNOM_ABI_VERSION
}
