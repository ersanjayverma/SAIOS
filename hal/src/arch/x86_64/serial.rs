//! Hardware Abstraction Layer for serial devices.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialError {
    NotInitialized,
    Unsupported,
    Busy,
    Fault,
}

pub type SerialResult<T> = Result<T, SerialError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Debug, Clone, Copy)]
pub struct SerialConfig {
    pub baud_rate: u32,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub data_bits: DataBits,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            baud_rate: 115_200,
            parity: Parity::None,
            stop_bits: StopBits::One,
            data_bits: DataBits::Eight,
        }
    }
}

pub trait SerialHal {
    /// Initialize the device.
    fn init(&mut self, config: SerialConfig) -> SerialResult<()>;

    /// Returns true if transmit register can accept another byte.
    fn can_write(&self) -> bool;

    /// Returns true if a byte is waiting.
    fn can_read(&self) -> bool;

    /// Write one byte.
    fn write_byte(&mut self, byte: u8);

    /// Read one byte.
    fn read_byte(&mut self) -> Option<u8>;

    /// Flush transmitter.
    fn flush(&mut self);

    /// Optional convenience.
    fn write_str(&mut self, s: &str) {
        for b in s.bytes() {
            if b == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(b);
        }
    }
}
