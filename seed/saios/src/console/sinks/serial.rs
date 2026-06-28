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
}
