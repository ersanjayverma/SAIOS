use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u64);

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(pub u64);

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandleId(pub u64);

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationId(pub u32);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ObjectClass {
    Kernel,
    System,
    Storage,
    Volume,
    Filesystem,
    Directory,
    File,
    Memory,
    Region,
    Page,
    Process,
    Thread,
    Device,
    Driver,
    Network,
    Interface,
    Socket,
    Route,
    User,
    Group,
    Service,
    Event,
    Log,
    AI,
    Model,
    Skill,
    Task,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ObjectState {
    Created,
    Initialized,
    Running,
    Paused,
    Stopping,
    Destroyed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HealthState {
    Healthy,
    Warning,
    Critical,
    Offline,
}

pub type Timestamp = u64;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ObjectFlags(pub u64);

impl ObjectFlags {
    pub const NONE: Self = Self(0);
    pub const PERSISTENT: Self = Self(1 << 0);
    pub const VIRTUAL: Self = Self(1 << 1);
    pub const READ_ONLY: Self = Self(1 << 2);
    pub const SYSTEM: Self = Self(1 << 3);

    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

pub type SmallString64 = String;

#[derive(Clone)]
pub struct ObjectHeader {
    pub id: ObjectId,
    pub class: ObjectClass,
    pub state: ObjectState,
    pub health: HealthState,
    pub name: SmallString64,
    pub owner: Option<ObjectId>,
    pub parent: Option<ObjectId>,
    pub created: Timestamp,
    pub modified: Timestamp,
    pub provider: ProviderId,
    pub flags: ObjectFlags,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct CapabilitySet(pub u64);

impl CapabilitySet {
    pub const NONE: Self = Self(0);
    pub const READ: Self = Self(1 << 0);
    pub const WRITE: Self = Self(1 << 1);
    pub const EXECUTE: Self = Self(1 << 2);
    pub const ENUMERATE: Self = Self(1 << 3);
    pub const CREATE: Self = Self(1 << 4);
    pub const DELETE: Self = Self(1 << 5);
    pub const MOUNT: Self = Self(1 << 6);
    pub const STREAM: Self = Self(1 << 7);
    pub const DIAGNOSE: Self = Self(1 << 8);
    pub const EXPLAIN: Self = Self(1 << 9);
    pub const CONFIGURE: Self = Self(1 << 10);
    pub const SUBSCRIBE: Self = Self(1 << 11);
    pub const PERSIST: Self = Self(1 << 12);

    pub const fn contains(self, cap: Self) -> bool {
        (self.0 & cap.0) == cap.0
    }
}

#[derive(Clone)]
pub struct Property {
    pub key: String,
    pub value: String,
}

pub type PropertySet = Vec<Property>;

#[derive(Clone)]
pub struct OperationDescriptor {
    pub id: OperationId,
    pub name: &'static str,
    pub required: CapabilitySet,
    pub description: &'static str,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RelationshipKind {
    Parent,
    Child,
    Owner,
    Owned,
    DependsOn,
    BackedBy,
    Exposes,
    MountedAt,
}

#[derive(Clone)]
pub struct Relationship {
    pub from: ObjectId,
    pub to: ObjectId,
    pub kind: RelationshipKind,
}

pub trait KernelObject {
    fn id(&self) -> ObjectId;
    fn kind(&self) -> ObjectClass;
    fn name(&self) -> &str;
    fn state(&self) -> ObjectState;
    fn properties(&self) -> PropertySet;
    fn operations(&self) -> &'static [OperationId];
}

pub trait SomObject: KernelObject {
    fn header(&self) -> &ObjectHeader;
    fn capabilities(&self) -> CapabilitySet;
    fn relationships(&self) -> Vec<Relationship>;
}

pub struct Explanation {
    pub summary: String,
    pub details: Vec<String>,
}

pub struct DiagnosticReport {
    pub health: HealthState,
    pub findings: Vec<String>,
    pub recommendation: String,
}

pub struct Prediction {
    pub forecast: String,
    pub confidence_pct: u8,
}

pub struct Recommendation {
    pub action: String,
    pub rationale: String,
}

pub trait Explainable {
    fn explain(&self) -> Explanation;
}

pub trait Diagnosable {
    fn diagnose(&self) -> DiagnosticReport;
}

pub trait Predictable {
    fn predict(&self) -> Prediction;
}

pub trait Recommendable {
    fn recommend(&self) -> Recommendation;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(pub u64);

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ObjectEventKind {
    Created,
    Deleted,
    Updated,
    Moved,
    Mounted,
    HealthChanged,
    StateChanged,
}

#[derive(Clone)]
pub struct ObjectEvent {
    pub id: EventId,
    pub kind: ObjectEventKind,
    pub object: ObjectId,
    pub at: Timestamp,
    pub details: String,
}
