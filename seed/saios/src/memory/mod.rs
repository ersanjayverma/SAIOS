pub mod address_space;
pub mod constants;
pub mod errors;
pub mod heap;
pub mod ownership;
pub mod page_table;
pub mod pmm;
pub mod types;
pub mod vmm;
use efi_main::SaiosBootInfo;

use crate::memory::address_space::AddressSpace;
use crate::memory::address_space::AddressSpaceRegistry;
use crate::memory::errors::MemoryResult;
use crate::memory::types::BootMemoryMapView;
pub fn init(boot_info: &SaiosBootInfo) -> MemoryResult<()> {
    crate::console::write_debug_str("[MEMORY] Entering init\n");

    crate::console::write_debug_str("[MEMORY] Creating BootMemoryMapView\n");
    let memory_map = unsafe { BootMemoryMapView::from_raw(&boot_info.memorymap)? };
    crate::console::write_debug_str("[MEMORY] BootMemoryMapView created\n");

    crate::console::write_debug_str("[MEMORY] Calling pmm::init\n");
    match pmm::init(&memory_map) {
        Ok(()) => crate::console::write_debug_str("[MEMORY] pmm::init returned OK\n"),
        Err(e) => {
            crate::console::write_debug_str("[MEMORY] pmm::init FAILED: ");
            match e {
                crate::memory::errors::MemoryError::TooManyFrames => {
                    crate::console::write_debug_str("TooManyFrames\n");
                }
                crate::memory::errors::MemoryError::InvalidMemoryMap => {
                    crate::console::write_debug_str("InvalidMemoryMap\n");
                }
                crate::memory::errors::MemoryError::AlreadyInitialized => {
                    crate::console::write_debug_str("AlreadyInitialized\n");
                }
                _ => crate::console::write_debug_str("Unknown\n"),
            }
            return Err(e);
        }
    }

    // Paging must be initialized before the VMM mapper — the mapper
    // reads CR3 via paging::active_root() which requires PAGING.call_once().
    self::vmm::paging::init();
    crate::console::write_debug_str("[MEMORY] paging::init OK\n");

    vmm::init()?;
    crate::console::write_debug_str("[MEMORY] vmm::init returned OK\n");

    address_space::init()?;
    crate::console::write_debug_str("[MEMORY] address_space::init returned OK\n");

    heap::init()?;
    crate::console::write_debug_str("[MEMORY] heap::init returned OK\n");

    ownership::init()?;
    crate::console::write_debug_str("[MEMORY] ownership::init returned OK\n");

    AddressSpaceRegistry::kernel_space().activate()?;
    crate::console::write_debug_str("[MEMORY] Kernel space activated\n");

    crate::console::write_debug_str("[MEMORY] OK\n");

    Ok(())
}

pub fn total_memory() -> usize {
    pmm::total_memory()
}

pub fn free_memory() -> usize {
    pmm::free_memory()
}
