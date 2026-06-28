#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MemoryError {
    AlreadyInitialized,
    NotInitialized,
    InvalidMemoryMap,
    TooManyFrames,
    FrameOutOfRange,
    DoubleFree,
    InvalidFree,
    OutOfFrames,
    AddressMisaligned,
    MappingExists,
    MappingNotFound,
    NoAddressSpaceSlots,
    HeapOutOfMemory,
    InvalidAllocation,
    OwnershipConflict,
    OwnershipMissing,
    Unsupported,
}

pub type MemoryResult<T> = Result<T, MemoryError>;
