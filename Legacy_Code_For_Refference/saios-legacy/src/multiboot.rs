//! Multiboot2 info parser — uses ZERO heap allocations.
//! Must be callable before the heap is initialised.

use spin::Mutex;

/// Cached copy of the boot memory map — stored after init_heap() so meminfo can read it.
pub static CACHED_REGIONS: Mutex<[MemRegion; MAX_MEM_REGIONS]> = Mutex::new(
    [MemRegion {
        base: 0,
        len: 0,
        kind: 0,
    }; MAX_MEM_REGIONS],
);
pub static CACHED_REGION_COUNT: spin::Mutex<usize> = spin::Mutex::new(0);

pub const MMAP_AVAILABLE: u32 = 1;
pub const MMAP_RESERVED: u32 = 2;
pub const MMAP_ACPI: u32 = 3;
pub const MMAP_NVS: u32 = 4;
pub const MMAP_BADRAM: u32 = 5;

pub const MAX_MEM_REGIONS: usize = 32;
pub const MAX_CMDLINE: usize = 256;

#[derive(Debug, Clone, Copy, Default)]
pub struct MemRegion {
    pub base: u64,
    pub len: u64,
    pub kind: u32,
}

/// Framebuffer info from the Multiboot2 framebuffer tag (type 8).
/// `addr == 0` means GRUB did not provide a framebuffer (text mode).
#[derive(Debug, Clone, Copy, Default)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
}

/// All boot info stored in plain arrays — no heap.
pub struct BootInfo {
    pub cmdline: [u8; MAX_CMDLINE],
    pub cmdline_len: usize,
    pub mem_regions: [MemRegion; MAX_MEM_REGIONS],
    pub mem_region_count: usize,
    pub total_mem: u64,
    pub framebuffer: FramebufferInfo,
}

impl BootInfo {
    pub const fn empty() -> Self {
        Self {
            cmdline: [0u8; MAX_CMDLINE],
            cmdline_len: 0,
            mem_regions: [MemRegion {
                base: 0,
                len: 0,
                kind: 0,
            }; MAX_MEM_REGIONS],
            mem_region_count: 0,
            total_mem: 0,
            framebuffer: FramebufferInfo {
                addr: 0,
                pitch: 0,
                width: 0,
                height: 0,
                bpp: 0,
            },
        }
    }

    pub fn cmdline_str(&self) -> &str {
        core::str::from_utf8(&self.cmdline[..self.cmdline_len]).unwrap_or("")
    }

    pub fn regions(&self) -> &[MemRegion] {
        &self.mem_regions[..self.mem_region_count]
    }
}

// Tag types
const TAG_END: u32 = 0;
const TAG_CMDLINE: u32 = 1;
const TAG_MMAP: u32 = 6;
const TAG_FRAMEBUFFER: u32 = 8;

/// Parse the Multiboot2 information structure.
/// No allocations — completely safe to call before heap init.
/// # Safety
/// `mbi_ptr` must be the value left in %rbx by GRUB.
pub unsafe fn parse(mbi_ptr: u64) -> BootInfo {
    unsafe {
        let mut info = BootInfo::empty();
        if mbi_ptr == 0 {
            return info;
        }

        let mut offset = 8u64; // skip total_size + reserved

        loop {
            let tag_ptr = (mbi_ptr + offset) as *const u32;
            let tag_type = core::ptr::read_unaligned(tag_ptr);
            let tag_size = core::ptr::read_unaligned(tag_ptr.add(1));

            match tag_type {
                TAG_END => break,

                TAG_CMDLINE => {
                    let str_ptr = (mbi_ptr + offset + 8) as *const u8;
                    let max_len = (tag_size as usize).saturating_sub(8).min(MAX_CMDLINE);
                    let mut len = 0usize;
                    while len < max_len && *str_ptr.add(len) != 0 {
                        len += 1;
                    }
                    let src = core::slice::from_raw_parts(str_ptr, len);
                    info.cmdline[..len].copy_from_slice(src);
                    info.cmdline_len = len;
                }

                TAG_MMAP => {
                    let entry_size =
                        core::ptr::read_unaligned((mbi_ptr + offset + 8) as *const u32) as u64;
                    let entries_start = mbi_ptr + offset + 16;
                    let entries_end = mbi_ptr + offset + tag_size as u64;
                    let mut ptr = entries_start;

                    while ptr + entry_size <= entries_end {
                        let base = core::ptr::read_unaligned(ptr as *const u64);
                        let len = core::ptr::read_unaligned((ptr + 8) as *const u64);
                        let kind = core::ptr::read_unaligned((ptr + 16) as *const u32);

                        if kind == MMAP_AVAILABLE {
                            info.total_mem += len;
                        }

                        if info.mem_region_count < MAX_MEM_REGIONS {
                            info.mem_regions[info.mem_region_count] = MemRegion { base, len, kind };
                            info.mem_region_count += 1;
                        }
                        ptr += entry_size;
                    }
                }

                TAG_FRAMEBUFFER => {
                    // Layout: type(4) size(4) addr(8) pitch(4) width(4) height(4)
                    //         bpp(1) fb_type(1) reserved(2) ...
                    let base = mbi_ptr + offset;
                    info.framebuffer = FramebufferInfo {
                        addr: core::ptr::read_unaligned((base + 8) as *const u64),
                        pitch: core::ptr::read_unaligned((base + 16) as *const u32),
                        width: core::ptr::read_unaligned((base + 20) as *const u32),
                        height: core::ptr::read_unaligned((base + 24) as *const u32),
                        bpp: core::ptr::read_unaligned((base + 28) as *const u8),
                    };
                }

                _ => {}
            }

            let aligned = (tag_size as u64 + 7) & !7;
            offset += aligned;
            if aligned == 0 {
                break;
            } // safety guard
        }

        info
    }
}
