pub mod allocator;
pub mod bitmap;
pub mod statistics;

use crate::memory::errors::MemoryResult;
use crate::memory::types::{BootMemoryMapView, PhysAddr, PhysicalFrame};

pub trait PhysicalMemoryManager {
    fn init(&mut self, memory_map: &BootMemoryMapView<'_>) -> MemoryResult<()>;
    fn alloc_frame(&mut self) -> MemoryResult<PhysicalFrame>;
    fn free_frame(&mut self, frame: PhysicalFrame) -> MemoryResult<()>;
    fn reserve(&mut self, start: PhysAddr, size: usize) -> MemoryResult<()>;
    fn total_memory(&self) -> usize;
    fn free_memory(&self) -> usize;
}

pub use allocator::{alloc_frame, free_frame, free_memory, init, reserve, total_memory};
