use super::{ClockEvent, HalDevice};

pub struct LapicTimer {
    enabled: bool,
    periodic_hz: u64,
    oneshot_ns: u64,
}

impl LapicTimer {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            periodic_hz: 0,
            oneshot_ns: 0,
        }
    }
}
impl HalDevice for LapicTimer {
    fn name(&self) -> &'static str {
        "lapic"
    }
}
impl ClockEvent for LapicTimer {
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
