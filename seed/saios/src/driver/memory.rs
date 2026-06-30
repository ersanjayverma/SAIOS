use hal::arch::paging::Table;
use efi_main::memorymap::{MemoryRegion, MemoryType};

pub struct BootAllocator {
    next_free_phys: u64,
    remaining_bytes: u64,
}

impl BootAllocator {
    pub unsafe fn new(entries: &[MemoryRegion]) -> Self {
        for entry in entries {
            // MemoryType::Usable corresponds to UEFI's CONVENTIONAL memory.
            // Check if this area has enough space (e.g., at least 64 KB for early structures).
            if entry.region_type == MemoryType::Usable && entry.length >= 65536 {
                return Self {
                    next_free_phys: entry.base,
                    remaining_bytes: entry.length,
                };
            }
        }
        panic!("No usable conventional memory found for page tables!");
    }

    /// Allocates a 4096-byte chunk guaranteed to be 4K aligned
    pub unsafe fn allocate_page_table(&mut self) -> *mut Table {
        if self.remaining_bytes < 4096 {
            panic!("Out of bootstrap memory!");
        }

        let addr = self.next_free_phys;
        
        // Advance pointer and decrease available pool
        self.next_free_phys += 4096;
        self.remaining_bytes -= 4096;

        let ptr = addr as *mut Table;
        (*ptr).clear(); // Ensure entries are safely zeroed out
        ptr
    }
}
