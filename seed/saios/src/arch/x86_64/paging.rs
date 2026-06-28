use crate::memory::types::{PhysAddr, VirtAddr};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PagingRoot {
    phys: PhysAddr,
}

impl PagingRoot {
    pub const fn new(phys: PhysAddr) -> Self {
        Self { phys }
    }

    pub const fn phys_addr(self) -> PhysAddr {
        self.phys
    }
}

pub fn active_root() -> PagingRoot {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    PagingRoot::new(PhysAddr::new(value & !0xfff))
}

pub unsafe fn switch_root(root: PagingRoot) {
    unsafe {
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) root.phys_addr().as_u64(),
            options(nostack, preserves_flags)
        );
    }
}

pub fn invalidate(address: VirtAddr) {
    unsafe {
        core::arch::asm!(
            "invlpg [{}]",
            in(reg) address.as_u64(),
            options(nostack, preserves_flags)
        );
    }
}
