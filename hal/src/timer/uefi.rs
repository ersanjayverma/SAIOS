use super::{ClockSource, HalDevice};

pub struct UefiTimer {
    frequency_hz: u64,
}

impl UefiTimer {
    pub const fn new() -> Self {
        Self {
            frequency_hz: 1_000_000,
        }
    }
}

impl ClockSource for UefiTimer {
    fn init(&mut self) {
        self.frequency_hz = 1_000_000; // UEFI timer frequency is typically 1 MHz
    }
  fn frequency_hz(&self) -> u64 {
        self.frequency_hz
    }
    fn counter(&self) -> u64 {
        0
    }
}
impl HalDevice for UefiTimer {
    fn name(&self) -> &'static str {
        "uefi"
    }
}
