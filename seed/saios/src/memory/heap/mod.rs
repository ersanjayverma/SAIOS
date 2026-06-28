pub mod buddy;
pub mod kmalloc;
pub mod slab;
pub mod stats;

use crate::memory::errors::MemoryResult;

pub trait HeapAllocator {
    fn alloc(&mut self, size: usize, align: usize) -> *mut u8;
    fn free(&mut self, ptr: *mut u8) -> MemoryResult<()>;
    fn realloc(&mut self, ptr: *mut u8, size: usize, align: usize) -> *mut u8;
    fn stats(&self) -> stats::HeapStats;
}

pub use kmalloc::{alloc, free, init, realloc, stats};
