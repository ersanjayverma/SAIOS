use super::id::ObjectId;
use super::state::ObjectState;
use super::types::ObjectType;

pub trait KernelObject {
    fn id(&self) -> ObjectId;
    fn object_type(&self) -> ObjectType;
    fn name(&self) -> &str;
    fn state(&self) -> ObjectState;
}
