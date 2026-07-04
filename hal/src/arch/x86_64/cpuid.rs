//! CPUID probing helpers for x86_64 feature discovery.

use core::arch::x86_64::__cpuid;
use core::arch::x86_64::__cpuid_count;

const CPUID_VENDOR: u32 = 0x0000_0000;
const CPUID_FEATURES: u32 = 0x0000_0001;
const CPUID_BRAND_START: u32 = 0x8000_0002;
const FEATURE_EDX_APIC: u32 = 1 << 9;
const FEATURE_EDX_X2APIC: u32 = 1 << 21;
const FEATURE_EDX_MSR: u32 = 1 << 5;
const FEATURE_EDX_TSC: u32 = 1 << 4;
const FEATURE_ECX_PAT: u32 = 1 << 12;
const FEATURE_EDX_PAE: u32 = 1 << 6;
const FEATURE_EDX_NX: u32 = 1 << 20;
const FEATURE_ECX_SMEP: u32 = 1 << 7;
const FEATURE_ECX_SMAP: u32 = 1 << 20;
const FEATURE_ECX_AVX: u32 = 1 << 28;
const FEATURE_EDX_SSE: u32 = 1 << 25;
const FEATURE_EDX_SSE2: u32 = 1 << 26;

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
    let (_, ebx, ecx, edx) = cpuid(CPUID_VENDOR);

    let mut vendor = [0u8; 12];

    vendor[0..4].copy_from_slice(&ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&ecx.to_le_bytes());

    vendor
}

pub fn brand() -> [u8; 48] {
    let mut brand = [0u8; 48];

    for i in 0..3 {
        let (eax, ebx, ecx, edx) = cpuid(CPUID_BRAND_START + i as u32);

        let offset = i * 16;

        brand[offset..offset + 4].copy_from_slice(&eax.to_le_bytes());
        brand[offset + 4..offset + 8].copy_from_slice(&ebx.to_le_bytes());
        brand[offset + 8..offset + 12].copy_from_slice(&ecx.to_le_bytes());
        brand[offset + 12..offset + 16].copy_from_slice(&edx.to_le_bytes());
    }

    brand
}

pub fn features() -> CpuFeatures {
    let (_, _, ecx, edx) = cpuid(CPUID_FEATURES);

    CpuFeatures {
        apic: edx & FEATURE_EDX_APIC != 0,
        x2apic: edx & FEATURE_EDX_X2APIC != 0,
        msr: edx & FEATURE_EDX_MSR != 0,
        tsc: edx & FEATURE_EDX_TSC != 0,
        pat: ecx & FEATURE_ECX_PAT != 0,
        pae: edx & FEATURE_EDX_PAE != 0,
        nx: edx & FEATURE_EDX_NX != 0,
        smep: ecx & FEATURE_ECX_SMEP != 0,
        smap: ecx & FEATURE_ECX_SMAP != 0,
        avx: ecx & FEATURE_ECX_AVX != 0,
        sse: edx & FEATURE_EDX_SSE != 0,
        sse2: edx & FEATURE_EDX_SSE2 != 0,
    }
}

pub fn logical_processors() -> u8 {
    let (_, ebx, _, _) = cpuid(CPUID_FEATURES);
    (ebx >> 16) as u8
}
