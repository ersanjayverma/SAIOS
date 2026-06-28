pub mod debug;
pub mod owner;
pub mod shared;
pub mod transfer;

use crate::memory::errors::MemoryResult;

pub use owner::Owner;
pub use transfer::{claim_frame, owner_of, release_frame, transfer_frame};

pub fn init() -> MemoryResult<()> {
    transfer::init()
}
