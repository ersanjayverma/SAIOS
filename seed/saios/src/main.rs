#![no_std]
#![no_main]

use core::fmt::{self, Write};
use efi_main::SaiosBootInfo;
static mut BOOT_INFO: *const SaiosBootInfo = core::ptr::null();
static mut CURSOR: usize = 0;

pub struct Writer;

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            let fb = (*BOOT_INFO).framebuffer.base as *mut u32;
            let stride = (*BOOT_INFO).framebuffer.stride;
            let width = (*BOOT_INFO).framebuffer.width;
            let height = (*BOOT_INFO).framebuffer.height;

            for byte in s.bytes() {
                match byte {
                    b'\n' => {
                        CURSOR = ((CURSOR / stride) + 1) * stride;
                    }
                    _ => {
                        if CURSOR < stride * height {
                            // White pixel for now
                            fb.add(CURSOR).write_volatile(0x00FFFFFF);
                            CURSOR += 1;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut writer = Writer;
        let _ = writer.write_fmt(format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n");
    };

    ($fmt:expr) => {
        $crate::print!(concat!($fmt, "\n"));
    };

    ($fmt:expr, $($arg:tt)*) => {
        $crate::print!(concat!($fmt, "\n"), $($arg)*);
    };
}
pub fn clear_framebuffer() {
    unsafe {
        let fb = (*BOOT_INFO).framebuffer.base as *mut u32;

        let pixels =
            (*BOOT_INFO).framebuffer.stride * (*BOOT_INFO).framebuffer.height;

        for i in 0..pixels {
            fb.add(i).write_volatile(0x00800000); // SAIOS Blue (BGR)
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info:*const SaiosBootInfo) -> ! {
    BOOT_INFO = boot_info;
    clear_framebuffer(); // Clear framebuffer to blue

    println!("========================================");
    println!("          SAIOS BOOT INFORMATION        ");
    println!("========================================");
      // Print metadata fields explicitly for validation
    println!(
        "Magic Check:   0x{:X} (Expected: 0x{:X})",
        (&*BOOT_INFO).magic,
        efi_main::SAIOS_BOOT_MAGIC
    );
    println!(
        "Boot Version:  {}.{}",
        (&*BOOT_INFO).version >> 16,
        (&*BOOT_INFO).version & 0xFFFF
    );
    println!("Struct Size:   {} bytes", (&*BOOT_INFO).size);
    println!("----------------------------------------");

    // Print the sub-structures using the derived Debug trait
    println!("{:#?}", (&*BOOT_INFO).framebuffer);
    println!("{:#?}", (&*BOOT_INFO).memorymap);
    println!("{:#?}", (&*BOOT_INFO).acpi);
    println!("{:#?}", (&*BOOT_INFO).smbios);
    println!("{:#?}", (&*BOOT_INFO).cpu);
    println!("{:#?}", (&*BOOT_INFO).firmware);

    println!("========================================");


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
