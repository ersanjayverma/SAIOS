#![no_std]
#![no_main]

use core::fmt::{self, Write};
use efi_main::SaiosBootInfo;
static mut BOOT_INFO: *const SaiosBootInfo = core::ptr::null();
struct Writer;

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            let fb = (*BOOT_INFO).framebuffer.base as *mut u32;

            for b in s.bytes() {
                fb.write_volatile(b as u32);
            }
        }

        Ok(())
    }
}
pub fn clear_framebuffer(color: u32) {
    unsafe {
        let fb = (*BOOT_INFO).framebuffer.base as *mut u32;

        let pixels =
            (*BOOT_INFO).framebuffer.stride * (*BOOT_INFO).framebuffer.height;

        for i in 0..pixels {
            fb.add(i).write_volatile(color);
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info:*const SaiosBootInfo) -> ! {
    BOOT_INFO = boot_info;
    clear_framebuffer(0x00FF0000); // Clear framebuffer to red
    let mut w = Writer;
    let _ = w.write_str("========================================");
    let _ = w.write_str("          SAIOS BOOT INFORMATION        ");
    let _ = w.write_str("========================================");
    //   // Print metadata fields explicitly for validation
    // println!(
    //     "Magic Check:   0x{:X} (Expected: 0x{:X})",
    //     (&*BOOT_INFO).magic,
    //     efi_main::SAIOS_BOOT_MAGIC
    // );
    // println!(
    //     "Boot Version:  {}.{}",
    //     (&*BOOT_INFO).version >> 16,
    //     (&*BOOT_INFO).version & 0xFFFF
    // );
    // println!("Struct Size:   {} bytes", (&*BOOT_INFO).size);
    // println!("----------------------------------------");

    // // Print the sub-structures using the derived Debug trait
    // println!("{:#?}", (&*BOOT_INFO).framebuffer);
    // println!("{:#?}", (&*BOOT_INFO).memorymap);
    // println!("{:#?}", (&*BOOT_INFO).acpi);
    // println!("{:#?}", (&*BOOT_INFO).smbios);
    // println!("{:#?}", (&*BOOT_INFO).cpu);
    // println!("{:#?}", (&*BOOT_INFO).firmware);

    // println!("========================================");


    loop {
        core::hint::spin_loop();
    }
   
    BOOT_INFO = boot_info;

let magic = unsafe { (*BOOT_INFO).magic };
unsafe {
    let fb = (*BOOT_INFO).framebuffer.base as *mut u32;
}
loop {
    core::hint::spin_loop();
}
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Extract the panic message and file/line location
    let message = info.message();
    let location = info.location();

    // if let Some(loc) = location {
    //     let _ = println!("Panic occurred at {}:{}:{}", loc.file(), loc.line(), message);
    // } else {
    //     let _ = println!("Panic occurred: {}", message);
    // }
    loop {
        core::hint::spin_loop();
    }
}
