use core::fmt;

use super::backend::ConsoleBackend;

pub struct SerialConsole;

impl SerialConsole {
    pub const fn new() -> Self {
        Self
    }

    pub fn init() {
        hal::arch::x86_64::console::init_serial();
    }

    #[inline(always)]
    pub fn emergency_put_char(c: char) {
        hal::arch::x86_64::console::_print(format_args!("{}", c));
    }

    pub fn emergency_write_str(s: &str) {
        for c in s.chars() {
            Self::emergency_put_char(c);
        }
    }

    fn write_escape(args: fmt::Arguments) {
        hal::arch::x86_64::console::_print(args);
    }
}

impl ConsoleBackend for SerialConsole {
    fn put_char(&mut self, c: char) {
        hal::arch::x86_64::console::_print(format_args!("{}", c));
    }

    fn clear(&mut self) {
        // ANSI clear screen + home cursor (works in most serial terminals).
        Self::write_escape(format_args!("\x1b[2J\x1b[H"));
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        // ANSI cursor is 1-based.
        Self::write_escape(format_args!("\x1b[{};{}H", y + 1, x + 1));
    }
}
