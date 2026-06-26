//! SAIOS memory subsystem.
//!
//! # Boot sequence (strict ordering — none of these may allocate before the
//! heap exists):
//!   1. `multiboot::parse(mbi_ptr)`     → fills fixed arrays, no heap needed
//!   2. `memory::init(regions, ks, ke)` → builds the physical frame bitmap
//!   3. `memory::init_heap()`           → carves the kernel heap from frames
//!
//!   After step 3, `alloc` (Vec/String/Box/Arc) is available.
//!
//! # Heap design — dynamically sized, frame-backed
//! Earlier versions used a fixed 256 MiB static BSS array.  That wasted
//! virtual address space and could not adapt to the machine's actual RAM.
//!
//! The heap is now allocated *dynamically* from the physical frame
//! allocator at boot:
//!   heap_size = clamp(usable_RAM / 4, MIN_HEAP, MAX_HEAP)
//!
//! Because `boot.s` (and the UEFI stub) identity-map the first 128 GiB,
//! a physical frame address equals its virtual address in kernel mode, so
//! the heap is directly usable without extra page-table work.

pub mod frame;
pub mod oom;
pub mod paging;
pub mod slab;

use crate::multiboot::MemRegion;
use core::sync::atomic::AtomicU64;
use frame::FrameAllocator;
use spin::Mutex;

// -- Heap sizing constants ---------------------------------------------------

/// Smallest heap we will ever create (machines with very little RAM).
const MIN_HEAP: usize = 16 * 1024 * 1024; // 16 MiB (was 32 MiB)
/// Largest heap we will create up front (rest of RAM stays for user frames).
const MAX_HEAP: usize = 256 * 1024 * 1024; // 256 MiB (was 512 MiB)
/// Fraction of usable RAM to dedicate to the kernel heap (1/DIV).
const HEAP_RAM_DIVISOR: usize = 8; // 12.5% of RAM (was 4 = 25%)

/// Physical address ceiling we can safely use for the heap — must stay within
/// the boot identity-mapped window (128 GiB).
const IDENTITY_MAP_LIMIT: u64 = 128 * 1024 * 1024 * 1024;

// -- Global allocator --------------------------------------------------------

use linked_list_allocator::LockedHeap;

/// Slab-fronted allocator: small allocations go through the lock-free slab cache
/// (O(1) per-size-class free lists); larger allocations fall through to the
/// linked-list heap.  This is the F-MEM-02 fix.
pub struct SaiosAllocator {
    pub heap: LockedHeap,
}

unsafe impl core::alloc::GlobalAlloc for SaiosAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let ptr = unsafe { slab::slab_alloc(layout) };
        if !ptr.is_null() {
            return ptr;
        }
        unsafe { self.heap.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        if slab::class_for_layout_pub(layout) {
            unsafe { slab::slab_dealloc(ptr, layout) };
            return;
        }
        unsafe { self.heap.dealloc(ptr, layout) };
    }
}

#[global_allocator]
pub static ALLOCATOR: SaiosAllocator = SaiosAllocator {
    heap: LockedHeap::empty(),
};

/// Physical/virtual base + current end of the dynamically allocated heap.
/// Used by `heap_grow` to extend the heap with contiguous frames.
static HEAP_BASE: AtomicU64 = AtomicU64::new(0);
static HEAP_END: AtomicU64 = AtomicU64::new(0);

/// Initialise the kernel heap by carving a contiguous region from the
/// physical frame allocator.  MUST be called after `memory::init`.
///
/// The heap size is chosen as a fraction of usable RAM, clamped to
/// `[MIN_HEAP, MAX_HEAP]`.  If RAM is scarce, it falls back to MIN_HEAP and,
/// failing that, to whatever the largest contiguous block available is.
pub fn init_heap() {
    // Decide how big the heap should be based on detected RAM.
    let usable = FRAME_ALLOCATOR.lock().free_bytes();
    let mut heap_size = (usable / HEAP_RAM_DIVISOR).clamp(MIN_HEAP, MAX_HEAP);

    // Round down to a whole number of frames.
    heap_size -= heap_size % frame::FRAME_SIZE;

    // Try to allocate the chosen size; on failure halve repeatedly down to MIN.
    let mut pages = heap_size / frame::FRAME_SIZE;
    let phys = loop {
        if let Some(p) = FRAME_ALLOCATOR.lock().alloc_contiguous(pages) {
            // Must lie fully within the identity-mapped window.
            if p + (pages * frame::FRAME_SIZE) as u64 <= IDENTITY_MAP_LIMIT {
                break p;
            }
            // Outside the map — give the frames back and try a smaller block.
            free_contiguous(p, pages);
        }
        if pages <= MIN_HEAP / frame::FRAME_SIZE {
            panic!(
                "init_heap: cannot allocate even {} MiB heap",
                MIN_HEAP / (1024 * 1024)
            );
        }
        pages /= 2; // halve and retry
    };

    let actual = pages * frame::FRAME_SIZE;
    HEAP_BASE.store(phys, core::sync::atomic::Ordering::SeqCst);
    HEAP_END.store(phys + actual as u64, core::sync::atomic::Ordering::SeqCst);

    // SAFETY: `phys` is a contiguous, frame-allocator-owned region that is
    // identity-mapped (phys == virt) and not used by anything else.
    unsafe {
        ALLOCATOR.heap.lock().init(phys as *mut u8, actual);
    }

    crate::println!(
        "[heap] {} MiB dynamic heap at {:#x}-{:#x} (frame-backed)",
        actual / (1024 * 1024),
        phys,
        phys + actual as u64
    );
}

/// Grow the heap by at least `bytes` more, allocating fresh frames.
///
/// Returns `true` on success. The new region need not be contiguous with
/// the current heap end; the allocator's internal linked-list handles
/// non-contiguous regions.
pub fn heap_grow(bytes: usize) -> bool {
    let pages = bytes.div_ceil(frame::FRAME_SIZE);

    // Allocate frames.
    let phys = match FRAME_ALLOCATOR.lock().alloc_contiguous(pages) {
        Some(p) => p,
        None => return false,
    };

    // Must be within the identity map.
    let region_end = phys + (pages * frame::FRAME_SIZE) as u64;
    if region_end > IDENTITY_MAP_LIMIT {
        free_contiguous(phys, pages);
        return false;
    }

    // SAFETY: region is identity-mapped and owned by the frame allocator.
    unsafe {
        ALLOCATOR.heap.lock().extend(pages * frame::FRAME_SIZE);
    }

    // Update heap end to include the new region using atomic store.
    HEAP_END.store(region_end, core::sync::atomic::Ordering::SeqCst);
    crate::println!("[heap] grew by {} MiB at {:#x}", pages * 4 / 1024, phys);
    true
}

/// Free a run of `pages` contiguous frames starting at `phys`.
fn free_contiguous(phys: u64, pages: usize) {
    let mut fa = FRAME_ALLOCATOR.lock();
    for i in 0..pages {
        fa.free(phys + (i * frame::FRAME_SIZE) as u64);
    }
}

// -- Physical frame allocator ------------------------------------------------

/// Global physical frame allocator (bitmap over all RAM, see `frame.rs`).
pub static FRAME_ALLOCATOR: Mutex<FrameAllocator> = Mutex::new(FrameAllocator::new());

/// Initialise the frame allocator from the (Multiboot2 or UEFI) memory map.
/// `kernel_start`/`kernel_end` are the physical bounds of the loaded kernel
/// image, which are marked reserved so the heap never overlaps the kernel.
pub fn init(regions: &[MemRegion], kernel_start: u64, kernel_end: u64) {
    init_with_reserved(regions, kernel_start, kernel_end, None);
}

pub fn init_with_reserved(
    regions: &[MemRegion],
    kernel_start: u64,
    kernel_end: u64,
    reserved: Option<(u64, u64)>,
) {
    FRAME_ALLOCATOR
        .lock()
        .init(regions, kernel_start, kernel_end, reserved);
    let fa = FRAME_ALLOCATOR.lock();
    let usable_mib = fa.free_bytes() / (1024 * 1024);
    let total_mib = fa.total_bytes() / (1024 * 1024);
    if total_mib >= 1024 {
        crate::println!(
            "[mem] {}.{} GiB usable / {}.{} GiB total ({} frames)",
            usable_mib / 1024,
            (usable_mib % 1024) * 10 / 1024,
            total_mib / 1024,
            (total_mib % 1024) * 10 / 1024,
            fa.total_frames()
        );
    } else {
        crate::println!(
            "[mem] {} MiB usable / {} MiB total ({} frames)",
            usable_mib,
            total_mib,
            fa.total_frames()
        );
    }
}

/// Allocate one physical frame (4 KiB). Returns its physical address.
pub fn alloc_frame() -> Option<u64> {
    crate::memory_contract::MemoryContract::alloc_kernel_frame("alloc_frame")
}

/// Return a previously allocated frame to the allocator.
pub fn free_frame(phys: u64) {
    crate::memory_contract::MemoryContract::free_frame(phys, "free_frame");
}

/// Allocate `n` physically-contiguous frames. Returns the start address.
pub fn alloc_frames(n: usize) -> Option<u64> {
    crate::memory_contract::MemoryContract::alloc_kernel_frames(n, "alloc_frames")
}

/// Return a physically-contiguous frame run to the allocator.
pub fn free_frames(phys: u64, n: usize) {
    crate::memory_contract::MemoryContract::free_frames(phys, n, "free_frames");
}

/// Returns `(total_frames, free_frames, used_frames)`.
pub fn frame_stats() -> (usize, usize, usize) {
    let fa = FRAME_ALLOCATOR.lock();
    (fa.total_frames(), fa.free_frames(), fa.used_frames())
}

/// Returns `(heap_base, heap_end)` physical addresses of the kernel heap.
/// Uses lock-free atomic reads for heap_range().
pub fn heap_range() -> (u64, u64) {
    (
        HEAP_BASE.load(core::sync::atomic::Ordering::SeqCst),
        HEAP_END.load(core::sync::atomic::Ordering::SeqCst),
    )
}
