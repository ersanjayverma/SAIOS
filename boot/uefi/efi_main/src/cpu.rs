use core::arch::x86_64::__cpuid_count;
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct CpuInfo {
    pub vendor: [u8; 13],
    pub brand: [u8; 49],
    pub features: u64,
    pub extended_features: u64,
    pub max_basic_cpuid: u32,
    pub max_extended_cpuid: u32,
    pub family: u8,
    pub model: u8,
    pub stepping: u8,
    pub cores: u32,
    pub threads: u32,
    pub cache_line_size: u8,
    pub cache_size: u32,
    pub microcode_version: u32,
    pub apic_id: u32,
    pub logical_processors: u32,
    pub hypervisor: HypervisorInfo,
}
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HypervisorInfo {
    pub present: bool,
    pub vendor: [u8; 13],
    pub max_basic_cpuid: u32,
    pub features: u64,
}
pub fn initialize() -> uefi::Result<CpuInfo> {
    let vendor = self::vendor();
    let brand = self::brand();
    let features = self::features();
    let extended_features = self::extended_features();
    let max_basic_cpuid = self::max_basic_cpuid();
    let max_extended_cpuid = self::max_extended_cpuid();
    let family = self::family();
    let model = self::model();
    let stepping = self::stepping();
    let cores = self::cores();
    let threads = self::threads();
    let cache_line_size = self::cache_line_size();
    let cache_size = self::cache_size();
    let microcode_version = self::microcode_version();
    let apic_id = self::apic_id();
    let logical_processors = self::logical_processors();

    let hypervisor = HypervisorInfo {
        vendor: self::hypervisor_vendor(),
        present: self::hypervisor_present(),
        max_basic_cpuid: self::hypervisor_max_basic_cpuid(),
        features: self::hypervisor_features(),
    };

    Ok(CpuInfo {
        vendor,
        brand,
        features,
        extended_features,
        max_basic_cpuid,
        max_extended_cpuid,
        family,
        model,
        stepping,
        cores,
        threads,
        cache_line_size,
        cache_size,
        microcode_version,
        apic_id,
        logical_processors,
        hypervisor,
    })
}
pub fn hypervisor_present() -> bool {
    let ecx = self::cpuid(1, 0).ecx;
    (ecx & (1 << 31)) != 0
}
pub fn hypervisor_vendor() -> [u8; 13] {
    if !self::hypervisor_present() {
        return [0; 13];
    }
    let mut vendor = [0u8; 13];
    let ebx = self::cpuid(0x40000000, 0).ebx;
    let ecx = self::cpuid(0x40000000, 0).ecx;
    let edx = self::cpuid(0x40000000, 0).edx;
    vendor[0..4].copy_from_slice(&ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&ecx.to_le_bytes());
    vendor[8..12].copy_from_slice(&edx.to_le_bytes());
    vendor[12] = 0;
    vendor
}
pub fn hypervisor_max_basic_cpuid() -> u32 {
    if !self::hypervisor_present() {
        return 0;
    }
    self::cpuid(0x40000000, 0).eax
}
pub fn hypervisor_features() -> u64 {
    if !self::hypervisor_present() {
        return 0;
    }
    if self::hypervisor_max_basic_cpuid() < 0x40000001 {
        return 0;
    }
    let ecx = self::cpuid(0x40000001, 0).ecx;
    let edx = self::cpuid(0x40000001, 0).edx;
    ((edx as u64) << 32) | (ecx as u64)
}
pub fn vendor() -> [u8; 13] {
    let mut vendor = [0u8; 13];
    let ebx = self::cpuid(0, 0).ebx;
    let ecx = self::cpuid(0, 0).ecx;
    let edx = self::cpuid(0, 0).edx;
    vendor[0..4].copy_from_slice(&ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&ecx.to_le_bytes());
    vendor[12] = 0;
    vendor
}
pub fn brand() -> [u8; 49] {
    if self::max_extended_cpuid() < 0x80000004 {
        return [0; 49];
    }
    let mut brand = [0u8; 49];
    for i in 0usize..3 {
        let off = i * 16;

        let r = self::cpuid(0x8000_0002 + i as u32, 0);

        brand[off..off + 4].copy_from_slice(&r.eax.to_le_bytes());
        brand[off + 4..off + 8].copy_from_slice(&r.ebx.to_le_bytes());
        brand[off + 8..off + 12].copy_from_slice(&r.ecx.to_le_bytes());
        brand[off + 12..off + 16].copy_from_slice(&r.edx.to_le_bytes());
    }
    brand[48] = 0;
    brand
}
pub fn features() -> u64 {
    let ecx = self::cpuid(1, 0).ecx;
    let edx = self::cpuid(1, 0).edx;
    ((edx as u64) << 32) | (ecx as u64)
}
pub fn extended_features() -> u64 {
    if self::max_basic_cpuid() < 7 {
        return 0;
    }
    let ecx = self::cpuid(7, 0).ecx;
    let edx = self::cpuid(7, 0).edx;
    ((edx as u64) << 32) | (ecx as u64)
}
pub fn max_basic_cpuid() -> u32 {
    self::cpuid(0, 0).eax
}
pub fn max_extended_cpuid() -> u32 {
    self::cpuid(0x80000000, 0).eax
}
pub fn family() -> u8 {
    let eax = self::cpuid(1, 0).eax;
    let family_id = ((eax >> 8) & 0xF) as u8;
    let extended_family_id = ((eax >> 20) & 0xFF) as u8;
    if family_id == 0xF {
        family_id + extended_family_id
    } else {
        family_id
    }
}
pub fn model() -> u8 {
    let eax = self::cpuid(1, 0).eax;
    let model_id = ((eax >> 4) & 0xF) as u8;
    let extended_model_id = ((eax >> 16) & 0xF) as u8;
    if self::family() == 0x6 || self::family() == 0xF {
        (extended_model_id << 4) | model_id
    } else {
        model_id
    }
}
pub fn stepping() -> u8 {
    let eax = self::cpuid(1, 0).eax;
    (eax & 0xF) as u8
}
pub fn cores() -> u32 {
    if self::max_basic_cpuid() < 4 {
        return 1;
    }

    let eax = self::cpuid(4, 0).eax;
    ((eax >> 26) & 0x3F) + 1
}
pub fn threads() -> u32 {
    let ebx = self::cpuid(1, 0).ebx;
    ((ebx >> 16) & 0xFF) as u32
}
pub fn cache_line_size() -> u8 {
    let ebx = self::cpuid(1, 0).ebx;
    ((ebx >> 8) & 0xFF) as u8
}
pub fn cache_size() -> u32 {
    // TODO:
    // CPUID leaf 4 must be decoded using:
    // cache_size =
    // (ways + 1) *
    // (partitions + 1) *
    // (line_size + 1) *
    // (sets + 1)
    //
    // Returning 0 indicates "unknown".
    0
}

pub fn microcode_version() -> u32 {
    // TODO:
    // The microcode revision is not available through CPUID.
    // It must be read from IA32_BIOS_SIGN_ID (MSR 0x8B)
    // after executing CPUID leaf 1.
    //
    // Returning 0 indicates "unknown".
    0
}
pub fn apic_id() -> u32 {
    let ebx = self::cpuid(1, 0).ebx;
    ((ebx >> 24) & 0xFF) as u32
}
pub fn logical_processors() -> u32 {
    let ebx = self::cpuid(1, 0).ebx;
    ((ebx >> 16) & 0xFF) as u32
}
pub fn cpuid(eax: u32, ecx: u32) -> CpuidRegisters {
    let r = { __cpuid_count(eax, ecx) };

    CpuidRegisters {
        eax: r.eax,
        ebx: r.ebx,
        ecx: r.ecx,
        edx: r.edx,
    }
}
#[repr(C)]
pub struct CpuidRegisters {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}
