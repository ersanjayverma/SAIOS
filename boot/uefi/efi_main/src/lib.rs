#![no_std]
use uefi::println;
pub const SAIOS_BOOT_MAGIC: u64 = 0x5341_494F_5342_4F4F; // Choose your preferred value
pub const SAIOS_BOOT_VERSION: u32 = 1;
pub mod acpi;
pub mod cpu;
pub mod firmware;
pub mod graphics;
pub mod memorymap;
pub mod smbios;
pub mod ui;
pub const R_X86_64_RELATIVE: u32 = 8;
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaiosBootInfo {
    pub magic: u64,
    pub version: u32,
    pub size: u32,

    pub framebuffer: graphics::FramebufferInfo,
    pub memorymap: memorymap::MemoryMapInfo,
    pub acpi: acpi::AcpiInfo,
    pub smbios: smbios::SmbiosInfo,
    pub cpu: cpu::CpuInfo,
    pub firmware: firmware::FirmwareInfo,

    pub reserved: [u64; 16],
}
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elf64Header {
    pub ident: [u8; 16],

    pub elf_type: u16,
    pub machine: u16,
    pub version: u32,

    pub entry: u64,

    pub phoff: u64,
    pub shoff: u64,

    pub flags: u32,

    pub ehsize: u16,

    pub phentsize: u16,
    pub phnum: u16,

    pub shentsize: u16,
    pub shnum: u16,

    pub shstrndx: u16,
}
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Elf64ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,

    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,

    pub p_filesz: u64,
    pub p_memsz: u64,

    pub p_align: u64,
}
#[repr(u32)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProgramHeaderType {
    Null = 0,
    Load = 1,
    Dynamic = 2,
    Interp = 3,
    Note = 4,
    Shlib = 5,
    Phdr = 6,
    Tls = 7,
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Elf64Dyn {
    pub d_tag: i64,
    pub d_val: u64,
}
#[repr(i64)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DynamicTag {
    Null = 0,
    Rela = 7,
    RelaSz = 8,
    RelaEnt = 9,
}
#[derive(Debug, Default)]
pub struct DynamicInfo {
    pub rela: Option<u64>,
    pub rela_size: Option<u64>,
    pub rela_entry_size: Option<u64>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}
pub fn initialize_boot_info() -> SaiosBootInfo {
    let _ = println!("[boot] boot_info: framebuffer init...");
    let framebuffer = match graphics::initialize() {
        Ok(v) => {
            let _ = println!("[boot] boot_info: framebuffer ok");
            v
        }
        Err(e) => {
            let _ = println!("[boot] boot_info: framebuffer fail {:?}", e.status());
            graphics::FramebufferInfo::empty()
        }
    };

    let _ = println!("[boot] boot_info: acpi init...");
    let acpi = match acpi::initialize() {
        Ok(v) => {
            let _ = println!("[boot] boot_info: acpi ok");
            v
        }
        Err(e) => {
            let _ = println!("[boot] boot_info: acpi fail {:?}", e.status());
            acpi::AcpiInfo::empty()
        }
    };

    let _ = println!("[boot] boot_info: smbios init...");
    let smbios = smbios::initialize().unwrap_or_else(|e| {
        let _ = println!("[boot] boot_info: smbios fail {:?}", e.status());
        let _ = println!("[boot] boot_info: smbios fallback");
        smbios::SmbiosInfo {
        entry_point: 0,
        version_major: 0,
        version_minor: 0,
        version_revision: 0,
        table_address: 0,
        table_length: 0,
        is_64bit: false,
    }
    });
    let _ = println!("[boot] boot_info: smbios ok/fallback");

    let _ = println!("[boot] boot_info: cpu init...");
    let cpu = cpu::initialize().unwrap_or_else(|e| {
        let _ = println!("[boot] boot_info: cpu fail {:?}", e.status());
        let _ = println!("[boot] boot_info: cpu fallback");
        cpu::CpuInfo {
        vendor: [0; 13],
        brand: [0; 49],
        features: 0,
        extended_features: 0,
        max_basic_cpuid: 0,
        max_extended_cpuid: 0,
        family: 0,
        model: 0,
        stepping: 0,
        cores: 0,
        threads: 0,
        cache_line_size: 0,
        cache_size: 0,
        microcode_version: 0,
        apic_id: 0,
        logical_processors: 0,
        hypervisor: cpu::HypervisorInfo {
            present: false,
            vendor: [0; 13],
            max_basic_cpuid: 0,
            features: 0,
        },
    }
    });
    let _ = println!("[boot] boot_info: cpu ok/fallback");

    let _ = println!("[boot] boot_info: firmware init...");
    let firmware = firmware::initialize().unwrap_or_else(|e| {
        let _ = println!("[boot] boot_info: firmware fail {:?}", e.status());
        let _ = println!("[boot] boot_info: firmware fallback");
        firmware::FirmwareInfo {
        vendor: [0; 32],
        firmware_revision: 0,
        uefi_revision: uefi::table::Revision::new(0, 0),
    }
    });
    let _ = println!("[boot] boot_info: firmware ok/fallback");

    SaiosBootInfo {
        magic: SAIOS_BOOT_MAGIC,
        version: SAIOS_BOOT_VERSION,
        size: core::mem::size_of::<SaiosBootInfo>() as u32,

        framebuffer,
        memorymap: memorymap::MemoryMapInfo {
            entries: core::ptr::null(),
            entry_count: 0,
        },
        acpi,
        smbios,
        cpu,
        firmware,

        reserved: [0; 16],
    }
}
pub fn load_elf64_header(buffer: &[u8]) -> Result<Elf64Header, &'static str> {
    if buffer.len() < core::mem::size_of::<Elf64Header>() {
        return Err("Buffer too small");
    }

    let header = unsafe { core::ptr::read_unaligned(buffer.as_ptr() as *const Elf64Header) };

    Ok(header)
}
pub fn load_program_header(elf: &[u8], offset: usize) -> Result<Elf64ProgramHeader, &'static str> {
    let size = core::mem::size_of::<Elf64ProgramHeader>();

    if offset + size > elf.len() {
        return Err("Program header outside file");
    }

    let ptr = unsafe { elf.as_ptr().add(offset) };

    Ok(unsafe { core::ptr::read_unaligned(ptr as *const Elf64ProgramHeader) })
}

pub fn find_dynamic_segment(program_headers: &[Elf64ProgramHeader]) -> Option<&Elf64ProgramHeader> {
    program_headers
        .iter()
        .find(|ph| ph.p_type == ProgramHeaderType::Dynamic as u32)
}
pub fn parse_dynamic(bytes: &[u8], segment: &Elf64ProgramHeader) -> DynamicInfo {
    let mut dynamic_info = DynamicInfo::default();
    let entry_size = core::mem::size_of::<Elf64Dyn>();
    let num_entries = (segment.p_filesz / entry_size as u64) as usize;

    for i in 0..num_entries {
        let offset = (segment.p_offset as usize) + (i * entry_size);
        if offset + entry_size > bytes.len() {
            break;
        }

        let dyn_entry: Elf64Dyn =
            unsafe { core::ptr::read_unaligned(bytes[offset..].as_ptr() as *const Elf64Dyn) };

        match dyn_entry.d_tag {
            x if x == DynamicTag::Null as i64 => break,
            x if x == DynamicTag::Rela as i64 => dynamic_info.rela = Some(dyn_entry.d_val),
            x if x == DynamicTag::RelaSz as i64 => dynamic_info.rela_size = Some(dyn_entry.d_val),
            x if x == DynamicTag::RelaEnt as i64 => {
                dynamic_info.rela_entry_size = Some(dyn_entry.d_val)
            }
            _ => {}
        }
    }

    dynamic_info
}
#[inline]
pub fn relocation_type(info: u64) -> u32 {
    (info & 0xffffffff) as u32
}

#[inline]
pub fn relocation_symbol(info: u64) -> u32 {
    (info >> 32) as u32
}
pub fn apply_relocations(load_bias: u64, relocations: &[Elf64Rela]) -> Result<(), &'static str> {
    for rela in relocations {
        let relocation_type = relocation_type(rela.r_info);
       
        match relocation_type {
            R_X86_64_RELATIVE => {
                let new_value = load_bias.wrapping_add(rela.r_addend as u64);
                unsafe {
                    let ptr = (load_bias + rela.r_offset) as *mut u64;
                    *ptr = new_value;
                }
            }
            _ => {
                return Err("Unsupported relocation type");
            }
        }
    }

    Ok(())
}
pub fn virtual_to_file_offset(program_headers: &[Elf64ProgramHeader], vaddr: u64) -> Option<usize> {
    for ph in program_headers {
        if ph.p_type != ProgramHeaderType::Load as u32 {
            continue;
        }

        if vaddr >= ph.p_vaddr && vaddr < ph.p_vaddr + ph.p_memsz {
            return Some((ph.p_offset + (vaddr - ph.p_vaddr)) as usize);
        }
    }

    None
}
