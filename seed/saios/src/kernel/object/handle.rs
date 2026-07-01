use super::id::ObjectId;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct ObjectHandle {
    pub(crate) id: ObjectId,
}

impl ObjectHandle {
    pub const fn new(id: ObjectId) -> Self {
        Self { id }
    }

    pub const fn id(self) -> ObjectId {
        self.id
    }
}
