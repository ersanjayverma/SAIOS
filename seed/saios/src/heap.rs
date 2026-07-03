use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::pmm::{self, HeapStats, PAGE_SIZE};

const INITIAL_HEAP_BYTES: usize = 32 * 1024 * 1024; // 32 MiB initial mapped heap
const RESERVED_VIRTUAL_HEAP_BYTES: usize = 1024 * 1024 * 1024; // 1 GiB virtual heap budget
const HEAP_GROW_STEP_SMALL_BYTES: usize = 2 * 1024 * 1024; // 2 MiB
const HEAP_GROW_STEP_LARGE_BYTES: usize = 4 * 1024 * 1024; // 4 MiB
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

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
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

fn alloc_best_effort_pages(max_pages: usize) -> Option<(usize, usize)> {
    let mut pages = max_pages;
    while pages > 0 {
        if let Some(base) = pmm::alloc_pages(pages) {
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

pub struct KernelHeapAllocator;

unsafe impl GlobalAlloc for KernelHeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return core::ptr::NonNull::<u8>::dangling().as_ptr();
        }

        lock();
        let state = unsafe { &mut *STATE.get() };

        if !state.initialized {
            unlock();
            return core::ptr::null_mut();
        }

        if let Some(ptr) = try_alloc_from_chunks(state, layout) {
            unlock();
            return ptr as *mut u8;
        }

        // Grow as needed up to target (25% RAM), then retry once.
        if state.committed_bytes < state.target_bytes {
            let growth_step = if layout.size() > HEAP_GROW_STEP_SMALL_BYTES {
                HEAP_GROW_STEP_LARGE_BYTES
            } else {
                HEAP_GROW_STEP_SMALL_BYTES
            };
            let min_extra = core::cmp::max(layout.size(), growth_step);
            let desired = core::cmp::min(
                state.target_bytes,
                state.committed_bytes.saturating_add(min_extra),
            );
            grow_heap_locked(state, desired, growth_step);
        }

        let ptr = try_alloc_from_chunks(state, layout)
            .map(|p| p as *mut u8)
            .unwrap_or(core::ptr::null_mut());

        unlock();
        ptr
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator v1: no reclaim yet.
    }
}

pub fn init() {
    lock();
    let state = unsafe { &mut *STATE.get() };

    state.chunks = [HeapChunk::empty(); MAX_HEAP_CHUNKS];
    state.chunk_count = 0;
    state.active_chunk = 0;
    state.committed_bytes = 0;
    state.max_bytes = 0;
    state.target_bytes = 0;
    state.initialized = false;

    let total_ram_bytes = pmm::total_pages().saturating_mul(PAGE_SIZE as usize);
    state.max_bytes = core::cmp::min(total_ram_bytes, RESERVED_VIRTUAL_HEAP_BYTES);
    state.target_bytes = state.max_bytes;

    // Bring heap to the initial mapped size first.
    let initial = core::cmp::min(INITIAL_HEAP_BYTES, state.max_bytes);
    grow_heap_locked(state, initial, HEAP_GROW_STEP_LARGE_BYTES);

    state.initialized = state.chunk_count > 0;
    assert!(
        state.initialized,
        "heap: failed to allocate initial heap pages"
    );
    unlock();
}

pub fn stats() -> HeapStats {
    lock();
    let state = unsafe { &*STATE.get() };

    if !state.initialized {
        unlock();
        return HeapStats {
            total: 0,
            used: 0,
            free: 0,
        };
    }

    let total = state.committed_bytes;
    let used = compute_used_bytes(state);
    let free = total.saturating_sub(used);

    unlock();
    HeapStats { total, used, free }
}
