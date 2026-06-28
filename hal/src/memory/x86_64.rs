use crate::memory::{MmuHal, PageFlags, PagingRoot, PhysAddr, VirtAddr, contracts};

#[cfg(not(target_arch = "x86_64"))]
compile_error!("hal::memory::x86_64 currently supports only x86_64");

#[derive(Debug, Copy, Clone, Default)]
pub struct X64Paging {
    features: contracts::CpuFeatures,
}

impl X64Paging {
    pub fn new() -> Self {
        Self {
            features: contracts::detect_cpu_features(),
        }
    }
}
impl MmuHal for X64Paging {
    fn active_root(&self) -> PagingRoot {
        let value: u64;
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
        }
        PagingRoot::new(PhysAddr::new(value & !0xfff))
    }
    fn cpu_features(&self) -> contracts::CpuFeatures {
        self.features
    }
    /// # Safety
    ///
    /// The supplied root must reference a valid, fully initialized page-table
    /// hierarchy. Loading an invalid CR3 value can immediately fault the CPU.
    unsafe fn switch_root(&self, root: PagingRoot) {
        unsafe {
            core::arch::asm!(
                "mov cr3, {}",
                in(reg) root.phys_addr().as_u64(),
                options(nostack, preserves_flags)
            );
        }
    }

    fn flush(&self, address: VirtAddr) {
        unsafe {
            core::arch::asm!(
                "invlpg [{}]",
                in(reg) address.as_u64(),
                options(nostack, preserves_flags)
            );
        }
    }

    fn flush_all(&self) {
        let root = self.active_root();
        unsafe { self.switch_root(root) };
    }

    fn page_size(&self) -> usize {
        4096
    }

    fn sanitize_page_flags(&self, requested: PageFlags) -> PageFlags {
        let mut bits = requested.bits();

        if !self.features.nx {
            bits &= !PageFlags::NO_EXECUTE.bits();
        }

        PageFlags::from_bits(bits)
    }
}
