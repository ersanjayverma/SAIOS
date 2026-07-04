use alloc::string::String;

use super::id::ObjectId;
use super::state::ObjectState;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ObjectEvent {
    Created,
    StateChanged,
    ParentChanged,
    OwnerChanged,
    Acquired,
    Released,
    Destroyed,
}

#[derive(Clone, Debug)]
pub struct ObjectEventRecord {
    pub id: ObjectId,
    pub event: ObjectEvent,
    pub name: String,
    pub state: Option<ObjectState>,
    pub detail: String,
}
