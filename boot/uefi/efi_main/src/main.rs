#![no_std]
#![no_main]
extern crate alloc;
use uefi::println;
use uefi::*;
pub const SAIOS_BOOT_MAGIC: u64 = 0x5341_494F_5342_4F4F; // Choose your preferred value
pub const SAIOS_BOOT_VERSION: u32 = 1;
pub mod graphics;
pub mod memorymap;
pub mod acpi;
pub mod smbios;
pub mod cpu;
pub mod firmware;
#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    println!("================================");
    println!("        SAIOS Bootloader");
    println!("================================");
    let boot_info = self::initialize_boot_info();
        
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    println!("Panic occurred!");
    loop {
        core::hint::spin_loop();
    }
}
#[repr(C)]
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