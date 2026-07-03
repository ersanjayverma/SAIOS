use core::arch::x86_64::__cpuid_count;

const CPUID_BASIC_VENDOR: u32 = 0x0000_0000;
const CPUID_BASIC_FEATURES: u32 = 0x0000_0001;
const CPUID_BASIC_EXTENDED_FEATURES: u32 = 0x0000_0007;
const CPUID_CACHE_PARAMETERS: u32 = 0x0000_0004;
const CPUID_HYPERVISOR_MAX: u32 = 0x4000_0000;
const CPUID_HYPERVISOR_FEATURES: u32 = 0x4000_0001;
const CPUID_EXTENDED_BASE: u32 = 0x8000_0000;
const CPUID_BRAND_START: u32 = 0x8000_0002;
const CPUID_BRAND_END: u32 = 0x8000_0004;

/// A snapshot of everything the bootloader can learn about the boot CPU via
/// CPUID (and a couple of MSRs). Passed to the kernel as part of the boot
/// handoff so it does not have to re-probe the processor.
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
/// Details about a hypervisor host, if the CPU is running virtualized.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HypervisorInfo {
    pub present: bool,
    pub vendor: [u8; 13],
    pub max_basic_cpuid: u32,
    pub features: u64,
}
/// Probe the CPU and assemble a complete [`CpuInfo`] record.
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
/// Whether a hypervisor is present (CPUID.1:ECX bit 31).
pub fn hypervisor_present() -> bool {
    let ecx = self::cpuid(CPUID_BASIC_FEATURES, 0).ecx;
    (ecx & (1 << 31)) != 0
}
/// Hypervisor vendor signature (12 ASCII bytes + NUL), or zeros if none.
pub fn hypervisor_vendor() -> [u8; 13] {
    if !self::hypervisor_present() {
        return [0; 13];
    }
    let mut vendor = [0u8; 13];
    let ebx = self::cpuid(CPUID_HYPERVISOR_MAX, 0).ebx;
    let ecx = self::cpuid(CPUID_HYPERVISOR_MAX, 0).ecx;
    let edx = self::cpuid(CPUID_HYPERVISOR_MAX, 0).edx;
    vendor[0..4].copy_from_slice(&ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&ecx.to_le_bytes());
    vendor[8..12].copy_from_slice(&edx.to_le_bytes());
    vendor[12] = 0;
    vendor
}
/// Highest hypervisor CPUID leaf supported (0 if no hypervisor).
pub fn hypervisor_max_basic_cpuid() -> u32 {
    if !self::hypervisor_present() {
        return 0;
    }
    self::cpuid(CPUID_HYPERVISOR_MAX, 0).eax
}
/// Hypervisor feature bits from CPUID leaf 0x40000001 (0 if unavailable).
pub fn hypervisor_features() -> u64 {
    if !self::hypervisor_present() {
        return 0;
    }
    if self::hypervisor_max_basic_cpuid() < CPUID_HYPERVISOR_FEATURES {
        return 0;
    }
    let ecx = self::cpuid(CPUID_HYPERVISOR_FEATURES, 0).ecx;
    let edx = self::cpuid(CPUID_HYPERVISOR_FEATURES, 0).edx;
    ((edx as u64) << 32) | (ecx as u64)
}
/// CPU vendor string (12 ASCII bytes + NUL), e.g. "GenuineIntel".
pub fn vendor() -> [u8; 13] {
    let mut vendor = [0u8; 13];
    let ebx = self::cpuid(CPUID_BASIC_VENDOR, 0).ebx;
    let ecx = self::cpuid(CPUID_BASIC_VENDOR, 0).ecx;
    let edx = self::cpuid(CPUID_BASIC_VENDOR, 0).edx;
    vendor[0..4].copy_from_slice(&ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&ecx.to_le_bytes());
    vendor[12] = 0;
    vendor
}
/// CPU brand/marketing string (48 ASCII bytes + NUL), or zeros if unsupported.
pub fn brand() -> [u8; 49] {
    if self::max_extended_cpuid() < CPUID_BRAND_END {
        return [0; 49];
    }
    let mut brand = [0u8; 49];
    for i in 0usize..3 {
        let off = i * 16;

        let r = self::cpuid(CPUID_BRAND_START + i as u32, 0);

        brand[off..off + 4].copy_from_slice(&r.eax.to_le_bytes());
        brand[off + 4..off + 8].copy_from_slice(&r.ebx.to_le_bytes());
        brand[off + 8..off + 12].copy_from_slice(&r.ecx.to_le_bytes());
        brand[off + 12..off + 16].copy_from_slice(&r.edx.to_le_bytes());
    }
    brand[48] = 0;
    brand
}
/// Standard feature flags: EDX in the high 32 bits, ECX in the low 32 bits of
/// CPUID leaf 1.
pub fn features() -> u64 {
    let ecx = self::cpuid(CPUID_BASIC_FEATURES, 0).ecx;
    let edx = self::cpuid(CPUID_BASIC_FEATURES, 0).edx;
    ((edx as u64) << 32) | (ecx as u64)
}
/// Extended feature flags from CPUID leaf 7 (0 if unsupported).
pub fn extended_features() -> u64 {
    if self::max_basic_cpuid() < CPUID_BASIC_EXTENDED_FEATURES {
        return 0;
    }
    let ecx = self::cpuid(CPUID_BASIC_EXTENDED_FEATURES, 0).ecx;
    let edx = self::cpuid(CPUID_BASIC_EXTENDED_FEATURES, 0).edx;
    ((edx as u64) << 32) | (ecx as u64)
}
/// Highest standard CPUID leaf supported by the CPU.
pub fn max_basic_cpuid() -> u32 {
    self::cpuid(CPUID_BASIC_VENDOR, 0).eax
}
/// Highest extended (0x8000_xxxx) CPUID leaf supported by the CPU.
pub fn max_extended_cpuid() -> u32 {
    self::cpuid(CPUID_EXTENDED_BASE, 0).eax
}
/// Effective CPU family, combining the base and extended family fields.
pub fn family() -> u8 {
    let eax = self::cpuid(CPUID_BASIC_FEATURES, 0).eax;
    let family_id = ((eax >> 8) & 0xF) as u8;
    let extended_family_id = ((eax >> 20) & 0xFF) as u8;
    if family_id == 0xF {
        family_id + extended_family_id
    } else {
        family_id
    }
}
/// Effective CPU model, combining the base and extended model fields.
pub fn model() -> u8 {
    let eax = self::cpuid(CPUID_BASIC_FEATURES, 0).eax;
    let model_id = ((eax >> 4) & 0xF) as u8;
    let extended_model_id = ((eax >> 16) & 0xF) as u8;
    if self::family() == 0x6 || self::family() == 0xF {
        (extended_model_id << 4) | model_id
    } else {
        model_id
    }
}
/// CPU stepping identifier.
pub fn stepping() -> u8 {
    let eax = self::cpuid(CPUID_BASIC_FEATURES, 0).eax;
    (eax & 0xF) as u8
}
/// Number of physical cores reported by CPUID leaf 4 (falls back to 1).
pub fn cores() -> u32 {
    if self::max_basic_cpuid() < CPUID_CACHE_PARAMETERS {
        return 1;
    }

    let eax = self::cpuid(CPUID_CACHE_PARAMETERS, 0).eax;
    ((eax >> 26) & 0x3F) + 1
}
/// Number of logical threads reported by CPUID.1:EBX[23:16].
pub fn threads() -> u32 {
    let ebx = self::cpuid(CPUID_BASIC_FEATURES, 0).ebx;
    (ebx >> 16) & 0xFF
}
/// CLFLUSH line size in bytes (CPUID.1:EBX[15:8], units of 8 bytes as raw).
pub fn cache_line_size() -> u8 {
    let ebx = self::cpuid(CPUID_BASIC_FEATURES, 0).ebx;
    ((ebx >> 8) & 0xFF) as u8
}
pub fn cache_size() -> u32 {
    // Decode CPUID leaf 4 deterministic cache parameters. Each valid subleaf
    // describes one cache; the size is:
    //   (ways + 1) * (partitions + 1) * (line_size + 1) * (sets + 1)
    // We return the largest cache reported (the last-level cache), which is the
    // most meaningful single value. Returns 0 when unavailable.
    if self::max_basic_cpuid() < CPUID_CACHE_PARAMETERS {
        return 0;
    }

    let mut largest: u32 = 0;
    for subleaf in 0u32..=63 {
        let r = self::cpuid(CPUID_CACHE_PARAMETERS, subleaf);

        // EAX[4:0] == 0 means no more caches are described.
        let cache_type = r.eax & 0x1F;
        if cache_type == 0 {
            break;
        }

        let line_size = (r.ebx & 0xFFF) + 1; // EBX[11:0]
        let partitions = ((r.ebx >> 12) & 0x3FF) + 1; // EBX[21:12]
        let ways = ((r.ebx >> 22) & 0x3FF) + 1; // EBX[31:22]
        let sets = r.ecx + 1; // ECX (32-bit)

        let size = (ways as u64)
            .saturating_mul(partitions as u64)
            .saturating_mul(line_size as u64)
            .saturating_mul(sets as u64);

        let size = core::cmp::min(size, u32::MAX as u64) as u32;
        if size > largest {
            largest = size;
        }
    }

    largest
}

pub fn microcode_version() -> u32 {
    // The microcode revision lives in IA32_BIOS_SIGN_ID (MSR 0x8B). The
    // architectural procedure is: clear the MSR, execute CPUID leaf 1, then
    // read the MSR back; the revision is in the high 32 bits (EDX).
    //
    // MSR access requires ring 0. During UEFI boot services we are in ring 0,
    // but rdmsr/wrmsr are not guaranteed on every emulated CPU, so guard on the
    // presence of the standard leaf. Returns 0 when unavailable.
    const IA32_BIOS_SIGN_ID: u32 = 0x8B;

    if self::max_basic_cpuid() < CPUID_BASIC_FEATURES {
        return 0;
    }

    unsafe {
        // Clear the MSR so a stale value cannot be mistaken for the revision.
        wrmsr(IA32_BIOS_SIGN_ID, 0);
        // CPUID leaf 1 latches the current microcode revision into the MSR.
        let _ = self::cpuid(CPUID_BASIC_FEATURES, 0);
        (rdmsr(IA32_BIOS_SIGN_ID) >> 32) as u32
    }
}

/// Read a 64-bit Model Specific Register. Caller must ensure ring 0 and that
/// the MSR is supported by the CPU.
#[inline]
unsafe fn rdmsr(msr: u32) -> u64 {
    let (high, low): (u32, u32);
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((high as u64) << 32) | (low as u64)
}

/// Write a 64-bit Model Specific Register. Caller must ensure ring 0 and that
/// the MSR is supported and writable on the CPU.
#[inline]
unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags),
        );
    }
}
/// Initial local APIC ID of the boot CPU (CPUID.1:EBX[31:24]).
pub fn apic_id() -> u32 {
    let ebx = self::cpuid(CPUID_BASIC_FEATURES, 0).ebx;
    (ebx >> 24) & 0xFF
}
/// Maximum addressable logical processor IDs in this package
/// (CPUID.1:EBX[23:16]).
pub fn logical_processors() -> u32 {
    let ebx = self::cpuid(CPUID_BASIC_FEATURES, 0).ebx;
    (ebx >> 16) & 0xFF
}
/// Thin wrapper over the `cpuid` instruction returning all four result
/// registers for the given leaf/subleaf.
pub fn cpuid(eax: u32, ecx: u32) -> CpuidRegisters {
    let r = { __cpuid_count(eax, ecx) };

    CpuidRegisters {
        eax: r.eax,
        ebx: r.ebx,
        ecx: r.ecx,
        edx: r.edx,
    }
}
/// The `eax`/`ebx`/`ecx`/`edx` output registers from a `cpuid` call.
#[repr(C)]
pub struct CpuidRegisters {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}
