#![no_std]
#![no_main]
extern crate alloc;
use uefi::println;
use uefi::*;

mod acpi;
mod cpu;
mod firmware;
mod graphics;
mod lib;
mod memorymap;
mod pixelformat;
mod smbios;
use lib::*;
#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    println!("================================");
    println!("        SAIOS Bootloader");
    println!("================================");
    let boot_info = SaiosBootInfo {
        magic: SAIOS_BOOT_MAGIC,
        version: 1,
        size: core::mem::size_of::<SaiosBootInfo>() as u32,

        framebuffer: graphics::initialize().expect("Failed to initialize framebuffer"),
        memorymap: memorymap::initialize().expect("Failed to initialize memory map"),
        acpi: acpi::initialize().expect("Failed to initialize ACPI info"),
        smbios: smbios::initialize().expect("Failed to initialize SMBIOS info"),
        cpu: cpu::initialize().expect("Failed to initialize CPU info"),
        firmware: firmware::initialize().expect("Failed to initialize firmware info"),

        reserved: [0; 16],
    };
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
