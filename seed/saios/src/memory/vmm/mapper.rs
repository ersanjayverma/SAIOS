use crate::memory::constants::{MAX_VMM_MAPPINGS, PAGE_SIZE};
use crate::memory::errors::{MemoryError, MemoryResult};
use crate::memory::page_table::mapper::MappingEntry;
use crate::memory::types::{
    AddressSpaceId, PhysAddr, PhysAddrExt, PhysicalFrame, VirtAddr, VirtAddrExt,
};
use crate::memory::vmm::VirtualMemoryManager;
use crate::memory::vmm::paging::{PagingRoot, active_root as arch_active_root, switch_root};
use crate::memory::vmm::tlb;
use core::cell::UnsafeCell;
use hal::memory::PageFlags;

struct GlobalVmm(UnsafeCell<KernelVirtualMemoryManager>);

unsafe impl Sync for GlobalVmm {}

static VMM: GlobalVmm = GlobalVmm(UnsafeCell::new(KernelVirtualMemoryManager::new()));

pub fn init() -> MemoryResult<()> {
    manager().init()
}

pub fn map(
    owner: AddressSpaceId,
    virt: VirtAddr,
    phys: PhysAddr,
    flags: PageFlags,
) -> MemoryResult<()> {
    manager().map(owner, virt, phys, flags)
}

pub fn unmap(owner: AddressSpaceId, virt: VirtAddr) -> MemoryResult<()> {
    manager().unmap(owner, virt)
}

pub fn translate(virt: VirtAddr) -> Option<PhysAddr> {
    manager().translate(virt)
}

pub fn protect(owner: AddressSpaceId, virt: VirtAddr, flags: PageFlags) -> MemoryResult<()> {
    manager().protect(owner, virt, flags)
}

pub fn switch(root: PagingRoot) -> MemoryResult<()> {
    manager().switch(root)
}

pub fn active_root() -> PagingRoot {
    manager().active_root
}

pub fn clone_space_mappings(source: AddressSpaceId, target: AddressSpaceId) -> MemoryResult<()> {
    manager().clone_space_mappings(source, target)
}

fn manager() -> &'static mut KernelVirtualMemoryManager {
    unsafe { &mut *VMM.0.get() }
}

pub struct KernelVirtualMemoryManager {
    initialized: bool,
    active_root: PagingRoot,
    mappings: [MappingEntry; MAX_VMM_MAPPINGS],
}

impl KernelVirtualMemoryManager {
    const fn new() -> Self {
        Self {
            initialized: false,
            active_root: PagingRoot::new(PhysAddr::new(0)),
            mappings: [MappingEntry::empty(); MAX_VMM_MAPPINGS],
        }
    }

    fn init(&mut self) -> MemoryResult<()> {
        if self.initialized {
            return Err(MemoryError::AlreadyInitialized);
        }

        self.active_root = arch_active_root();
        self.initialized = true;
        Ok(())
    }

    fn find_mapping_index(&self, owner: AddressSpaceId, virt: VirtAddr) -> Option<usize> {
        self.mappings.iter().position(|entry| {
            entry.active
                && entry.owner == owner
                && entry.virt.as_u64() == virt.align_down(PAGE_SIZE).as_u64()
        })
    }

    fn first_free_slot(&self) -> Option<usize> {
        self.mappings.iter().position(|entry| !entry.active)
    }

    fn clone_space_mappings(
        &mut self,
        source: AddressSpaceId,
        target: AddressSpaceId,
    ) -> MemoryResult<()> {
        let source_mappings = self.mappings;
        for mapping in source_mappings
            .iter()
            .filter(|entry| entry.active && entry.owner == source)
        {
            self.map(
                target,
                mapping.virt,
                mapping.frame.start_address(),
                mapping.flags,
            )?;
        }

        Ok(())
    }
}

impl VirtualMemoryManager for KernelVirtualMemoryManager {
    fn map(
        &mut self,
        owner: AddressSpaceId,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> MemoryResult<()> {
        if !virt.is_page_aligned() || !phys.is_page_aligned() {
            return Err(MemoryError::AddressMisaligned);
        }

        if self.find_mapping_index(owner, virt).is_some() {
            return Err(MemoryError::MappingExists);
        }

        let slot = self.first_free_slot().ok_or(MemoryError::OutOfFrames)?;
        self.mappings[slot] = MappingEntry {
            active: true,
            owner,
            virt,
            frame: PhysicalFrame::from_start_address(phys)?,
            flags: flags | PageFlags::PRESENT,
        };
        tlb::flush(virt);
        Ok(())
    }

    fn unmap(&mut self, owner: AddressSpaceId, virt: VirtAddr) -> MemoryResult<()> {
        let slot = self
            .find_mapping_index(owner, virt)
            .ok_or(MemoryError::MappingNotFound)?;
        self.mappings[slot] = MappingEntry::empty();
        tlb::flush(virt.align_down(PAGE_SIZE));
        Ok(())
    }

    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr> {
        self.mappings
            .iter()
            .find(|entry| {
                entry.active && entry.virt.as_u64() == virt.align_down(PAGE_SIZE).as_u64()
            })
            .map(|entry| {
                PhysAddr::new(entry.frame.start_address().as_u64() + virt.page_offset() as u64)
            })
    }

    fn protect(
        &mut self,
        owner: AddressSpaceId,
        virt: VirtAddr,
        flags: PageFlags,
    ) -> MemoryResult<()> {
        let slot = self
            .find_mapping_index(owner, virt)
            .ok_or(MemoryError::MappingNotFound)?;
        self.mappings[slot].flags = flags | PageFlags::PRESENT;
        tlb::flush(virt.align_down(PAGE_SIZE));
        Ok(())
    }

    fn switch(&mut self, root: PagingRoot) -> MemoryResult<()> {
        if root.phys_addr().as_u64() as usize % PAGE_SIZE != 0 {
            return Err(MemoryError::AddressMisaligned);
        }

        unsafe { switch_root(root) };
        self.active_root = root;
        Ok(())
    }
}
