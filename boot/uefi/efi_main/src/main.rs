#![no_std]
#![no_main]
extern crate alloc;
use uefi::println;
use uefi::*;
use uefi::boot::MemoryType as UefiMemoryType;
#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    println!("================================");
    println!("        SAIOS Bootloader"        );
    println!("================================");
    let boot_info = efi_main::initialize_boot_info();
    println!("========================================");
    println!("          SAIOS BOOT INFORMATION        ");
    println!("========================================");
    
    // Print metadata fields explicitly for validation
    println!("Magic Check:   0x{:X} (Expected: 0x{:X})", boot_info.magic, efi_main::SAIOS_BOOT_MAGIC); 
    println!("Boot Version:  {}.{}", boot_info.version >> 16, boot_info.version & 0xFFFF);
    println!("Struct Size:   {} bytes", boot_info.size);
    println!("----------------------------------------");

    // Print the sub-structures using the derived Debug trait
    println!("{:#?}", boot_info.framebuffer);
    println!("{:#?}", boot_info.memorymap);
    println!("{:#?}", boot_info.acpi);
    println!("{:#?}", boot_info.smbios);
    println!("{:#?}", boot_info.cpu);
    println!("{:#?}", boot_info.firmware);
    
    println!("========================================");  
    
    
    let memory_map = unsafe {
        uefi::boot::exit_boot_services(Some(uefi::table::boot::MemoryType::LOADER_DATA))
    };
   
    self::jump_to_seed(boot_info)
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Extract the panic message and file/line location
    let message = info.message();
    let location = info.location();

    if let Some(loc) = location {
        let _ = println!("Panic occurred at {}:{}:{}", loc.file(), loc.line(), message);
    } else {
        let _ = println!("Panic occurred: {}", message);
    }
    loop {
        core::hint::spin_loop();
    }
}


pub fn jump_to_seed(boot_info: efi_main::SaiosBootInfo) -> ! {
    // Jump to kernel entry point
    let seed_entry_point = 0x100000 as *const ();
    let seed_fn: extern "C" fn(&efi_main::SaiosBootInfo) -> ! =
        unsafe { core::mem::transmute(seed_entry_point) };

    seed_fn(&boot_info) 
}
