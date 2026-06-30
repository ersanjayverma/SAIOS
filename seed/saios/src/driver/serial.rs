use core::fmt::{self, Write};
use hal::arch::x86_64::io::{inb, outb};
const COM1: u16 = 0x3F8;
pub struct Serial;

impl Serial {
    pub fn init() {
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x80);
        outb(COM1 + 0, 0x03);
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03);
        outb(COM1 + 2, 0xC7);
        outb(COM1 + 4, 0x0B);
    }

    fn tx_empty() -> bool {
        (inb(COM1 + 5) & 0x20) != 0
    }

    pub fn write_byte(byte: u8) {
        while !Self::tx_empty() {}
        outb(COM1, byte);
       
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
