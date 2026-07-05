//! x86_64 paging table structures and CR3 access helpers.

use core::arch::asm;

pub const ENTRY_COUNT: usize = 512;
pub const PAGE_SIZE: u64 = 4096;

pub const FLAG_PRESENT: u64 = 1 << 0;
pub const FLAG_WRITABLE: u64 = 1 << 1;
pub const FLAG_USER: u64 = 1 << 2;
pub const FLAG_PWT: u64 = 1 << 3;
pub const FLAG_PCD: u64 = 1 << 4;
pub const FLAG_ACCESSED: u64 = 1 << 5;
pub const FLAG_DIRTY: u64 = 1 << 6;
/// Page-table PAT bit for 4 KiB pages.
pub const FLAG_PAT: u64 = 1 << 7;
pub const FLAG_HUGE: u64 = 1 << 7;
pub const FLAG_GLOBAL: u64 = 1 << 8;
pub const FLAG_NX: u64 = 1 << 63;

const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Entry(pub u64);

impl Entry {
    pub const fn new() -> Self {
        Self(0)
    }

    pub const fn is_present(self) -> bool {
        (self.0 & FLAG_PRESENT) != 0
    }

    pub const fn address(self) -> u64 {
        self.0 & ADDR_MASK
    }

    pub fn set_address(&mut self, addr: u64) {
        debug_assert!(
            (addr & (PAGE_SIZE - 1)) == 0,
            "page address must be 4 KiB aligned"
        );
        self.0 = (self.0 & !ADDR_MASK) | (addr & ADDR_MASK);
    }

    pub fn set_flags(&mut self, flags: u64) {
        self.0 |= flags;
    }

    pub fn clear_flags(&mut self, flags: u64) {
        self.0 &= !flags;
    }

    pub fn set_page(&mut self, addr: u64, flags: u64) {
        debug_assert!(
            (addr & (PAGE_SIZE - 1)) == 0,
            "page address must be 4 KiB aligned"
        );
        self.0 = (addr & ADDR_MASK) | flags | FLAG_PRESENT;
    }
}

impl Default for Entry {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C, align(4096))]
pub struct Table {
    pub entries: [Entry; ENTRY_COUNT],
}

impl Table {
    pub const fn new() -> Self {
        Self {
            entries: [Entry::new(); ENTRY_COUNT],
        }
    }

    pub fn clear(&mut self) {
        self.entries.fill(Entry::new());
    }

    pub fn as_phys_addr(&self) -> u64 {
        self as *const Self as u64
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
pub fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

#[inline(always)]
/// # Safety
///
/// The caller must ensure `value` is a valid physical address of a properly
/// constructed PML4 table; loading an invalid CR3 will cause undefined
/// behavior or a triple fault.
pub unsafe fn write_cr3(value: u64) {
    unsafe {
        asm!("mov cr3, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
pub fn invlpg(addr: u64) {
    unsafe {
        asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
    }
}
