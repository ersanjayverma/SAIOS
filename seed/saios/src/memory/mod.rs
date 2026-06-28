pub mod address_space;
pub mod constants;
pub mod errors;
pub mod heap;
pub mod ownership;
pub mod pmm;
pub mod types;
pub mod vmm;

use efi_main::SaiosBootInfo;

use crate::memory::address_space::AddressSpace;
use crate::memory::address_space::AddressSpaceRegistry;
use crate::memory::errors::MemoryResult;
use crate::memory::types::BootMemoryMapView;

pub fn init(boot_info: &SaiosBootInfo) -> MemoryResult<()> {
    let memory_map = unsafe { BootMemoryMapView::from_raw(&boot_info.memorymap)? };

    pmm::init(&memory_map)?;
    vmm::init()?;
    address_space::init()?;
    heap::init()?;
    ownership::init()?;

    // Keep the kernel address space active after bootstrap initialization.
    AddressSpaceRegistry::kernel_space().activate()?;

    Ok(())
}

pub fn total_memory() -> usize {
    pmm::total_memory()
}

pub fn free_memory() -> usize {
    pmm::free_memory()
}
