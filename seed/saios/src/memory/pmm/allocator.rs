use core::cell::UnsafeCell;

use efi_main::memorymap::MemoryType;

use crate::memory::constants::{MAX_TRACKED_FRAMES, PAGE_SIZE};
use crate::memory::errors::{MemoryError, MemoryResult};
use crate::memory::pmm::PhysicalMemoryManager;
use crate::memory::pmm::bitmap::FrameBitmap;
use crate::memory::pmm::statistics::PhysicalMemoryStats;
use crate::memory::types::{BootMemoryMapView, PhysAddr, PhysAddrExt, PhysicalFrame};

struct GlobalPmm(UnsafeCell<BitmapPhysicalMemoryManager>);

unsafe impl Sync for GlobalPmm {}

static PMM: GlobalPmm = GlobalPmm(UnsafeCell::new(BitmapPhysicalMemoryManager::new()));

pub fn init(memory_map: &BootMemoryMapView<'_>) -> MemoryResult<()> {
    manager().init(memory_map)
}

pub fn alloc_frame() -> MemoryResult<PhysicalFrame> {
    manager().alloc_frame()
}

pub fn free_frame(frame: PhysicalFrame) -> MemoryResult<()> {
    manager().free_frame(frame)
}

pub fn reserve(start: PhysAddr, size: usize) -> MemoryResult<()> {
    manager().reserve(start, size)
}

pub fn total_memory() -> usize {
    manager().total_memory()
}

pub fn free_memory() -> usize {
    manager().free_memory()
}

fn manager() -> &'static mut BitmapPhysicalMemoryManager {
    unsafe { &mut *PMM.0.get() }
}

pub struct BitmapPhysicalMemoryManager {
    initialized: bool,
    highest_frame: usize,
    next_search: usize,
    free: FrameBitmap,
    allocated: FrameBitmap,
    reserved: FrameBitmap,
    stats: PhysicalMemoryStats,
}

impl BitmapPhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            highest_frame: 0,
            next_search: 0,
            free: FrameBitmap::new(),
            allocated: FrameBitmap::new(),
            reserved: FrameBitmap::new(),
            stats: PhysicalMemoryStats::empty(),
        }
    }

    fn validate_frame(&self, frame: PhysicalFrame) -> MemoryResult<()> {
        if !self.initialized {
            return Err(MemoryError::NotInitialized);
        }

        if frame.number() > self.highest_frame {
            return Err(MemoryError::FrameOutOfRange);
        }

        Ok(())
    }

    /// Track a physical memory region in the bitmaps.
    ///
    /// Only frames within `[0, MAX_TRACKED_FRAMES)` are tracked; frames
    /// beyond that bound are silently ignored (the system simply won't
    /// use that memory).  This avoids `TooManyFrames` errors caused by
    /// high-address MMIO / reserved regions that happen to sit above the
    /// tracked window.
    fn track_region(
        &mut self,
        start: u64,
        length: u64,
        free: bool,
        reserve_only: bool,
    ) {
        let start_frame = (start as usize).saturating_div(PAGE_SIZE);
        let end_frame = (start as usize)
            .saturating_add(length as usize)
            .saturating_add(PAGE_SIZE - 1)
            .saturating_div(PAGE_SIZE);

        // Clamp to the tracked window — anything above is ignored.
        let first = start_frame.min(MAX_TRACKED_FRAMES);
        let last = end_frame.min(MAX_TRACKED_FRAMES);

        if last > self.highest_frame {
            self.highest_frame = last;
        }

        for frame_number in first..last {
            if free && !self.free.is_set(frame_number) {
                self.free.set(frame_number).ok();
                self.stats.free_bytes += PAGE_SIZE;
            }

            if reserve_only && !self.reserved.is_set(frame_number) {
                self.reserved.set(frame_number).ok();
                self.stats.reserved_bytes += PAGE_SIZE;
            }
        }
    }
}

impl PhysicalMemoryManager for BitmapPhysicalMemoryManager {
    fn init(&mut self, memory_map: &BootMemoryMapView<'_>) -> MemoryResult<()> {
        if self.initialized {
            return Err(MemoryError::AlreadyInitialized);
        }

        self.free.clear_all();
        self.allocated.clear_all();
        self.reserved.clear_all();
        self.stats = PhysicalMemoryStats::empty();

        for entry in memory_map.entries() {
            // Only RAM-type regions participate in frame tracking.
            // MMIO, ACPI, reserved, and other non-RAM regions are
            // skipped — they sit at high physical addresses that
            // would otherwise overflow the fixed-size bitmap.
            let is_ram = matches!(
                entry.region_type,
                MemoryType::Usable
                    | MemoryType::Reclaimable
                    | MemoryType::Loader
                    | MemoryType::Seed
            );

            if !is_ram {
                continue;
            }

            let is_free = matches!(
                entry.region_type,
                MemoryType::Usable | MemoryType::Reclaimable
            );

            self.stats.total_bytes += entry.length as usize;

            // reserve_only = true for Loader / Seed (kernel, boot info, etc.)
            self.track_region(entry.base, entry.length, is_free, !is_free);
        }

        self.initialized = true;
        Ok(())
    }

    fn alloc_frame(&mut self) -> MemoryResult<PhysicalFrame> {
        if !self.initialized {
            return Err(MemoryError::NotInitialized);
        }

        let search_limit = self.highest_frame.min(MAX_TRACKED_FRAMES);
        if search_limit == 0 {
            return Err(MemoryError::OutOfFrames);
        }

        for offset in 0..search_limit {
            let frame_number = (self.next_search + offset) % search_limit;
            if self.free.is_set(frame_number)
                && !self.allocated.is_set(frame_number)
                && !self.reserved.is_set(frame_number)
            {
                self.free.clear(frame_number).ok();
                self.allocated.set(frame_number).ok();
                self.stats.free_bytes = self.stats.free_bytes.saturating_sub(PAGE_SIZE);
                self.stats.allocated_frames += 1;
                self.next_search = frame_number + 1;
                return Ok(PhysicalFrame::new(frame_number));
            }
        }

        Err(MemoryError::OutOfFrames)
    }

    fn free_frame(&mut self, frame: PhysicalFrame) -> MemoryResult<()> {
        self.validate_frame(frame)?;

        if !self.allocated.is_set(frame.number()) {
            return Err(MemoryError::DoubleFree);
        }

        self.allocated.clear(frame.number()).ok();
        self.free.set(frame.number()).ok();
        self.stats.free_bytes += PAGE_SIZE;
        self.stats.allocated_frames = self.stats.allocated_frames.saturating_sub(1);
        self.next_search = frame.number();
        Ok(())
    }

    fn reserve(&mut self, start: PhysAddr, size: usize) -> MemoryResult<()> {
        if !start.is_page_aligned() {
            return Err(MemoryError::AddressMisaligned);
        }

        let start_frame = start.as_u64() as usize / PAGE_SIZE;
        let end_frame = start_frame + (size + (PAGE_SIZE - 1)) / PAGE_SIZE;

        if end_frame > MAX_TRACKED_FRAMES {
            return Err(MemoryError::TooManyFrames);
        }

        for frame_number in start_frame..end_frame {
            if self.allocated.is_set(frame_number) {
                return Err(MemoryError::OwnershipConflict);
            }

            if self.free.is_set(frame_number) {
                self.free.clear(frame_number).ok();
                self.stats.free_bytes = self.stats.free_bytes.saturating_sub(PAGE_SIZE);
            }

            if !self.reserved.is_set(frame_number) {
                self.reserved.set(frame_number).ok();
                self.stats.reserved_bytes += PAGE_SIZE;
            }
        }

        Ok(())
    }

    fn total_memory(&self) -> usize {
        self.stats.total_bytes
    }

    fn free_memory(&self) -> usize {
        self.stats.free_bytes
    }
}
