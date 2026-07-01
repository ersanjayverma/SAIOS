use alloc::string::String;
use alloc::vec::Vec;

use crate::provider::ProviderType;
use crate::som::{
    CapabilitySet, EventId, HandleId, HealthState, ObjectClass, ObjectFlags, ObjectHeader, ObjectId,
    OperationId, ProviderId, Relationship, Timestamp,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AbiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl AbiVersion {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }

    pub const fn is_compatible_with(self, target: Self) -> bool {
        self.major == target.major && self.minor >= target.minor
    }
}

pub const SNOM_ABI_VERSION: AbiVersion = AbiVersion::new(1, 0, 0);

pub trait AbiStable {
    fn abi_version(&self) -> AbiVersion {
        SNOM_ABI_VERSION
    }

    fn abi_compatible(&self, required: AbiVersion) -> bool {
        self.abi_version().is_compatible_with(required)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ObjectVersion {
    pub generation: u32,
    pub revision: u32,
}

#[derive(Clone)]
pub struct Metadata {
    pub object_id: ObjectId,
    pub provider_id: ProviderId,
    pub provider_type: ProviderType,
    pub created: Timestamp,
    pub modified: Timestamp,
    pub version: ObjectVersion,
    pub class: ObjectClass,
    pub state: crate::som::ObjectState,
    pub health: HealthState,
    pub flags: ObjectFlags,
}

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

#[derive(Clone)]
pub struct TypedProperty {
    pub key: String,
    pub value: PropertyValue,
}

pub type PropertyStore = Vec<TypedProperty>;

#[derive(Clone)]
pub struct Operation {
    pub id: OperationId,
    pub name: &'static str,
    pub required: CapabilitySet,
    pub description: &'static str,
}

pub type OperationTable = Vec<Operation>;
pub type RelationshipTable = Vec<Relationship>;

#[derive(Clone)]
pub struct InvocationContext {
    pub handle: Option<HandleId>,
    pub caller: Option<ObjectId>,
}

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

pub trait SaiObject: AbiStable {
    fn header(&self) -> &ObjectHeader;
    fn metadata(&self) -> &Metadata;
    fn properties(&self) -> PropertyStore;
    fn operations(&self) -> OperationTable;
    fn relationships(&self) -> RelationshipTable;

    fn invoke(
        &self,
        _ctx: &InvocationContext,
        _op: OperationId,
        _args: &[OperationArg],
    ) -> Result<OperationResult, &'static str> {
        Err("operation not supported")
    }
}

#[derive(Clone)]
pub struct ObjectDescription {
    pub header: ObjectHeader,
    pub metadata: Metadata,
    pub properties: PropertyStore,
    pub operations: OperationTable,
    pub capabilities: CapabilitySet,
    pub relationships: RelationshipTable,
}

pub trait Reflect {
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TransactionState {
    Begun,
    Committed,
    RolledBack,
}

#[derive(Clone)]
pub struct ObjectTransaction {
    pub id: TransactionId,
    pub object: ObjectId,
    pub state: TransactionState,
    pub created: Timestamp,
}

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

#[derive(Clone)]
pub struct SnomEvent {
    pub id: EventId,
    pub kind: SnomEventKind,
    pub object: ObjectId,
    pub at: Timestamp,
    pub summary: String,
}

pub fn abi_version() -> AbiVersion {
    SNOM_ABI_VERSION
}
