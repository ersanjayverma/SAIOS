use crate::memory::constants::FLAG_MASK;
use crate::memory::types::PhysAddr;
use crate::memory::vmm::flags::PageFlags;
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct PageTableEntry {
    value: u64,
}

impl PageTableEntry {
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    pub const fn raw(self) -> u64 {
        self.value
    }

    pub const fn present(self) -> bool {
        self.value & (1 << 0) != 0
    }

    pub const fn writable(self) -> bool {
        self.value & (1 << 1) != 0
    }

    pub const fn user(self) -> bool {
        self.value & (1 << 2) != 0
    }

    pub const fn huge(self) -> bool {
        self.value & (1 << 7) != 0
    }

    pub const fn nx(self) -> bool {
        self.value & (1 << 63) != 0
    }

    pub const fn frame(self) -> PhysAddr {
        PhysAddr::new(self.value & 0x000f_ffff_ffff_f000)
    }

    pub fn set_frame(&mut self, frame: PhysAddr) {
        // Clear the physical address field (bits 12..51) while preserving
        // flags in bits 0..11 and 52..63.
        const ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;
        self.value &= !ADDR_MASK;
        self.value |= frame.as_u64() & ADDR_MASK;
    }

    pub fn set_flags(&mut self, flags: PageFlags) {
        self.value &= !FLAG_MASK;
        self.value |= flags.bits();
    }

    pub fn clear(&mut self) {
        self.value = 0;
    }

    pub fn set_present(&mut self, present: bool) {
        if present {
            self.value |= 1 << 0;
        } else {
            self.value &= !(1 << 0);
        }
    }
    pub fn set_user(&mut self, user: bool) {
        if user {
            self.value |= 1 << 2;
        } else {
            self.value &= !(1 << 2);
        }
    }
    pub fn set_writable(&mut self, writable: bool) {
        if writable {
            self.value |= 1 << 1;
        } else {
            self.value &= !(1 << 1);
        }
    }
}
