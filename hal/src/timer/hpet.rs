use super::{ClockEvent, ClockSource, HalDevice};

pub struct HpetTimer {
    enabled: bool,
    frequency_hz: u64,
    periodic_hz: u64,
    oneshot_ns: u64,
}

impl HpetTimer {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            frequency_hz: 0,
            periodic_hz: 0,
            oneshot_ns: 0,
        }
    }
}

impl ClockSource for HpetTimer {
    fn init(&mut self) {
        self.frequency_hz = 10_000_000; // HPET frequency is typically 10 MHz
    }
    fn frequency_hz(&self) -> u64 {
        self.frequency_hz
    }
    fn counter(&self) -> u64 {
        0
    }
}
impl HalDevice for HpetTimer {
    fn name(&self) -> &'static str {
        "hpet"
    }
}
impl ClockEvent for HpetTimer {
    fn set_periodic(&mut self, hz: u64) {
        self.periodic_hz = hz;
    }

    fn set_oneshot(&mut self, ns: u64) {
        self.oneshot_ns = ns;
    }

    fn enable(&mut self) {
        self.enabled = true;
    }

    fn disable(&mut self) {
        self.enabled = false;
    }
}
