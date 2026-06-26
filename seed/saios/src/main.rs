#![no_std]
#![no_main]

use core::fmt::{self, Write};
use efi_main::SaiosBootInfo;
static mut VGA_BUFFER: *mut u8 = 0xb8000 as *mut u8;

struct Writer;

impl Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for (i, byte) in s.bytes().enumerate() {
            unsafe {
                *VGA_BUFFER.add(i * 2) = byte;
                *VGA_BUFFER.add(i * 2 + 1) = 0x0f; // white on black
            }
        }
        Ok(())
    }
}

macro_rules! println {
    ($($arg:tt)*) => ({
        let _ = Writer.write_fmt(format_args!($($arg)*));
    })
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: &SaiosBootInfo) -> ! {
    let vga_buffer = 0xb8000 as *mut u8;
    *vga_buffer.offset(0) = b'H';
    *vga_buffer.offset(1) = 0x0f; // white on black
    *vga_buffer.offset(2) = b'i';
    *vga_buffer.offset(3) = 0x0f;
    println!("Hello, SAIOS!");
     loop {
        core::hint::spin_loop();
    }
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
