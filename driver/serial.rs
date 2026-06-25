//! 16550 UART serial driver — COM1 at I/O port 0x3F8.
//! Used for debug output visible in QEMU's terminal (-serial stdio).

use spin::Mutex;

const COM1: u16 = 0x3F8;

pub static SERIAL: Mutex<Serial> = Mutex::new(Serial::new(COM1));

pub struct Serial {
    base: u16,
}

impl Serial {
    const fn new(base: u16) -> Self {
        Self { base }
    }

    pub fn init(&mut self) {
        unsafe {
            crate::arch::port_write_u8(self.base + 1, 0x00); // disable interrupts
            crate::arch::port_write_u8(self.base + 3, 0x80); // enable DLAB (set baud divisor)
            crate::arch::port_write_u8(self.base, 0x03); // divisor low: 38400 baud
            crate::arch::port_write_u8(self.base + 1, 0x00); // divisor high
            crate::arch::port_write_u8(self.base + 3, 0x03); // 8 bits, no parity, one stop bit
            crate::arch::port_write_u8(self.base + 2, 0xC7); // enable FIFO, clear, 14-byte threshold
            crate::arch::port_write_u8(self.base + 4, 0x0B); // IRQs enabled, RTS/DSR set
        }
    }

    fn is_transmit_ready(&mut self) -> bool {
        unsafe { crate::arch::port_read_u8(self.base + 5) & 0x20 != 0 }
    }

    pub fn write_byte(&mut self, byte: u8) {
        while !self.is_transmit_ready() {}
        unsafe {
            crate::arch::port_write_u8(self.base, byte);
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
    }
}

impl core::fmt::Write for Serial {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.write_str(s);
        Ok(())
    }
}

pub fn init() {
    SERIAL.lock().init();
}

/// IRQ-safe serial write.  The SERIAL lock is taken in both thread context
/// (via `serial_print!`) and IRQ context (the timer/keyboard handlers log here),
/// so the hold MUST run with interrupts disabled — otherwise an IRQ that fires
/// while a thread holds the lock would spin on it forever (deadlock).
#[doc(hidden)]
pub fn _serial_print(args: core::fmt::Arguments) {
    use core::fmt::Write;
    crate::arch::without_interrupts(|| {
        SERIAL.lock().write_fmt(args).ok();
    });
}

/// Print to the serial console.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::driver::serial::_serial_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! serial_println {
    ()              => ($crate::serial_print!("\n"));
    ($($arg:tt)*)   => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}
