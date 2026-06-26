//! Slab allocator for common kernel object sizes — F-MEM-02 fix.
//!
//! Provides O(1) allocation/deallocation for fixed-size objects (32, 64, 128,
//! 256, 512 bytes) using per-size-class free lists backed by 4KB slabs.
//! Falls through to the linked-list heap for other sizes.

use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

/// Number of slab size classes.
const NUM_CLASSES: usize = 5;
/// Object sizes for each class.
const CLASS_SIZES: [usize; NUM_CLASSES] = [32, 64, 128, 256, 512];
/// Slab page size (4 KiB).
const SLAB_PAGE_SIZE: usize = 4096;

/// Per-class free list head (intrusive linked list through free objects).
static FREE_HEADS: [AtomicPtr<u8>; NUM_CLASSES] = [
    AtomicPtr::new(core::ptr::null_mut()),
    AtomicPtr::new(core::ptr::null_mut()),
    AtomicPtr::new(core::ptr::null_mut()),
    AtomicPtr::new(core::ptr::null_mut()),
    AtomicPtr::new(core::ptr::null_mut()),
];

/// Whether the slab allocator is initialized and ready.
static SLAB_READY: AtomicBool = AtomicBool::new(false);

/// Allocation counters per class.
static SLAB_ALLOCS: [AtomicU64; NUM_CLASSES] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
/// Fallback (non-slab) allocation count.
static FALLBACK_ALLOCS: AtomicU64 = AtomicU64::new(0);

/// Initialize the slab allocator by pre-allocating one slab page per class.
/// Must be called after the heap is available.
pub fn init() {
    for (class, &size) in CLASS_SIZES.iter().enumerate() {
        if !grow_class(class) {
            crate::serial_println!("[slab] failed to grow class {} (size={})", class, size);
            return;
        }
    }
    SLAB_READY.store(true, Ordering::Release);
    crate::serial_println!(
        "[slab] initialized classes=[32, 64, 128, 256, 512] objects=[{}, {}, {}, {}, {}]",
        SLAB_PAGE_SIZE / CLASS_SIZES[0],
        SLAB_PAGE_SIZE / CLASS_SIZES[1],
        SLAB_PAGE_SIZE / CLASS_SIZES[2],
        SLAB_PAGE_SIZE / CLASS_SIZES[3],
        SLAB_PAGE_SIZE / CLASS_SIZES[4],
    );
}

/// Allocate one slab page for the given class and chain all objects onto its
/// free list.  Returns false if allocation failed.
fn grow_class(class: usize) -> bool {
    let obj_size = CLASS_SIZES[class];
    let objects_per_slab = SLAB_PAGE_SIZE / obj_size;

    // Allocate a raw 4KB page from the linked-list heap.
    let layout =
        unsafe { core::alloc::Layout::from_size_align_unchecked(SLAB_PAGE_SIZE, SLAB_PAGE_SIZE) };
    let page = unsafe { alloc::alloc::alloc(layout) };
    if page.is_null() {
        return false;
    }

    // Chain all objects in the page into the free list (intrusive next pointer
    // stored at the start of each free object).
    for i in (0..objects_per_slab).rev() {
        let obj = unsafe { page.add(i * obj_size) };
        let old_head = FREE_HEADS[class].load(Ordering::Acquire);
        unsafe { (obj as *mut *mut u8).write(old_head) };
        FREE_HEADS[class].store(obj, Ordering::Release);
    }
    true
}

/// Find the size class for a given layout, if eligible for slab allocation.
#[inline]
fn class_for_layout(layout: core::alloc::Layout) -> Option<usize> {
    if !SLAB_READY.load(Ordering::Acquire) {
        return None;
    }
    let size = layout.size().max(layout.align());
    for (i, &class_size) in CLASS_SIZES.iter().enumerate() {
        if size <= class_size {
            return Some(i);
        }
    }
    None
}

/// Public check: does this layout belong to a slab class?
/// Used by the global allocator's dealloc path.
#[inline]
pub fn class_for_layout_pub(layout: core::alloc::Layout) -> bool {
    class_for_layout(layout).is_some()
}

/// Try to allocate from the slab.  Returns null if no slab class matches or
/// the class free list is exhausted (caller should fall back to heap).
pub unsafe fn slab_alloc(layout: core::alloc::Layout) -> *mut u8 {
    let Some(class) = class_for_layout(layout) else {
        return core::ptr::null_mut();
    };

    // Pop from free list (lock-free CAS loop).
    loop {
        let head = FREE_HEADS[class].load(Ordering::Acquire);
        if head.is_null() {
            // Free list exhausted — try to grow.
            if !grow_class(class) {
                FALLBACK_ALLOCS.fetch_add(1, Ordering::Relaxed);
                return core::ptr::null_mut();
            }
            continue;
        }
        let next = unsafe { (head as *const *mut u8).read() };
        if FREE_HEADS[class]
            .compare_exchange_weak(head, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            SLAB_ALLOCS[class].fetch_add(1, Ordering::Relaxed);
            return head;
        }
    }
}

/// Return an object to the slab free list.
pub unsafe fn slab_dealloc(ptr: *mut u8, layout: core::alloc::Layout) {
    let Some(class) = class_for_layout(layout) else {
        return;
    };
    // Push onto free list (lock-free CAS loop).
    loop {
        let old_head = FREE_HEADS[class].load(Ordering::Acquire);
        unsafe { (ptr as *mut *mut u8).write(old_head) };
        if FREE_HEADS[class]
            .compare_exchange_weak(old_head, ptr, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return;
        }
    }
}

/// Slab allocator statistics.
pub struct SlabStats {
    pub class_allocs: [u64; NUM_CLASSES],
    pub fallback_allocs: u64,
    pub ready: bool,
}

pub fn stats() -> SlabStats {
    SlabStats {
        class_allocs: [
            SLAB_ALLOCS[0].load(Ordering::Relaxed),
            SLAB_ALLOCS[1].load(Ordering::Relaxed),
            SLAB_ALLOCS[2].load(Ordering::Relaxed),
            SLAB_ALLOCS[3].load(Ordering::Relaxed),
            SLAB_ALLOCS[4].load(Ordering::Relaxed),
        ],
        fallback_allocs: FALLBACK_ALLOCS.load(Ordering::Relaxed),
        ready: SLAB_READY.load(Ordering::Relaxed),
    }
}
