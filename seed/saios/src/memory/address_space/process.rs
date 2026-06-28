use crate::memory::address_space::{AddressSpace, destroy_space};
use crate::memory::errors::MemoryResult;
use crate::memory::types::{AddressSpaceId, PhysAddr, VirtAddr};
use crate::memory::vmm;
use crate::memory::vmm::paging::PagingRoot;
use hal::memory::PageFlags;

#[derive(Debug, Copy, Clone)]
pub struct ProcessAddressSpace {
    id: AddressSpaceId,
    root: PagingRoot,
}

impl ProcessAddressSpace {
    pub(crate) const fn new(id: AddressSpaceId, root: PagingRoot) -> Self {
        Self { id, root }
    }
}

impl AddressSpace for ProcessAddressSpace {
    fn id(&self) -> AddressSpaceId {
        self.id
    }

    fn root(&self) -> PagingRoot {
        self.root
    }

    fn map(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> MemoryResult<()> {
        vmm::map(self.id, virt, phys, flags | PageFlags::USER)
    }

    fn unmap(&self, virt: VirtAddr) -> MemoryResult<()> {
        vmm::unmap(self.id, virt)
    }

    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        vmm::translate(virt)
    }

    fn clone_space(&self) -> MemoryResult<ProcessAddressSpace> {
        let clone = crate::memory::address_space::AddressSpaceRegistry::create_process_space()?;
        vmm::clone_space_mappings(self.id, clone.id)?;
        Ok(clone)
    }

    fn activate(&self) -> MemoryResult<()> {
        vmm::switch(self.root)
    }

    fn destroy(&self) -> MemoryResult<()> {
        destroy_space(self.id)
    }
}
