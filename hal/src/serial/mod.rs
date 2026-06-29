//! Hardware Abstraction Layer — Serial Port
//!
//! Platform-agnostic serial port types and constants.
//! Architecture-specific I/O is provided by the kernel's `drivers::serial`.

/// Standard PC serial port base I/O addresses.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u16)]
pub enum SerialPort {
    Com1 = 0x3F8,
    Com2 = 0x2F8,
    Com3 = 0x3E8,
    Com4 = 0x2E8,
}

/// Line Status Register bit flags.
pub mod lsr {
    pub const DATA_READY: u8 = 0x01;
    pub const OVERRUN_ERR: u8 = 0x02;
    pub const PARITY_ERR: u8 = 0x04;
    pub const FRAMING_ERR: u8 = 0x08;
    pub const BREAK_INT: u8 = 0x10;
    pub const THRE: u8 = 0x20;
    pub const TEMT: u8 = 0x40;
    pub const FIFO_ERR: u8 = 0x80;
}

/// Interrupt Enable Register bit flags.
pub mod ier {
    pub const RX_AVAILABLE: u8 = 0x01;
    pub const TX_EMPTY: u8 = 0x02;
    pub const LINE_STATUS: u8 = 0x04;
    pub const MODEM_STATUS: u8 = 0x08;
}

/// Modem Control Register bit flags.
pub mod mcr {
    pub const DTR: u8 = 0x01;
    pub const RTS: u8 = 0x02;
    pub const OUT1: u8 = 0x04;
    pub const OUT2: u8 = 0x08;
    pub const LOOPBACK: u8 = 0x10;
}

/// Standard baud rate divisors for a 115200 Hz clock.
/// divisor = 115200 / (16 × baud)
pub mod baud {
    pub const B115200: u16 = 1;
    pub const B57600: u16 = 2;
    pub const B38400: u16 = 3;
    pub const B19200: u16 = 6;
    pub const B9600: u16 = 12;
    pub const B4800: u16 = 24;
    pub const B2400: u16 = 48;
    pub const B1200: u16 = 96;
}

/// Serial port configuration.
#[derive(Debug, Copy, Clone)]
pub struct SerialConfig {
    pub port: SerialPort,
    pub baud_divisor: u16,
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub fifo_enabled: bool,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: SerialPort::Com1,
            baud_divisor: baud::B38400,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: StopBits::One,
            fifo_enabled: true,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
    Mark,
    Space,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StopBits {
    One,
    OneAndHalf,
    Two,
}
