pub const PAGE_SIZE: usize = 4096;

/// Maximum physical frames the PMM can track.
///
/// 1 048 576 frames × 4 KiB = 4 GiB addressable physical memory.
/// Each of the three bitmaps (free / allocated / reserved) is 128 KiB,
/// so the entire PMM static fits in ~384 KiB — well within any UEFI
/// identity-mapped region.
pub const MAX_TRACKED_FRAMES: usize = 1_048_576;
pub const FRAME_BITMAP_WORDS: usize = MAX_TRACKED_FRAMES / 64;

pub const MAX_VMM_MAPPINGS: usize = 4096;
pub const MAX_ADDRESS_SPACES: usize = 32;
pub const MAX_ADDRESS_SPACE_MAPPINGS: usize = 512;
pub const MAX_OWNERSHIP_RECORDS: usize = 4096;
pub const EARLY_HEAP_SIZE: usize = 1024 * 1024;
pub const HEAP_PAGE_COUNT: usize = EARLY_HEAP_SIZE / PAGE_SIZE;
pub const MAX_HEAP_ALLOCATIONS: usize = 2048;
pub const MAX_SLAB_RECYCLED_BLOCKS: usize = 128;
pub const SMALL_HEAP_LIMIT: usize = 1024;
pub const USER_SPACE_END: u64 = 0x0000_7fff_ffff_ffff;
pub const KERNEL_SPACE_START: u64 = 0xffff_8000_0000_0000;
pub const FLAG_MASK: u64 = 0xfff | (1u64 << 63);
