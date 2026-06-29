pub struct SerialSink;

impl SerialSink {
    pub const fn new() -> Self {
        Self
    }

    pub fn init(&mut self) {
        crate::drivers::serial::init();
    }

    pub fn write_str(&mut self, s: &str) {
        crate::drivers::serial::write_str(s);
    }

    pub fn write_byte(&mut self, b: u8) {
        crate::drivers::serial::write_byte(b);
    }

    pub fn write_fmt(&mut self, args: core::fmt::Arguments<'_>) {
        crate::drivers::serial::write_fmt(args);
    }

    pub fn flush(&mut self) {
        crate::drivers::serial::flush();
    }

    pub fn is_data_ready(&self) -> bool {
        crate::drivers::serial::is_data_ready()
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        crate::drivers::serial::read_byte()
    }

    pub fn try_read_byte(&mut self) -> Option<u8> {
        crate::drivers::serial::try_read_byte()
    }

    pub fn read_bytes(&mut self, buf: &mut [u8]) -> usize {
        crate::drivers::serial::read_bytes(buf)
    }

    pub fn is_present(&self) -> bool {
        crate::drivers::serial::is_present()
    }

    pub fn is_ready(&self) -> bool {
        crate::drivers::serial::is_ready()
    }
}
