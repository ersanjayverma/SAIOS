use alloc::format;
use alloc::string::String;

use super::types::ObjectType;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ObjectId(pub u64);

impl ObjectId {
    pub const fn from_parts(type_code: u16, namespace: u16, sequence: u32) -> Self {
        let value = ((type_code as u64) << 48) | ((namespace as u64) << 32) | (sequence as u64);
        Self(value)
    }

    pub const fn type_code(self) -> u16 {
        (self.0 >> 48) as u16
    }

    pub const fn namespace(self) -> u16 {
        ((self.0 >> 32) & 0xFFFF) as u16
    }

    pub const fn sequence(self) -> u32 {
        (self.0 & 0xFFFF_FFFF) as u32
    }

    pub fn label(self, object_type: ObjectType) -> String {
        format!("{}-{:08X}", object_type.prefix(), self.sequence())
    }
}
