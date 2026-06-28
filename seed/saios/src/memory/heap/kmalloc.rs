use core::cell::UnsafeCell;
use core::ptr;

use crate::memory::constants::{
    EARLY_HEAP_SIZE, HEAP_PAGE_COUNT, MAX_HEAP_ALLOCATIONS, PAGE_SIZE, SMALL_HEAP_LIMIT,
};
use crate::memory::errors::{MemoryError, MemoryResult};
use crate::memory::heap::HeapAllocator;
use crate::memory::heap::buddy::BuddyAllocator;
use crate::memory::heap::slab::SlabCache;
use crate::memory::heap::stats::HeapStats;
use crate::memory::types::pages_for_bytes;

const SLAB_CLASSES: [usize; 7] = [16, 32, 64, 128, 256, 512, 1024];

struct GlobalHeap(UnsafeCell<BootstrapHeapAllocator>);

unsafe impl Sync for GlobalHeap {}

static HEAP: GlobalHeap = GlobalHeap(UnsafeCell::new(BootstrapHeapAllocator::new()));

pub fn init() -> MemoryResult<()> {
    heap().init()
}

pub fn alloc(size: usize, align: usize) -> *mut u8 {
    heap().alloc(size, align)
}

pub fn free(ptr: *mut u8) -> MemoryResult<()> {
    heap().free(ptr)
}

pub fn realloc(ptr: *mut u8, size: usize, align: usize) -> *mut u8 {
    heap().realloc(ptr, size, align)
}

pub fn stats() -> HeapStats {
    heap().stats()
}

fn heap() -> &'static mut BootstrapHeapAllocator {
    unsafe { &mut *HEAP.0.get() }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum AllocationKind {
    Small(usize),
    Large,
}

#[derive(Debug, Copy, Clone)]
struct AllocationRecord {
    active: bool,
    offset: usize,
    size: usize,
    align: usize,
    kind: AllocationKind,
}

impl AllocationRecord {
    const fn empty() -> Self {
        Self {
            active: false,
            offset: 0,
            size: 0,
            align: 0,
            kind: AllocationKind::Large,
        }
    }
}

pub struct BootstrapHeapAllocator {
    initialized: bool,
    arena: [u8; EARLY_HEAP_SIZE],
    small_region_limit: usize,
    slabs: [SlabCache; SLAB_CLASSES.len()],
    buddy: BuddyAllocator,
    allocations: [AllocationRecord; MAX_HEAP_ALLOCATIONS],
    stats: HeapStats,
}

impl BootstrapHeapAllocator {
    const fn new() -> Self {
        Self {
            initialized: false,
            arena: [0; EARLY_HEAP_SIZE],
            small_region_limit: HEAP_PAGE_COUNT / 4 * PAGE_SIZE,
            slabs: [
                SlabCache::new(16),
                SlabCache::new(32),
                SlabCache::new(64),
                SlabCache::new(128),
                SlabCache::new(256),
                SlabCache::new(512),
                SlabCache::new(1024),
            ],
            buddy: BuddyAllocator::new(),
            allocations: [AllocationRecord::empty(); MAX_HEAP_ALLOCATIONS],
            stats: HeapStats::empty(EARLY_HEAP_SIZE),
        }
    }

    fn init(&mut self) -> MemoryResult<()> {
        if self.initialized {
            return Err(MemoryError::AlreadyInitialized);
        }

        self.initialized = true;
        Ok(())
    }

    fn find_record(&self, ptr: *mut u8) -> Option<usize> {
        let base = self.arena.as_ptr() as usize;
        let target = ptr as usize;
        self.allocations
            .iter()
            .position(|entry| base + entry.offset == target && (entry.active || entry.size != 0))
    }

    fn reserve_record(&mut self, record: AllocationRecord) -> Option<usize> {
        let slot = self
            .allocations
            .iter()
            .position(|entry| !entry.active && entry.size == 0)?;
        self.allocations[slot] = record;
        Some(slot)
    }

    fn allocate_small(&mut self, size: usize, align: usize) -> *mut u8 {
        if align > SMALL_HEAP_LIMIT {
            self.stats.failed_allocations += 1;
            return ptr::null_mut();
        }

        let class_index = SLAB_CLASSES
            .iter()
            .position(|block_size| *block_size >= size.max(align));
        let Some(class_index) = class_index else {
            return self.allocate_large(size, align);
        };

        let block_size = self.slabs[class_index].block_size;
        let offset = if let Some(recycled) = self.slabs[class_index].take_recycled() {
            recycled
        } else {
            let next = self.slabs[class_index].next_bump();
            if next + block_size > self.small_region_limit {
                self.stats.failed_allocations += 1;
                return ptr::null_mut();
            }
            next
        };

        let record = AllocationRecord {
            active: true,
            offset,
            size,
            align,
            kind: AllocationKind::Small(class_index),
        };

        if self.reserve_record(record).is_none() {
            self.stats.failed_allocations += 1;
            return ptr::null_mut();
        }

        self.stats.used_bytes += block_size;
        self.stats.free_bytes = self.stats.total_bytes.saturating_sub(self.stats.used_bytes);
        self.stats.active_allocations += 1;
        unsafe { self.arena.as_mut_ptr().add(offset) }
    }

    fn allocate_large(&mut self, size: usize, align: usize) -> *mut u8 {
        if align > PAGE_SIZE {
            self.stats.failed_allocations += 1;
            return ptr::null_mut();
        }

        let pages = pages_for_bytes(size.max(PAGE_SIZE));
        let Some(start_page) = self.buddy.alloc_pages(pages) else {
            self.stats.failed_allocations += 1;
            return ptr::null_mut();
        };

        let offset = self.small_region_limit + (start_page * PAGE_SIZE);
        if offset + (pages * PAGE_SIZE) > self.arena.len() {
            self.buddy.free_pages(start_page, pages);
            self.stats.failed_allocations += 1;
            return ptr::null_mut();
        }

        let record = AllocationRecord {
            active: true,
            offset,
            size: pages * PAGE_SIZE,
            align,
            kind: AllocationKind::Large,
        };

        if self.reserve_record(record).is_none() {
            self.buddy.free_pages(start_page, pages);
            self.stats.failed_allocations += 1;
            return ptr::null_mut();
        }

        self.stats.used_bytes += pages * PAGE_SIZE;
        self.stats.free_bytes = self.stats.total_bytes.saturating_sub(self.stats.used_bytes);
        self.stats.active_allocations += 1;
        unsafe { self.arena.as_mut_ptr().add(offset) }
    }
}

impl HeapAllocator for BootstrapHeapAllocator {
    fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        if !self.initialized || size == 0 || align == 0 {
            return ptr::null_mut();
        }

        if size <= SMALL_HEAP_LIMIT {
            self.allocate_small(size, align)
        } else {
            self.allocate_large(size, align)
        }
    }

    fn free(&mut self, ptr: *mut u8) -> MemoryResult<()> {
        let record_index = self.find_record(ptr).ok_or(MemoryError::InvalidFree)?;
        let record = &mut self.allocations[record_index];

        if !record.active {
            return Err(MemoryError::DoubleFree);
        }

        match record.kind {
            AllocationKind::Small(class_index) => {
                self.slabs[class_index].recycle(record.offset);
                self.stats.used_bytes = self
                    .stats
                    .used_bytes
                    .saturating_sub(self.slabs[class_index].block_size);
            }
            AllocationKind::Large => {
                let start_page =
                    (record.offset.saturating_sub(self.small_region_limit)) / PAGE_SIZE;
                let pages = pages_for_bytes(record.size);
                self.buddy.free_pages(start_page, pages);
                self.stats.used_bytes = self.stats.used_bytes.saturating_sub(record.size);
            }
        }

        record.active = false;
        self.stats.free_bytes = self.stats.total_bytes.saturating_sub(self.stats.used_bytes);
        self.stats.active_allocations = self.stats.active_allocations.saturating_sub(1);
        Ok(())
    }

    fn realloc(&mut self, ptr: *mut u8, size: usize, align: usize) -> *mut u8 {
        if ptr.is_null() {
            return self.alloc(size, align);
        }

        let Some(record_index) = self.find_record(ptr) else {
            return ptr::null_mut();
        };

        let old_record = self.allocations[record_index];
        let new_ptr = self.alloc(size, align.max(old_record.align));
        if new_ptr.is_null() {
            return ptr::null_mut();
        }

        let copy_len = old_record.size.min(size);
        unsafe {
            ptr::copy_nonoverlapping(ptr as *const u8, new_ptr, copy_len);
        }

        let _ = self.free(ptr);
        new_ptr
    }

    fn stats(&self) -> HeapStats {
        self.stats
    }
}
