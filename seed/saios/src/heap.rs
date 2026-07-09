//! Kernel heap allocator.
//!
//! A bump-style allocator that grows by allocating physical pages and mapping
//! them into kernel virtual address space. It serves as the global allocator
//! for the `alloc` crate.

use crate::kernel::constants::{
    HEAP_FALLBACK_INITIAL_BYTES as FALLBACK_INITIAL_HEAP_BYTES,
    HEAP_FALLBACK_MAX_BYTES as FALLBACK_MAX_HEAP_BYTES,
    HEAP_IDENTITY_MAX_PHYS,
    HEAP_INITIAL_BYTES as INITIAL_HEAP_BYTES,
    HEAP_MAX_BYTES as RESERVED_VIRTUAL_HEAP_BYTES,
};

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::pmm::{self, HeapStats, PAGE_SIZE};
/// Small grow step in bytes (2 MiB).
const HEAP_GROW_STEP_SMALL_BYTES: usize = 2 * 1024 * 1024;
/// Large grow step in bytes (4 MiB).
const HEAP_GROW_STEP_LARGE_BYTES: usize = 4 * 1024 * 1024;
const HEAP_GROW_STEP_8M_BYTES: usize = 8 * 1024 * 1024;
const HEAP_GROW_STEP_16M_BYTES: usize = 16 * 1024 * 1024;
const HEAP_GROW_STEP_32M_BYTES: usize = 32 * 1024 * 1024;
/// Maximum number of heap chunks.
const MAX_HEAP_CHUNKS: usize = 512;

#[derive(Copy, Clone)]
struct HeapChunk {
    base: usize,
    current: usize,
    end: usize,
}

impl HeapChunk {
    const fn empty() -> Self {
        Self {
            base: 0,
            current: 0,
            end: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.base == 0 && self.current == 0 && self.end == 0
    }
}

#[derive(Copy, Clone)]
struct HeapState {
    chunks: [HeapChunk; MAX_HEAP_CHUNKS],
    chunk_count: usize,
    active_chunk: usize,
    committed_bytes: usize,
    max_bytes: usize,
    target_bytes: usize,
    initialized: bool,
}

impl HeapState {
    const fn new() -> Self {
        Self {
            chunks: [HeapChunk::empty(); MAX_HEAP_CHUNKS],
            chunk_count: 0,
            active_chunk: 0,
            committed_bytes: 0,
            max_bytes: 0,
            target_bytes: 0,
            initialized: false,
        }
    }
}

static STATE: StaticCell<HeapState> = StaticCell::new(HeapState::new());
static LOCK: AtomicBool = AtomicBool::new(false);
static IDENTITY_HEAP_MAX_PHYS: AtomicU64 = AtomicU64::new(0);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);
static DEALLOC_REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);

// IRQ-safe: the kernel heap allocator is reachable from interrupt context
// (e.g. the timer watchdog's `console::println!` allocates), and is also
// held across VFS operations that can themselves be preempted by a tick.
// See `spinlock_acquire_irqsave`'s doc-comment for the VirtualBox/NEM-only
// crash this class of bug caused.
fn lock() -> bool {
    hal::arch::x86_64::sync::spinlock_acquire_irqsave(&LOCK)
}

fn unlock(was_enabled: bool) {
    hal::arch::x86_64::sync::spinlock_release_irqrestore(&LOCK, was_enabled);
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

fn bytes_to_pages_ceil(bytes: usize) -> usize {
    let page = PAGE_SIZE as usize;
    let with_tail = bytes.saturating_add(page.saturating_sub(1));
    core::cmp::max(1, with_tail / page)
}

fn identity_heap_max_phys() -> Option<u64> {
    match IDENTITY_HEAP_MAX_PHYS.load(Ordering::Relaxed) {
        0 => None,
        value => Some(value),
    }
}

pub fn configure_identity_mode(max_phys: Option<u64>) {
    IDENTITY_HEAP_MAX_PHYS.store(max_phys.unwrap_or(0), Ordering::Relaxed);
}

pub fn identity_mode_enabled() -> bool {
    IDENTITY_HEAP_MAX_PHYS.load(Ordering::Relaxed) != 0
}

pub fn dynamic_mappings_available() -> bool {
    !identity_mode_enabled()
}

fn alloc_best_effort_pages(max_pages: usize) -> Option<(usize, usize)> {
    let mut pages = max_pages;
    while pages > 0 {
        let allocation = if let Some(max_phys) = identity_heap_max_phys() {
            pmm::alloc_pages_below(pages, max_phys)
        } else {
            pmm::alloc_pages_below(pages, HEAP_IDENTITY_MAX_PHYS)
        };

        if let Some(base) = allocation {
            return Some((base as usize, pages));
        }
        pages /= 2;
    }
    None
}

fn add_chunk(state: &mut HeapState, base: usize, bytes: usize) -> bool {
    if state.chunk_count >= MAX_HEAP_CHUNKS || bytes == 0 {
        return false;
    }

    if state.committed_bytes >= state.max_bytes {
        return false;
    }

    let available = state.max_bytes.saturating_sub(state.committed_bytes);
    let bytes = core::cmp::min(bytes, available);
    if bytes == 0 {
        return false;
    }

    let idx = state.chunk_count;
    state.chunks[idx] = HeapChunk {
        base,
        current: base,
        end: base.saturating_add(bytes),
    };
    state.chunk_count = idx + 1;
    state.committed_bytes = state.committed_bytes.saturating_add(bytes);
    true
}

fn grow_heap_locked(state: &mut HeapState, desired_bytes: usize, growth_step_bytes: usize) {
    let desired = core::cmp::min(desired_bytes, state.max_bytes);
    while state.committed_bytes < desired && state.chunk_count < MAX_HEAP_CHUNKS {
        let remaining = desired.saturating_sub(state.committed_bytes);
        let request_bytes = core::cmp::min(remaining, growth_step_bytes);
        let request_pages = bytes_to_pages_ceil(request_bytes);

        let Some((base, pages)) = alloc_best_effort_pages(request_pages) else {
            break;
        };

        let bytes = pages.saturating_mul(PAGE_SIZE as usize);
        if !add_chunk(state, base, bytes) {
            break;
        }
    }
}

fn try_alloc_from_chunks(state: &mut HeapState, layout: Layout) -> Option<usize> {
    if state.chunk_count == 0 {
        return None;
    }

    for i in 0..state.chunk_count {
        let idx = (state.active_chunk + i) % state.chunk_count;
        let chunk = state.chunks[idx];
        if chunk.is_empty() {
            continue;
        }

        let alloc_start = align_up(chunk.current, layout.align());
        let alloc_end = alloc_start.saturating_add(layout.size());
        if alloc_end <= chunk.end {
            state.chunks[idx].current = alloc_end;
            state.active_chunk = idx;
            return Some(alloc_start);
        }
    }

    None
}

fn compute_used_bytes(state: &HeapState) -> usize {
    let mut used = 0usize;
    for i in 0..state.chunk_count {
        let c = state.chunks[i];
        used = used.saturating_add(c.current.saturating_sub(c.base));
    }
    used
}

fn growth_request_bytes_for_layout(layout: Layout) -> usize {
    let needed = core::cmp::max(layout.size(), layout.align());
    if needed <= HEAP_GROW_STEP_SMALL_BYTES {
        HEAP_GROW_STEP_SMALL_BYTES
    } else if needed <= HEAP_GROW_STEP_LARGE_BYTES {
        HEAP_GROW_STEP_LARGE_BYTES
    } else if needed <= HEAP_GROW_STEP_8M_BYTES {
        HEAP_GROW_STEP_8M_BYTES
    } else if needed <= HEAP_GROW_STEP_16M_BYTES {
        HEAP_GROW_STEP_16M_BYTES
    } else if needed <= HEAP_GROW_STEP_32M_BYTES {
        HEAP_GROW_STEP_32M_BYTES
    } else {
        needed.saturating_add(HEAP_GROW_STEP_SMALL_BYTES)
    }
}

/// Global allocator used by the kernel.
pub struct KernelHeapAllocator;

unsafe impl GlobalAlloc for KernelHeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::NonNull::<u8>::dangling().as_ptr();
        }

        ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOC_REQUESTED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);

        let was_enabled = lock();
        let state = unsafe { &mut *STATE.get() };

        if !state.initialized {
            unlock(was_enabled);
            return core::ptr::null_mut();
        }

        if let Some(ptr) = try_alloc_from_chunks(state, layout) {
            unlock(was_enabled);
            return ptr as *mut u8;
        }

        // Grow as needed up to target (25% RAM), then retry once.
        if state.committed_bytes < state.target_bytes {
            let min_extra = growth_request_bytes_for_layout(layout);
            let desired = core::cmp::min(
                state.target_bytes,
                state.committed_bytes.saturating_add(min_extra),
            );
            grow_heap_locked(state, desired, min_extra);
        }

        let ptr = try_alloc_from_chunks(state, layout)
            .map(|p| p as *mut u8)
            .unwrap_or(core::ptr::null_mut());

        unlock(was_enabled);
        ptr
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator v1: deallocation is a no-op because individual
        // allocations are not tracked.
        DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        DEALLOC_REQUESTED_BYTES.fetch_add(_layout.size() as u64, Ordering::Relaxed);
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct LeakStats {
    pub alloc_calls: u64,
    pub dealloc_calls: u64,
    pub alloc_requested_bytes: u64,
    pub dealloc_requested_bytes: u64,
    pub outstanding_requested_bytes: u64,
}

/// Initializes the kernel heap.
pub fn init() {
    let was_enabled = lock();
    let state = unsafe { &mut *STATE.get() };

    state.chunks = [HeapChunk::empty(); MAX_HEAP_CHUNKS];
    state.chunk_count = 0;
    state.active_chunk = 0;
    state.committed_bytes = 0;
    state.max_bytes = 0;
    state.target_bytes = 0;
    state.initialized = false;

    let total_ram_bytes = pmm::total_pages().saturating_mul(PAGE_SIZE as usize);
    let fallback_mode = identity_heap_max_phys().is_some();
    state.max_bytes = if fallback_mode {
        core::cmp::min(total_ram_bytes, FALLBACK_MAX_HEAP_BYTES)
    } else {
        core::cmp::min(total_ram_bytes, RESERVED_VIRTUAL_HEAP_BYTES)
    };
    state.target_bytes = state.max_bytes;

    // Bring heap to the initial mapped size first.
    let initial = if fallback_mode {
        core::cmp::min(FALLBACK_INITIAL_HEAP_BYTES, state.max_bytes)
    } else {
        core::cmp::min(INITIAL_HEAP_BYTES, state.max_bytes)
    };
    grow_heap_locked(state, initial, HEAP_GROW_STEP_LARGE_BYTES);

    state.initialized = state.chunk_count > 0;
    assert!(
        state.initialized,
        "heap: failed to allocate initial heap pages"
    );
    unlock(was_enabled);
}

/// Returns current heap usage statistics.
pub fn stats() -> HeapStats {
    let was_enabled = lock();
    let state = unsafe { &*STATE.get() };

    if !state.initialized {
        unlock(was_enabled);
        return HeapStats {
            total: 0,
            used: 0,
            free: 0,
        };
    }

    let total = state.committed_bytes;
    let used = compute_used_bytes(state);
    let free = total.saturating_sub(used);

    unlock(was_enabled);
    HeapStats { total, used, free }
}

pub fn leak_stats() -> LeakStats {
    let alloc_calls = ALLOC_CALLS.load(Ordering::Relaxed);
    let dealloc_calls = DEALLOC_CALLS.load(Ordering::Relaxed);
    let alloc_requested_bytes = ALLOC_REQUESTED_BYTES.load(Ordering::Relaxed);
    let dealloc_requested_bytes = DEALLOC_REQUESTED_BYTES.load(Ordering::Relaxed);
    let outstanding_requested_bytes = alloc_requested_bytes.saturating_sub(dealloc_requested_bytes);

    LeakStats {
        alloc_calls,
        dealloc_calls,
        alloc_requested_bytes,
        dealloc_requested_bytes,
        outstanding_requested_bytes,
    }
}
