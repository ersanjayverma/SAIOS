use core::fmt::{self, Write};

const COM1: u16 = 0x3F8;

#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nostack, preserves_flags),
        );
    }
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
   unsafe {
        core::arch::asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nostack, preserves_flags)
        );
    }
    value
}

pub struct Serial;

impl Serial {
    pub fn init() {
        unsafe {
            outb(COM1 + 1, 0x00);
            outb(COM1 + 3, 0x80);
            outb(COM1 + 0, 0x03);
            outb(COM1 + 1, 0x00);
            outb(COM1 + 3, 0x03);
            outb(COM1 + 2, 0xC7);
            outb(COM1 + 4, 0x0B);
        }
    }

    fn tx_empty() -> bool {
        unsafe { (inb(COM1 + 5) & 0x20) != 0 }
    }

    pub fn write_byte(byte: u8) {
        while !Self::tx_empty() {}

        unsafe {
            outb(COM1, byte);
        }
    }
}

impl Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            match b {
                b'\n' => {
                    Serial::write_byte(b'\r');
                    Serial::write_byte(b'\n');
                }
                _ => Serial::write_byte(b),
            }
        }
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    let mut serial = Serial;
    serial.write_fmt(args).unwrap();
}