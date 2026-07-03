//! Physical memory manager (PMM) facade.
//!
//! Re-exports the page size and address type and delegates allocation,
//! reservation and statistics to the lower-level memory driver.

use efi_main::memorymap::MemoryRegion;

pub use crate::driver::memory::PAGE_SIZE;
/// Physical address type.
pub type PhysAddr = crate::driver::memory::PhysAddr;
/// Memory map slice type.
pub type MemoryMap = [MemoryRegion];

/// Statistics snapshot for the kernel heap.
#[derive(Copy, Clone, Debug, Default)]
pub struct HeapStats {
    pub total: usize,
    pub used: usize,
    pub free: usize,
}

/// Initializes the physical memory manager from the UEFI memory map.
pub fn init(memory_map: &MemoryMap) {
    crate::driver::memory::init(memory_map);
}

/// Allocates a single physical page.
pub fn alloc_page() -> Option<PhysAddr> {
    crate::driver::memory::alloc_page()
}

/// Allocates `count` contiguous physical pages.
pub fn alloc_pages(count: usize) -> Option<PhysAddr> {
    crate::driver::memory::alloc_pages(count)
}

/// Frees a single physical page, ignoring errors.
pub fn free_page(page: PhysAddr) {
    let _ = crate::driver::memory::free_page(page);
}

/// Frees a single physical page and returns true on success.
pub fn try_free_page(page: PhysAddr) -> bool {
    crate::driver::memory::free_page(page)
}

/// Frees `count` pages starting at `base`.
pub fn free_pages_range(base: PhysAddr, count: usize) -> bool {
    if count == 0 {
        return false;
    }

    let mut ok = true;
    let mut current = base;
    for _ in 0..count {
        if !crate::driver::memory::free_page(current) {
            ok = false;
        }
        current = current.saturating_add(PAGE_SIZE);
    }

    ok
}

/// Marks the physical range `[base, base + length)` as reserved.
pub fn reserve(base: PhysAddr, length: u64) {
    crate::driver::memory::reserve(base, length);
}

/// Returns the number of bytes available for allocation.
pub fn available_bytes() -> u64 {
    crate::driver::memory::available()
}

/// Returns the number of bytes currently allocated.
pub fn used_bytes() -> u64 {
    crate::driver::memory::used()
}

/// Returns the total number of physical pages.
pub fn total_pages() -> usize {
    crate::driver::memory::total_pages()
}

/// Returns the number of free physical pages.
pub fn free_pages() -> usize {
    crate::driver::memory::free_pages()
}

/// Returns the number of used physical pages.
pub fn used_pages() -> usize {
    crate::driver::memory::used_pages()
}

/// Returns the total physical RAM in megabytes.
pub fn total_ram_mb() -> usize {
    (total_pages() * (PAGE_SIZE as usize)) / (1024 * 1024)
}

/// Diagnostic test that allocates, frees and reallocates pages.
pub fn run_reuse_test(page_count: usize) {
    use heapless::Vec;

    let before_free = free_pages();
    let before_used = used_pages();

    let mut first_round: Vec<PhysAddr, 1000> = Vec::new();
    for _ in 0..core::cmp::min(page_count, 1000) {
        if let Some(page) = alloc_page() {
            crate::console::println!("alloc {:x}", page);
            let _ = first_round.push(page);
        } else {
            crate::console::println!("alloc failed");
            break;
        }
    }

    for &page in &first_round {
        free_page(page);
    }

    let mut second_round: Vec<PhysAddr, 1000> = Vec::new();
    for _ in 0..first_round.len() {
        if let Some(page) = alloc_page() {
            crate::console::println!("realloc {:x}", page);
            let _ = second_round.push(page);
        } else {
            crate::console::println!("realloc failed");
            break;
        }
    }

    for &page in &second_round {
        free_page(page);
    }

    let after_free = free_pages();
    let after_used = used_pages();

    crate::console::println!("before used={} free={}", before_used, before_free);
    crate::console::println!("after  used={} free={}", after_used, after_free);
}
