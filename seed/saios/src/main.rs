#![no_std]
#![no_main]

pub mod arch;
pub mod boot;
pub mod drivers;
pub mod fs;
pub mod graphics;
pub mod ipc;
pub mod memory;
pub mod net;
pub mod process;
pub mod scheduler;

use core::fmt::{self, Write};
use efi_main::SaiosBootInfo;
use hal::display::gop::GopDisplay;
use hal::display::PixelFormat as HalPixelFormat;
static mut BOOT_INFO: *const SaiosBootInfo = core::ptr::null();
static mut CURSOR: usize = 0;

pub struct Writer;

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        unsafe {
            let fb = (*BOOT_INFO).framebuffer.base as *mut u32;
            let stride = (*BOOT_INFO).framebuffer.stride;
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
            fb.add(i).write_volatile(0x00880000); // SAIOS Blue (BGR)
        }
    }
}

fn to_hal_pixel_format(fmt: efi_main::graphics::PixelFormat) -> HalPixelFormat {
    match fmt {
        efi_main::graphics::PixelFormat::Rgb => HalPixelFormat::Rgb,
        efi_main::graphics::PixelFormat::Bgr => HalPixelFormat::Bgr,
        efi_main::graphics::PixelFormat::Bitmask => HalPixelFormat::Rgb,
        efi_main::graphics::PixelFormat::BltOnly => HalPixelFormat::Rgb,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info:*const SaiosBootInfo) -> ! {
    unsafe {
        BOOT_INFO = boot_info;
    }

    let fb = unsafe { (*boot_info).framebuffer };
    let mut display = unsafe {
        GopDisplay::from_raw(
            fb.base as *mut u8,
            fb.width as u32,
            fb.height as u32,
            fb.stride,
            fb.bpp as u8,
            to_hal_pixel_format(fb.pixel_format),
        )
    };
    crate::graphics::software::verify::verify_primitives_on_display(&mut display);
    display.present();
    display.clear(crate::graphics::Color::rgb(77, 52, 192));
    // println!("========================================");
    // println!("          SAIOS BOOT INFORMATION        ");
    // println!("========================================");
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


}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // if let Some(loc) = location {
    //     let _ = println!("Panic occurred at {}:{}:{}", loc.file(), loc.line(), message);
    // } else {
    //     let _ = println!("Panic occurred: {}", message);
    // }
    loop {
        core::hint::spin_loop();
    }
}
