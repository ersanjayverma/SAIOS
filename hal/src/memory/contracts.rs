use crate::memory::{PageFlags, PagingRoot, VirtAddr};
#[derive(Debug, Copy, Clone, Default)]
pub struct CpuFeatures {
    pub nx: bool,
    pub pcid: bool,
    pub page1g: bool,
    pub huge_pages: bool,
    pub smep: bool,
    pub smap: bool,
    pub invpcid: bool,
    pub xsave: bool,
    pub xsaveopt: bool,
    pub xsavec: bool,
    pub xsaves: bool,
}

#[cfg(target_arch = "x86_64")]
pub fn detect_cpu_features() -> CpuFeatures {
    use core::arch::x86_64::{__cpuid, __cpuid_count};

    let mut features = CpuFeatures {
        nx: false,
        pcid: false,
        page1g: false,
        huge_pages: false,
        smep: false,
        smap: false,
        invpcid: false,
        xsave: false,
        xsaveopt: false,
        xsavec: false,
        xsaves: false,
    };

    //
    // Standard CPUID leaves
    //

    let leaf0 = __cpuid(0);
    let max_standard = leaf0.eax;

    let leaf1 = __cpuid(1);
    features.huge_pages = (leaf1.edx & (1 << 26)) != 0;
    features.page1g = (leaf1.edx & (1 << 26)) != 0;
    features.pcid = (leaf1.ecx & (1 << 17)) != 0;
    features.xsave = (leaf1.ecx & (1 << 26)) != 0;

    if max_standard >= 7 {
        let leaf7 = __cpuid_count(7, 0);

        features.smep = (leaf7.ebx & (1 << 7)) != 0;
        features.invpcid = (leaf7.ebx & (1 << 10)) != 0;
        features.smap = (leaf7.ebx & (1 << 20)) != 0;
    }

    //
    // Extended CPUID leaves
    //

    let ext0 = __cpuid(0x8000_0000);

    if ext0.eax >= 0x8000_0001 {
        let ext1 = __cpuid(0x8000_0001);

        features.nx = (ext1.edx & (1 << 20)) != 0;
    }

    //
    // XSAVE capabilities
    //

    if features.xsave && max_standard >= 0xD {
        let xsave = __cpuid_count(0xD, 1);

        features.xsaveopt = (xsave.eax & (1 << 0)) != 0;
        features.xsavec = (xsave.eax & (1 << 1)) != 0;
        features.xsaves = (xsave.eax & (1 << 3)) != 0;
    }

    features
}

pub trait MmuHal {
    fn active_root(&self) -> PagingRoot;

    /// # Safety
    ///
    /// The supplied root must reference a valid, fully initialized page-table
    /// hierarchy for the current architecture. Switching to an invalid root may
    /// immediately fault the CPU or leave address translation undefined.
    unsafe fn switch_root(&self, root: PagingRoot);

    fn flush(&self, address: VirtAddr);
    fn flush_all(&self);
    fn page_size(&self) -> usize;
    fn cpu_features(&self) -> CpuFeatures;

    fn sanitize_page_flags(&self, requested: PageFlags) -> PageFlags;
}
