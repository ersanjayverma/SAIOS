use core::arch::x86_64::__cpuid;
use core::arch::x86_64::__cpuid_count;

pub struct CpuFeatures {
    pub apic: bool,
    pub x2apic: bool,
    pub msr: bool,
    pub tsc: bool,
    pub pat: bool,
    pub pae: bool,
    pub nx: bool,
    pub smep: bool,
    pub smap: bool,
    pub avx: bool,
    pub sse: bool,
    pub sse2: bool,
}
#[inline(always)]
pub fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let r = __cpuid(leaf);

    (r.eax, r.ebx, r.ecx, r.edx)
}
#[inline(always)]
pub fn cpuid_count(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let r = __cpuid_count(leaf, subleaf);

    (r.eax, r.ebx, r.ecx, r.edx)
}
pub fn vendor() -> [u8; 12] {
    let (_, ebx, ecx, edx) = cpuid(0);

    let mut vendor = [0u8; 12];

    vendor[0..4].copy_from_slice(&ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&ecx.to_le_bytes());

    vendor
}

pub fn brand() -> [u8; 48] {
    let mut brand = [0u8; 48];

    for i in 0..3 {
        let (eax, ebx, ecx, edx) = cpuid(0x8000_0002 + i as u32);

        let offset = i * 16;

        brand[offset..offset + 4].copy_from_slice(&eax.to_le_bytes());
        brand[offset + 4..offset + 8].copy_from_slice(&ebx.to_le_bytes());
        brand[offset + 8..offset + 12].copy_from_slice(&ecx.to_le_bytes());
        brand[offset + 12..offset + 16].copy_from_slice(&edx.to_le_bytes());
    }

    brand
}

pub fn features() -> CpuFeatures {
    let (_, _, ecx, edx) = cpuid(1);

    CpuFeatures {
        apic: edx & (1 << 9) != 0,
        x2apic: edx & (1 << 21) != 0,
        msr: edx & (1 << 5) != 0,
        tsc: edx & (1 << 4) != 0,
        pat: ecx & (1 << 12) != 0,
        pae: edx & (1 << 6) != 0,
        nx: edx & (1 << 20) != 0,
        smep: ecx & (1 << 7) != 0,
        smap: ecx & (1 << 20) != 0,
        avx: ecx & (1 << 28) != 0,
        sse: edx & (1 << 25) != 0,
        sse2: edx & (1 << 26) != 0,
    }
}

pub fn logical_processors() -> u8 {
    let (_, ebx, _, _) = cpuid(1);
    (ebx >> 16) as u8
}
