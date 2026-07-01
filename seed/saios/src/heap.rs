use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::pmm::{self, HeapStats, PAGE_SIZE};

const HEAP_SIZE_BYTES: usize = 4 * 1024 * 1024; // 4 MiB
const HEAP_PAGES: usize = HEAP_SIZE_BYTES / (PAGE_SIZE as usize);

#[derive(Copy, Clone)]
struct HeapState {
    base: usize,
    current: usize,
    end: usize,
    initialized: bool,
}

impl HeapState {
    const fn new() -> Self {
        Self {
            base: 0,
            current: 0,
            end: 0,
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

        let alloc_start = align_up(state.current, layout.align());
        let alloc_end = alloc_start.saturating_add(layout.size());

        if alloc_end > state.end {
            unlock();
            return core::ptr::null_mut();
        }

        state.current = alloc_end;
        unlock();
        alloc_start as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator v1: no reclaim yet.
    }
}

pub fn init() {
    let heap_base = pmm::alloc_pages(HEAP_PAGES).expect("heap: not enough contiguous pages");

    lock();
    let state = unsafe { &mut *STATE.get() };
    state.base = heap_base as usize;
    state.current = heap_base as usize;
    state.end = (heap_base as usize) + HEAP_SIZE_BYTES;
    state.initialized = true;
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

    let total = state.end - state.base;
    let used = state.current - state.base;
    let free = total - used;

    unlock();
    HeapStats { total, used, free }
}
