use crate::memory::ownership::Owner;
use crate::memory::types::PhysicalFrame;

pub fn debug_violation(frame: PhysicalFrame, current: Option<Owner>, requested: Owner) {
    debug_assert!(
        false,
        "ownership violation for frame {:?}: current={:?}, requested={:?}",
        frame, current, requested
    );
}
