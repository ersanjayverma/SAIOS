//! Console output support using the serial port.
//!
//! This module provides a singleton-backed `_print` / `println!` implementation
//! for use in the HAL.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};

use super::serial::{SerialConfig, SerialHal, SerialResult};

// ── Serial port singleton ─────────────────────────────────────────

/// Standard PC serial port base addresses.
pub const COM1: u16 = 0x3F8;
pub const COM2: u16 = 0x2F8;
pub const COM3: u16 = 0x3E8;
pub const COM4: u16 = 0x2E8;

const UART_DATA: u16 = 0;
const UART_INTERRUPT_ENABLE: u16 = 1;
const UART_FIFO_CONTROL: u16 = 2;
const UART_LINE_CONTROL: u16 = 3;
const UART_MODEM_CONTROL: u16 = 4;
const UART_LINE_STATUS: u16 = 5;
const UART_LSR_DATA_READY: u8 = 0x01;
const UART_LSR_TX_EMPTY: u8 = 0x20;
const UART_LCR_8N1: u8 = 0x03;
const UART_LCR_DLAB: u8 = 0x80;
const UART_FCR_ENABLE: u8 = 0x01;
const UART_FCR_CLEAR_RX: u8 = 0x02;
const UART_FCR_CLEAR_TX: u8 = 0x04;
const UART_FCR_TRIGGER_14: u8 = 0xC0;
const UART_MCR_READY: u8 = 0x03;
const UART_DIVISOR_LOW_115200: u8 = 0x01;
const UART_DIVISOR_HIGH_115200: u8 = 0x00;
const UART_INTERRUPTS_DISABLED: u8 = 0x00;

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
        (super::io::inb(self.base + UART_LINE_STATUS) & UART_LSR_TX_EMPTY) != 0
    }

    #[inline(always)]
    fn receiver_ready(&self) -> bool {
        (super::io::inb(self.base + UART_LINE_STATUS) & UART_LSR_DATA_READY) != 0
    }

    /// Blocking write of a single byte.
    fn write_byte(&mut self, byte: u8) {
        while !self.transmitter_ready() {
            // Prevent the compiler from optimizing this loop away
            core::hint::spin_loop();
        }
        super::io::outb(self.base + UART_DATA, byte);
    }

    /// Initialize the UART for 115200 8N1 with FIFO enabled.
    fn init(&mut self) -> SerialResult<()> {
        // Disable interrupts.
        super::io::outb(self.base + UART_INTERRUPT_ENABLE, UART_INTERRUPTS_DISABLED);

        // Enable DLAB (divisor latch access bit).
        super::io::outb(self.base + UART_LINE_CONTROL, UART_LCR_DLAB);

        // Divisor = 1 → 115200 baud.
        super::io::outb(self.base + UART_DATA, UART_DIVISOR_LOW_115200);
        super::io::outb(self.base + UART_INTERRUPT_ENABLE, UART_DIVISOR_HIGH_115200);

        // 8 data bits, no parity, one stop bit.
        super::io::outb(self.base + UART_LINE_CONTROL, UART_LCR_8N1);

        // Enable FIFO, clear receive/transmit queues, 14-byte trigger.
        super::io::outb(
            self.base + UART_FIFO_CONTROL,
            UART_FCR_ENABLE | UART_FCR_CLEAR_RX | UART_FCR_CLEAR_TX | UART_FCR_TRIGGER_14,
        );

        // IRQs disabled, RTS/DSR set.
        super::io::outb(self.base + UART_MODEM_CONTROL, UART_MCR_READY);
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
            Some(super::io::inb(self.base + UART_DATA))
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
static SERIAL_OUTPUT_ENABLED: AtomicBool = AtomicBool::new(true);

/// Enables or disables serial output produced by [`_print`].
pub fn set_output_enabled(enabled: bool) {
    SERIAL_OUTPUT_ENABLED.store(enabled, Ordering::Release);
}

/// Returns whether serial output is currently enabled.
pub fn output_enabled() -> bool {
    SERIAL_OUTPUT_ENABLED.load(Ordering::Acquire)
}

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
    if !output_enabled() {
        return;
    }

    // SAFETY: the serial singleton is initialised before first use
    // and we are in a single-threaded kernel context.
    unsafe {
        let serial = &mut *SERIAL.get();
        let _ = serial.write_fmt(args);
    }
}

/// Print to serial regardless of [`output_enabled`].
///
/// Intended for panic/emergency diagnostics.
pub fn _print_force(args: fmt::Arguments) {
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
