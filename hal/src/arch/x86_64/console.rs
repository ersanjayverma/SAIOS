//! Console output support using the serial port.
//!
//! This module provides a singleton-backed `_print` / `println!` implementation
//! for use in the HAL.

use core::fmt::{self, Write};

use super::serial::{SerialConfig, SerialHal, SerialResult};

// ── Serial port singleton ─────────────────────────────────────────

/// Standard PC serial port base addresses.
pub const COM1: u16 = 0x3F8;
pub const COM2: u16 = 0x2F8;
pub const COM3: u16 = 0x3E8;
pub const COM4: u16 = 0x2E8;

/// 16550 UART driver.
struct Serial {
    base: u16,
}

impl Serial {
    const fn new(base: u16) -> Self {
        Self { base }
    }

    #[inline(always)]
    fn transmitter_ready(&self) -> bool {
        (super::io::inb(self.base + 5) & 0x20) != 0 // LSR_TX_EMPTY
    }

    #[inline(always)]
    fn receiver_ready(&self) -> bool {
        (super::io::inb(self.base + 5) & 0x01) != 0 // LSR_DATA_READY
    }

    /// Blocking write of a single byte.
    fn write_byte(&mut self, byte: u8) {
        while !self.transmitter_ready() {
            // Prevent the compiler from optimizing this loop away
            core::hint::spin_loop();
        }
        super::io::outb(self.base, byte);
    }

    /// Initialize the UART for 115200 8N1 with FIFO enabled.
    fn init(&mut self) -> SerialResult<()> {
        // Disable interrupts.
        super::io::outb(self.base + 1, 0x00);

        // Enable DLAB (divisor latch access bit).
        super::io::outb(self.base + 3, 0x80);

        // Divisor = 1 → 115200 baud.
        super::io::outb(self.base, 0x01);
        super::io::outb(self.base + 1, 0x00);

        // 8 data bits, no parity, one stop bit.
        super::io::outb(self.base + 3, 0x03);

        // Enable FIFO, clear receive/transmit queues, 14-byte trigger.
        super::io::outb(
            self.base + 2,
            0x01 | 0x02 | 0x04 | 0xC0, // FCR_ENABLE | FCR_CLEAR_RX | FCR_CLEAR_TX | FCR_TRIGGER_14
        );

        // IRQs disabled, RTS/DSR set.
        super::io::outb(self.base + 4, 0x03);
        Ok(())
    }
}

impl SerialHal for Serial {
    fn init(&mut self, _config: SerialConfig) -> SerialResult<()> {
        self.init()
    }

    fn can_write(&self) -> bool {
        self.transmitter_ready()
    }

    fn can_read(&self) -> bool {
        self.receiver_ready()
    }

    fn write_byte(&mut self, byte: u8) {
        Serial::write_byte(self, byte);
    }

    fn read_byte(&mut self) -> Option<u8> {
        if self.receiver_ready() {
            Some(super::io::inb(self.base))
        } else {
            None
        }
    }

    fn flush(&mut self) {
        while !self.transmitter_ready() {
            // Prevent the compiler from optimizing this loop away
            core::hint::spin_loop();
        }
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

// ── Static singleton ──────────────────────────────────────────────

use crate::arch::x86_64::sync::StaticCell;

/// The global serial port singleton.
static SERIAL: StaticCell<Serial> = StaticCell::new(Serial::new(COM1));

/// One-time initialization of the serial singleton.
///
/// Must be called exactly once during early kernel init, before any
/// `println!` / `_print` calls.
pub fn init_serial() {
    // SAFETY: called once at boot, single-threaded context.
    unsafe {
        let serial = &mut *SERIAL.get();
        serial.init().expect("Failed to initialize serial port");
    }
}

/// Kernel print backend. Uses the static serial singleton.
pub fn _print(args: fmt::Arguments) {
    // SAFETY: the serial singleton is initialised before first use
    // and we are in a single-threaded kernel context.
    unsafe {
        let serial = &mut *SERIAL.get();
        let _ = serial.write_fmt(args);
    }
}

// ── Macros ───────────────────────────────────────────────────────

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::arch::x86_64::console::_print(format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => {{
        $crate::print!("\n");
    }};

    ($($arg:tt)*) => {{
        $crate::print!("{}\n", format_args!($($arg)*));
    }};
}
