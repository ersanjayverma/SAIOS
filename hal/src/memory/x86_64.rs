use crate::memory::{MmuHal, PageFlags, PagingRoot, PhysAddr, VirtAddr};

#[cfg(not(target_arch = "x86_64"))]
compile_error!("hal::memory::x86_64 currently supports only x86_64");

#[derive(Debug, Copy, Clone, Default)]
pub struct X64Paging;

impl X64Paging {
    pub const fn new() -> Self {
        Self
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

    fn supports_nx(&self) -> bool {
        let extended_max = core::arch::x86_64::__cpuid(0x8000_0000).eax;
        if extended_max < 0x8000_0001 {
            return false;
        }

        let leaf = core::arch::x86_64::__cpuid(0x8000_0001);
        (leaf.edx & (1 << 20)) != 0
    }

    fn supports_huge_pages(&self) -> bool {
        self.supports_1g_pages()
    }

    fn supports_1g_pages(&self) -> bool {
        let extended_max = core::arch::x86_64::__cpuid(0x8000_0000).eax;
        if extended_max < 0x8000_0001 {
            return false;
        }

        let leaf = core::arch::x86_64::__cpuid(0x8000_0001);
        (leaf.edx & (1 << 26)) != 0
    }

    fn supports_pcid(&self) -> bool {
        let leaf = core::arch::x86_64::__cpuid(1);
        (leaf.ecx & (1 << 17)) != 0
    }

    fn sanitize_page_flags(&self, requested: PageFlags) -> PageFlags {
        let mut bits = requested.bits();
        if !self.supports_nx() {
            bits &= !PageFlags::NO_EXECUTE.bits();
        }
        PageFlags::from_bits(bits)
    }
}
