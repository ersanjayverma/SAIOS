//! Physical frame allocator - bitmap over all RAM reported by Multiboot2.
//!
//! A "frame" is one 4 KiB page-aligned chunk of physical memory.
//! The bitmap stores 1 bit per frame: 0 = free, 1 = used.
//!
//! # Capacity
//! MAX_FRAMES = 32 M frames × 4 KiB = **128 GiB** maximum addressable RAM.
//! Bitmap size = 32 M / 8 = **4 MiB** in `.bss` - acceptable overhead.
//!
//! The identity map in `boot.s` also covers 128 GiB (128 PD tables ×
//! 512 × 2 MiB pages), so physical addresses up to 128 GiB are directly
//! accessible at the same virtual address in kernel mode.

use crate::multiboot::{MMAP_AVAILABLE, MemRegion};

pub const FRAME_SIZE: usize = 4096;

/// Maximum number of physical frames tracked.
/// 32 M frames × 4 KiB = 128 GiB.  Increase to 64 M (256 GiB) if needed.
const MAX_FRAMES: usize = 32 * 1024 * 1024; // 128 GiB / 4 KiB
const BITMAP_WORDS: usize = MAX_FRAMES / 64; // 512 K × u64 = 4 MiB

pub struct FrameAllocator {
    bitmap: [u64; BITMAP_WORDS],
    total: usize,      // total usable frames
    free: usize,       // currently free frames
    search_ptr: usize, // word index hint for next alloc
}

impl FrameAllocator {
    pub const fn new() -> Self {
        Self {
            bitmap: [u64::MAX; BITMAP_WORDS], // all marked used until init
            total: 0,
            free: 0,
            search_ptr: 0,
        }
    }

    /// Initialise from the Multiboot2 memory map.
    /// Marks available regions free, then re-marks the kernel image as used.
    pub fn init(
        &mut self,
        regions: &[MemRegion],
        kernel_start: u64,
        kernel_end: u64,
        reserved: Option<(u64, u64)>,
    ) {
        // Step 1: free every available region
        for r in regions {
            if r.kind != MMAP_AVAILABLE {
                continue;
            }
            let start_frame = align_up(r.base, FRAME_SIZE as u64) / FRAME_SIZE as u64;
            let end_frame = align_down(r.base + r.len, FRAME_SIZE as u64) / FRAME_SIZE as u64;
            for frame in start_frame..end_frame {
                if (frame as usize) < MAX_FRAMES {
                    self.clear_bit(frame as usize);
                    self.total += 1;
                    self.free += 1;
                }
            }
        }

        // Step 2: frame 0 always reserved (null pointer trap)
        self.set_bit(0);
        if self.free > 0 {
            self.free -= 1;
        }

        // Step 3: re-mark the kernel image as used
        let ks = align_down(kernel_start, FRAME_SIZE as u64) / FRAME_SIZE as u64;
        let ke = align_up(kernel_end, FRAME_SIZE as u64) / FRAME_SIZE as u64;
        for frame in ks..ke {
            if (frame as usize) < MAX_FRAMES && !self.test_bit(frame as usize) {
                self.set_bit(frame as usize);
                if self.free > 0 {
                    self.free -= 1;
                }
            }
        }

        if let Some((base, len)) = reserved {
            self.reserve_range(base, len);
        }
    }

    pub fn reserve_range(&mut self, base: u64, len: u64) {
        let start = align_down(base, FRAME_SIZE as u64) / FRAME_SIZE as u64;
        let end = align_up(base.saturating_add(len), FRAME_SIZE as u64) / FRAME_SIZE as u64;
        for frame in start..end {
            if (frame as usize) < MAX_FRAMES && !self.test_bit(frame as usize) {
                self.set_bit(frame as usize);
                if self.free > 0 {
                    self.free -= 1;
                }
            }
        }
    }

    /// Allocate one physical frame. Returns its physical address, or None if OOM.
    pub fn alloc(&mut self) -> Option<u64> {
        let start = self.search_ptr;
        let mut w = start;
        loop {
            if self.bitmap[w] != u64::MAX {
                // There's a free bit in this word
                let bit = (!self.bitmap[w]).trailing_zeros() as usize;
                let frame = w * 64 + bit;
                // Guard against a frame index past the tracked maximum.
                if frame >= MAX_FRAMES {
                    return None;
                }
                self.set_bit(frame);
                self.free = self.free.saturating_sub(1);
                self.search_ptr = w;
                return Some((frame * FRAME_SIZE) as u64);
            }
            w = (w + 1) % BITMAP_WORDS;
            if w == start {
                return None;
            } // full lap - OOM
        }
    }

    /// Free a previously allocated frame.
    pub fn free(&mut self, phys: u64) {
        let frame = (phys / FRAME_SIZE as u64) as usize;
        if frame < MAX_FRAMES && self.test_bit(frame) {
            self.clear_bit(frame);
            self.free += 1;
            if frame / 64 < self.search_ptr {
                self.search_ptr = frame / 64;
            }
        }
    }

    /// Allocate `n` contiguous physical frames. Returns start address or None.
    pub fn alloc_contiguous(&mut self, n: usize) -> Option<u64> {
        if n == 0 {
            return Some(0);
        }
        let mut start = 1usize; // skip frame 0
        'outer: loop {
            // Overflow check: start + n must not overflow usize
            if start.checked_add(n)?.gt(&MAX_FRAMES) {
                return None;
            }
            for i in 0..n {
                if self.test_bit(start + i) {
                    start = start + i + 1;
                    continue 'outer;
                }
            }
            // Found n free contiguous frames
            // Check for overflow when computing physical address
            let frame_bytes = start.checked_mul(FRAME_SIZE)?;
            for i in 0..n {
                self.set_bit(start + i);
                self.free = self.free.saturating_sub(1);
            }
            return Some(frame_bytes as u64);
        }
    }

    pub fn total_frames(&self) -> usize {
        self.total
    }
    pub fn free_frames(&self) -> usize {
        self.free
    }
    pub fn used_frames(&self) -> usize {
        self.total.saturating_sub(self.free)
    }

    pub fn total_bytes(&self) -> usize {
        self.total * FRAME_SIZE
    }
    pub fn free_bytes(&self) -> usize {
        self.free * FRAME_SIZE
    }
    pub fn used_bytes(&self) -> usize {
        self.used_frames() * FRAME_SIZE
    }

    // -- bitmap helpers ----------------------------------------------------

    fn set_bit(&mut self, frame: usize) {
        self.bitmap[frame / 64] |= 1u64 << (frame % 64);
    }
    fn clear_bit(&mut self, frame: usize) {
        self.bitmap[frame / 64] &= !(1u64 << (frame % 64));
    }
    fn test_bit(&self, frame: usize) -> bool {
        self.bitmap[frame / 64] & (1u64 << (frame % 64)) != 0
    }
}

fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}
fn align_down(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}
