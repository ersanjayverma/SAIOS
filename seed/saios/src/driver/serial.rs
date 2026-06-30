//! arch/x86_64/serial.rs
//!
//! 16550 UART driver for x86_64.
//! Early boot, polling mode, no_std.

use core::fmt::{self, Write};
use hal::arch::serial::{
    SerialHal,
    SerialConfig,
    SerialResult,
};
use hal::arch::x86_64::io::{inb, outb};
const DATA: u16 = 0;
const IER: u16 = 1;
const FCR: u16 = 2;
const LCR: u16 = 3;
const MCR: u16 = 4;
const LSR: u16 = 5;
const SCR: u16 = 7;

// Line Status Register
const LSR_DATA_READY: u8 = 1 << 0;
const LSR_TX_EMPTY: u8 = 1 << 5;

// FIFO Control Register
const FCR_ENABLE: u8 = 1 << 0;
const FCR_CLEAR_RX: u8 = 1 << 1;
const FCR_CLEAR_TX: u8 = 1 << 2;
const FCR_TRIGGER_14: u8 = 0b11 << 6;

/// Standard PC serial ports.
pub const COM1: u16 = 0x3F8;
pub const COM2: u16 = 0x2F8;
pub const COM3: u16 = 0x3E8;
pub const COM4: u16 = 0x2E8;

/// 16550 UART.
pub struct Serial {
    base: u16,
}
impl Serial {
    /// Creates a new serial port.
    pub const fn new(base: u16) -> Self {
        Self { base }
    }
    #[inline(always)]
    fn transmitter_ready(&self) -> bool {
        (inb(self.base + LSR) & LSR_TX_EMPTY) != 0
    }

    #[inline(always)]
    fn receiver_ready(&self) -> bool {
        (inb(self.base + LSR) & LSR_DATA_READY) != 0
    }
    /// Returns true if UART appears to exist.
    pub fn detect(&self) -> bool {
        outb(self.base + SCR, 0xAE);
        inb(self.base + SCR) == 0xAE
    }
    pub fn write_byte(&mut self, byte: u8) {
        while !self.transmitter_ready() {}
        outb(self.base + DATA, byte);
    }

}
impl SerialHal for Serial {
    /// Initializes the UART for 115200 8N1.
    fn init(&mut self, _config: SerialConfig) -> SerialResult<()> {
        // Disable interrupts.
        outb(self.base + IER, 0x00);

        // Enable DLAB.
        outb(self.base + LCR, 0x80);

        // Divisor = 1 (115200 baud).
        outb(self.base + DATA, 0x01);
        outb(self.base + IER, 0x00);

        // 8 bits, no parity, one stop bit.
        outb(self.base + LCR, 0x03);

        // Enable FIFO.
        outb(
            self.base + FCR,
            FCR_ENABLE | FCR_CLEAR_RX | FCR_CLEAR_TX | FCR_TRIGGER_14,
        );

        // IRQs disabled, RTS/DSR set.
        outb(self.base + MCR, 0x03);
        Ok(())
    }

    /// Writes one byte.
    fn write_byte(&mut self, byte: u8) {
        Serial::write_byte(self, byte);
    }
    fn can_write(&self) -> bool {
        self.transmitter_ready()
    }
    fn can_read(&self) -> bool {
        self.receiver_ready()
    }
    /// Reads one byte if available.
    fn read_byte(&mut self) -> Option<u8> {
        if self.receiver_ready() {
            Some(inb(self.base + DATA))
        } else {
            None
        }
    }

    /// Flushes the transmitter.
    fn flush(&mut self) {
        while !self.transmitter_ready() {}
    }
}

impl Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            match byte {
                b'\n' => {
                    self.write_byte(b'\r');
                    self.write_byte(b'\n');
                }
                _ => self.write_byte(byte),
            }
        }

        Ok(())
    }
}

/// Kernel print backend.
pub fn _print(args: fmt::Arguments) {
    let mut serial = Serial::new(COM1);
    serial.write_fmt(args).unwrap();
}
