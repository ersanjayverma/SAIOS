pub mod flags;
pub mod mapper;
pub mod paging;
pub mod tlb;
use crate::memory::errors::MemoryResult;
use crate::memory::types::{AddressSpaceId, PhysAddr, VirtAddr};
use crate::memory::vmm::paging::PagingRoot;
use hal::memory::PageFlags;

pub trait VirtualMemoryManager {
    fn map(
        &mut self,
        owner: AddressSpaceId,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> MemoryResult<()>;
    fn unmap(&mut self, owner: AddressSpaceId, virt: VirtAddr) -> MemoryResult<()>;
    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr>;
    fn protect(
        &mut self,
        owner: AddressSpaceId,
        virt: VirtAddr,
        flags: PageFlags,
    ) -> MemoryResult<()>;
    fn switch(&mut self, root: PagingRoot) -> MemoryResult<()>;
}

pub use mapper::{active_root, clone_space_mappings, init, map, protect, switch, translate, unmap};
