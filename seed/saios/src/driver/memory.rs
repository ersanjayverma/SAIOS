use efi_main::memorymap::{MemoryRegion, MemoryType};
use hal::arch::x86_64::sync::StaticCell;

pub const PAGE_SIZE: u64 = 4096;
const MAX_TRACKED_MEMORY_BYTES: u64 = 64 * 1024 * 1024 * 1024; // 64 GiB
const MAX_PAGES: usize = (MAX_TRACKED_MEMORY_BYTES / PAGE_SIZE) as usize;
const BITMAP_WORDS: usize = MAX_PAGES / 64;

struct Pmm {
    bitmap: [u64; BITMAP_WORDS],
    tracked_pages: usize,
    free_pages: usize,
    next_hint: usize,
    initialized: bool,
}

impl Pmm {
    const fn new() -> Self {
        Self {
            bitmap: [u64::MAX; BITMAP_WORDS],
            tracked_pages: 0,
            free_pages: 0,
            next_hint: 0,
            initialized: false,
        }
    }

    fn reset(&mut self) {
        self.bitmap.fill(u64::MAX);
        self.tracked_pages = 0;
        self.free_pages = 0;
        self.next_hint = 0;
        self.initialized = true;
    }

    fn is_used(&self, page: usize) -> bool {
        let word = page / 64;
        let bit = page % 64;
        (self.bitmap[word] & (1u64 << bit)) != 0
    }

    fn set_used(&mut self, page: usize) {
        let word = page / 64;
        let bit = page % 64;
        self.bitmap[word] |= 1u64 << bit;
    }

    fn set_free(&mut self, page: usize) {
        let word = page / 64;
        let bit = page % 64;
        self.bitmap[word] &= !(1u64 << bit);
    }
}

static PMM: StaticCell<Pmm> = StaticCell::new(Pmm::new());

const fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

const fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

/// Initialize the page allocator from the firmware-provided memory map.
pub fn init(entries: &[MemoryRegion]) {
    let pmm = unsafe { &mut *PMM.get() };
    pmm.reset();

    let mut highest = 0u64;
    for entry in entries {
        let end = entry.base.saturating_add(entry.length);
        if end > highest {
            highest = end;
        }
    }

    let tracked = (align_up(highest, PAGE_SIZE) / PAGE_SIZE) as usize;
    pmm.tracked_pages = core::cmp::min(tracked, MAX_PAGES);

    for entry in entries {
        if entry.region_type != MemoryType::Usable {
            continue;
        }

        let start = (align_up(entry.base, PAGE_SIZE) / PAGE_SIZE) as usize;
        let end = (align_down(entry.base.saturating_add(entry.length), PAGE_SIZE) / PAGE_SIZE) as usize;
        let limit = core::cmp::min(end, pmm.tracked_pages);

        for page in start..limit {
            if pmm.is_used(page) {
                pmm.set_free(page);
                pmm.free_pages += 1;
            }
        }
    }
}

/// Allocate one 4 KiB physical page.
pub fn alloc_page() -> Option<u64> {
    let pmm = unsafe { &mut *PMM.get() };
    if !pmm.initialized || pmm.free_pages == 0 || pmm.tracked_pages == 0 {
        return None;
    }

    let start = if pmm.next_hint < pmm.tracked_pages {
        pmm.next_hint
    } else {
        0
    };

    for offset in 0..pmm.tracked_pages {
        let page = (start + offset) % pmm.tracked_pages;
        if !pmm.is_used(page) {
            pmm.set_used(page);
            pmm.free_pages -= 1;
            pmm.next_hint = page + 1;
            return Some((page as u64) * PAGE_SIZE);
        }
    }

    None
}

/// Free one 4 KiB physical page.
pub fn free_page(phys_addr: u64) -> bool {
    if phys_addr & (PAGE_SIZE - 1) != 0 {
        return false;
    }

    let pmm = unsafe { &mut *PMM.get() };
    if !pmm.initialized {
        return false;
    }

    let page = (phys_addr / PAGE_SIZE) as usize;
    if page >= pmm.tracked_pages || !pmm.is_used(page) {
        return false;
    }

    pmm.set_free(page);
    pmm.free_pages += 1;
    if page < pmm.next_hint {
        pmm.next_hint = page;
    }

    true
}

/// Mark a physical address range as used.
pub fn reserve(base: u64, length: u64) {
    let pmm = unsafe { &mut *PMM.get() };
    if !pmm.initialized || length == 0 {
        return;
    }

    let start = (align_down(base, PAGE_SIZE) / PAGE_SIZE) as usize;
    let end = (align_up(base.saturating_add(length), PAGE_SIZE) / PAGE_SIZE) as usize;
    let limit = core::cmp::min(end, pmm.tracked_pages);

    for page in start..limit {
        if !pmm.is_used(page) {
            pmm.set_used(page);
            pmm.free_pages -= 1;
        }
    }
}

/// Bytes currently marked as used in the tracked PMM range.
pub fn used() -> u64 {
    let pmm = unsafe { &*PMM.get() };
    if !pmm.initialized {
        return 0;
    }
    ((pmm.tracked_pages - pmm.free_pages) as u64) * PAGE_SIZE
}

/// Bytes currently available in the tracked PMM range.
pub fn available() -> u64 {
    let pmm = unsafe { &*PMM.get() };
    if !pmm.initialized {
        return 0;
    }
    (pmm.free_pages as u64) * PAGE_SIZE
}
