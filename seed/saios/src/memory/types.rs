use core::slice;

use efi_main::memorymap::{MemoryMapInfo, MemoryRegion};
pub use hal::memory::{PhysAddr, VirtAddr};

use crate::memory::constants::PAGE_SIZE;
use crate::memory::errors::{MemoryError, MemoryResult};

pub trait PhysAddrExt {
    fn is_page_aligned(self) -> bool;
    fn align_down(self) -> Self;
    fn align_up(self) -> Self;
    fn page_number(self) -> usize;
}

impl PhysAddrExt for PhysAddr {
    fn is_page_aligned(self) -> bool {
        self.as_u64() as usize % PAGE_SIZE == 0
    }

    fn align_down(self) -> Self {
        Self::new(self.as_u64() & !((PAGE_SIZE as u64) - 1))
    }
    fn align_up(self) -> Self {
        if self.is_page_aligned() {
            self
        } else {
            Self::new((self.as_u64() & !((PAGE_SIZE as u64) - 1)) + PAGE_SIZE as u64)
        }
    }
    fn page_number(self) -> usize {
        self.as_u64() as usize / PAGE_SIZE
    }
}

pub trait VirtAddrExt {
    fn is_page_aligned(self) -> bool;
    fn align_down(self) -> Self;
    fn align_up(self) -> Self;
    fn page_number(self) -> usize;
    fn page_offset(self) -> usize;
    fn pml4_index(self) -> usize;

    fn pdpt_index(self) -> usize;

    fn pd_index(self) -> usize;

    fn pt_index(self) -> usize;
}

impl VirtAddrExt for VirtAddr {
    fn is_page_aligned(self) -> bool {
        self.as_u64() as usize % PAGE_SIZE == 0
    }

    fn align_down(self) -> Self {
        Self::new(self.as_u64() & !((PAGE_SIZE as u64) - 1))
    }
    fn align_up(self) -> Self {
        if self.is_page_aligned() {
            self
        } else {
            Self::new((self.as_u64() & !((PAGE_SIZE as u64) - 1)) + PAGE_SIZE as u64)
        }
    }

    fn page_offset(self) -> usize {
        self.as_u64() as usize & (PAGE_SIZE - 1)
    }
    fn page_number(self) -> usize {
        self.as_u64() as usize / PAGE_SIZE
    }
    fn pml4_index(self) -> usize {
        ((self.as_u64() >> 39) & 0x1ff) as usize
    }
    fn pdpt_index(self) -> usize {
        ((self.as_u64() >> 30) & 0x1ff) as usize
    }
    fn pd_index(self) -> usize {
        ((self.as_u64() >> 21) & 0x1ff) as usize
    }
    fn pt_index(self) -> usize {
        ((self.as_u64() >> 12) & 0x1ff) as usize
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PhysicalFrame {
    number: usize,
}

impl PhysicalFrame {
    pub const fn new(number: usize) -> Self {
        Self { number }
    }

    pub fn from_start_address(start: PhysAddr) -> MemoryResult<Self> {
        if !start.is_page_aligned() {
            return Err(MemoryError::AddressMisaligned);
        }

        Ok(Self {
            number: start.as_u64() as usize / PAGE_SIZE,
        })
    }

    pub const fn number(self) -> usize {
        self.number
    }

    pub const fn start_address(self) -> PhysAddr {
        PhysAddr::new((self.number * PAGE_SIZE) as u64)
    }

    pub fn containing(addr: PhysAddr) -> Self {
        Self {
            number: addr.as_u64() as usize / PAGE_SIZE,
        }
    }

    pub fn next(self) -> Self {
        Self {
            number: self.number + 1,
        }
    }

    pub fn prev(self) -> Self {
        Self {
            number: self.number - 1,
        }
    }

    pub fn offset(self, n: usize) -> Self {
        Self {
            number: self.number + n,
        }
    }

    pub fn distance(self, other: Self) -> usize {
        (self.number as isize - other.number as isize).unsigned_abs()
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct AddressSpaceId(u16);

impl AddressSpaceId {
    pub const KERNEL: Self = Self(0);
    pub const IDLE: Self = Self(1);
    pub const INIT: Self = Self(2);
    pub const FIRST_USER: Self = Self(3);

    pub const fn new(id: u16) -> Self {
        Self(id)
    }
    pub const fn raw(self) -> u16 {
        self.0
    }
    pub const fn as_u16(self) -> u16 {
        self.0
    }
}

pub struct BootMemoryMapView<'a> {
    entries: &'a [MemoryRegion],
}

impl<'a> BootMemoryMapView<'a> {
    pub unsafe fn from_raw(info: &'a MemoryMapInfo) -> MemoryResult<Self> {
        if info.entry_count == 0 {
            return Err(MemoryError::InvalidMemoryMap);
        }

        if info.entries.is_null() {
            return Err(MemoryError::InvalidMemoryMap);
        }

        Ok(Self {
            entries: unsafe { slice::from_raw_parts(info.entries, info.entry_count) },
        })
    }

    pub fn entries(&self) -> &'a [MemoryRegion] {
        self.entries
    }
}

pub const fn pages_for_bytes(bytes: usize) -> usize {
    (bytes + (PAGE_SIZE - 1)) / PAGE_SIZE
}
