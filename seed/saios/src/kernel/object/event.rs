use alloc::string::String;

use super::id::ObjectId;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ObjectEvent {
    Created,
    Started,
    Stopped,
    Destroyed,
    Faulted,
}

#[derive(Clone, Debug)]
pub struct ObjectEventRecord {
    pub id: ObjectId,
    pub event: ObjectEvent,
    pub name: String,
}
