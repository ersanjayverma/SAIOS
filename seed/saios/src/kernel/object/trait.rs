use super::id::ObjectId;
use super::state::ObjectState;
use super::types::ObjectType;

pub trait KernelObject {
    fn id(&self) -> ObjectId;
    fn object_type(&self) -> ObjectType;
    fn name(&self) -> &str;
    fn state(&self) -> ObjectState;
    fn parent(&self) -> Option<ObjectId>;
    fn owner(&self) -> Option<ObjectId>;
    fn reference_count(&self) -> u32;
    fn created_tick(&self) -> u64;
    fn last_modified_tick(&self) -> u64;
}
