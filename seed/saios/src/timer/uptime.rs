use core::time::Duration;

pub fn uptime_ns() -> u64 {
    super::clock::uptime_ns()
}

pub fn uptime_ms() -> u64 {
    super::clock::uptime_ms()
}

pub fn uptime() -> Duration {
    super::clock::uptime()
}
