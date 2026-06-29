pub mod kernel;
pub mod layout;
pub mod process;

use core::cell::UnsafeCell;

use crate::memory::constants::MAX_ADDRESS_SPACES;
use crate::memory::errors::{MemoryError, MemoryResult};
use crate::memory::types::{AddressSpaceId, PhysAddr, VirtAddr};
use crate::memory::vmm;
use crate::memory::vmm::paging::PagingRoot;
use hal::memory::PageFlags;

pub trait AddressSpace {
    fn id(&self) -> AddressSpaceId;
    fn root(&self) -> PagingRoot;
    fn map(&self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> MemoryResult<()>;
    fn unmap(&self, virt: VirtAddr) -> MemoryResult<()>;
    fn translate(&self, virt: VirtAddr) -> Option<PhysAddr>;
    fn clone_space(&self) -> MemoryResult<process::ProcessAddressSpace>;
    fn activate(&self) -> MemoryResult<()>;
    fn destroy(&self) -> MemoryResult<()>;
}

struct GlobalRegistry(UnsafeCell<AddressSpaceManager>);

unsafe impl Sync for GlobalRegistry {}

static REGISTRY: GlobalRegistry = GlobalRegistry(UnsafeCell::new(AddressSpaceManager::new()));

pub fn init() -> MemoryResult<()> {
    AddressSpaceRegistry::init()
}

pub struct AddressSpaceRegistry;

impl AddressSpaceRegistry {
    pub fn init() -> MemoryResult<()> {
        registry().init()
    }

    pub fn kernel_space() -> kernel::KernelAddressSpace {
        registry().kernel_space()
    }

    pub fn create_process_space() -> MemoryResult<process::ProcessAddressSpace> {
        registry().create_process_space()
    }
}

#[derive(Debug, Copy, Clone)]
struct SpaceSlot {
    used: bool,
    id: AddressSpaceId,
    root: PagingRoot,
}

impl SpaceSlot {
    const fn empty() -> Self {
        Self {
            used: false,
            id: AddressSpaceId::new(0),
            root: PagingRoot::new(PhysAddr::new(0)),
        }
    }
}

struct AddressSpaceManager {
    initialized: bool,
    active_id: AddressSpaceId,
    slots: [SpaceSlot; MAX_ADDRESS_SPACES],
}

impl AddressSpaceManager {
    const fn new() -> Self {
        Self {
            initialized: false,
            active_id: AddressSpaceId::new(0),
            slots: [SpaceSlot::empty(); MAX_ADDRESS_SPACES],
        }
    }

    fn init(&mut self) -> MemoryResult<()> {
        if self.initialized {
            return Err(MemoryError::AlreadyInitialized);
        }

        self.slots[0] = SpaceSlot {
            used: true,
            id: AddressSpaceId::new(0),
            root: vmm::active_root(),
        };
        self.active_id = AddressSpaceId::new(0);
        self.initialized = true;
        Ok(())
    }

    fn kernel_space(&self) -> kernel::KernelAddressSpace {
        kernel::KernelAddressSpace::new(self.slots[0].id, self.slots[0].root)
    }

    fn create_process_space(&mut self) -> MemoryResult<process::ProcessAddressSpace> {
        let slot = self
            .slots
            .iter_mut()
            .position(|entry| !entry.used)
            .ok_or(MemoryError::NoAddressSpaceSlots)?;

        // Allocate a fresh PML4 table for the new address space.
        // The kernel half (entries 256–511) is cloned from the kernel's
        // PML4 so that kernel mappings are shared across all spaces, while
        // the user half (entries 0–255) starts empty.
        let new_pml4_frame = crate::memory::pmm::alloc_frame()?;
        let kernel_root = self.slots[0].root;

        // Copy the kernel's PML4 entries into the new table.
        unsafe {
            let kernel_pml4 = crate::memory::page_table::walker::table_from_phys(
                kernel_root.phys_addr(),
            );
            let new_pml4 = crate::memory::page_table::walker::table_from_phys(
                new_pml4_frame.start_address(),
            );
            // Only copy kernel-half entries (indices 256..512).
            // User-half entries (0..256) remain zeroed.
            for i in 256..512 {
                new_pml4.entries[i] = kernel_pml4.entries[i];
            }
        }

        let root = PagingRoot::new(new_pml4_frame.start_address());
        let id = AddressSpaceId::new(slot as u16);
        self.slots[slot] = SpaceSlot {
            used: true,
            id,
            root,
        };
        Ok(process::ProcessAddressSpace::new(id, root))
    }

    fn destroy(&mut self, id: AddressSpaceId) -> MemoryResult<()> {
        if id.raw() == 0 {
            return Err(MemoryError::Unsupported);
        }

        let slot = self
            .slots
            .iter_mut()
            .find(|entry| entry.used && entry.id == id)
            .ok_or(MemoryError::MappingNotFound)?;
        *slot = SpaceSlot::empty();
        Ok(())
    }
}

fn registry() -> &'static mut AddressSpaceManager {
    unsafe { &mut *REGISTRY.0.get() }
}

pub(crate) fn destroy_space(id: AddressSpaceId) -> MemoryResult<()> {
    registry().destroy(id)
}
