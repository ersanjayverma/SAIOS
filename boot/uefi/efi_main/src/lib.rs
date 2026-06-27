#![no_std]
pub const SAIOS_BOOT_MAGIC: u64 = 0x5341_494F_5342_4F4F; // Choose your preferred value
pub const SAIOS_BOOT_VERSION: u32 = 1;
pub mod acpi;
pub mod cpu;
pub mod firmware;
pub mod graphics;
pub mod memorymap;
pub mod smbios;
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
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
pub fn initialize_boot_info() -> SaiosBootInfo {
    SaiosBootInfo {
        magic: SAIOS_BOOT_MAGIC,
        version: SAIOS_BOOT_VERSION,
        size: core::mem::size_of::<SaiosBootInfo>() as u32,

        framebuffer: graphics::initialize().expect("Failed to initialize framebuffer"),
        memorymap: memorymap::initialize().expect("Failed to initialize memory map"),
        acpi: acpi::initialize().expect("Failed to initialize ACPI info"),
        smbios: smbios::initialize().expect("Failed to initialize SMBIOS info"),
        cpu: cpu::initialize().expect("Failed to initialize CPU info"),
        firmware: firmware::initialize().expect("Failed to initialize firmware info"),

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
pub fn parse_elf_header(elf_data: &[u8]) -> u64 {
    if elf_data.len() < 0x20 {
        panic!("Not enough data for ELF header");
    }
    u64::from_le_bytes(
        elf_data[0x18..0x20]
            .try_into()
            .expect("Failed to parse entry point"),
    )
}
