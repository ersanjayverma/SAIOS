use core::fmt::{self, Write};

use crate::driver::serial::{COM1, Serial};
use super::sink::ConsoleSink;

/// A console backed by the 16550 UART.
///
/// Prefer the singleton-backed `_print` / `println!` macros for
/// general logging.  This struct is kept for explicit console
/// operations (e.g. `clear`).
pub struct SerialConsole {
    serial: Serial,
}

impl SerialConsole {
    pub fn new() -> Self {
        Self {
            serial: Serial::new(COM1),
        }
    }
}

impl ConsoleSink for SerialConsole {
    fn put_char(&mut self, ch: char) {
        self.serial.write_byte(ch as u8);
    }
}

impl Write for SerialConsole {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.serial.write_str(s)
    }
}
