use crate::memory::address_space::{AddressSpace, process::ProcessAddressSpace};
use crate::memory::errors::MemoryResult;
use crate::memory::types::{AddressSpaceId, PhysAddr, VirtAddr};
use crate::memory::vmm;
use crate::memory::vmm::paging::PagingRoot;
use hal::memory::PageFlags;

#[derive(Debug, Copy, Clone)]
pub struct KernelAddressSpace {
    id: AddressSpaceId,
    root: PagingRoot,
}

impl KernelAddressSpace {
    pub(crate) const fn new(id: AddressSpaceId, root: PagingRoot) -> Self {
        Self { id, root }
    }
}

impl AddressSpace for KernelAddressSpace {
    fn id(&self) -> AddressSpaceId {
        self.id
    }

    fn root(&self) -> PagingRoot {
        self.root
    }

    fn map(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> MemoryResult<()> {
        vmm::map(self.id, virt, phys, flags)
    }

    fn unmap(&self, virt: VirtAddr) -> MemoryResult<()> {
        vmm::unmap(self.id, virt)
    }

    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        vmm::translate(virt)
    }

    fn clone_space(&self) -> MemoryResult<ProcessAddressSpace> {
        crate::memory::address_space::AddressSpaceRegistry::create_process_space()
    }

    fn activate(&self) -> MemoryResult<()> {
        vmm::switch(self.root)
    }

    fn destroy(&self) -> MemoryResult<()> {
        Ok(())
    }
}
