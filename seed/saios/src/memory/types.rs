use core::slice;

use efi_main::memorymap::{MemoryMapInfo, MemoryRegion};
pub use hal::memory::{PhysAddr, VirtAddr};

use crate::memory::constants::PAGE_SIZE;
use crate::memory::errors::{MemoryError, MemoryResult};

pub trait PhysAddrExt {
    fn is_page_aligned(self) -> bool;
    fn align_down(self) -> Self;
}

impl PhysAddrExt for PhysAddr {
    fn is_page_aligned(self) -> bool {
        self.as_u64() as usize % PAGE_SIZE == 0
    }

    fn align_down(self) -> Self {
        Self::new(self.as_u64() & !((PAGE_SIZE as u64) - 1))
    }
}

pub trait VirtAddrExt {
    fn is_page_aligned(self) -> bool;
    fn align_down(self) -> Self;
    fn page_offset(self) -> usize;
}

impl VirtAddrExt for VirtAddr {
    fn is_page_aligned(self) -> bool {
        self.as_u64() as usize % PAGE_SIZE == 0
    }

    fn align_down(self) -> Self {
        Self::new(self.as_u64() & !((PAGE_SIZE as u64) - 1))
    }

    fn page_offset(self) -> usize {
        self.as_u64() as usize & (PAGE_SIZE - 1)
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
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AddressSpaceId(u16);

impl AddressSpaceId {
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u16 {
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
