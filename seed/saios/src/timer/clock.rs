use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;

pub struct Clock {
    pub boot_ns: u64,
    pub ticks: AtomicU64,
    tick_ns: AtomicU64,
    uptime_ns: AtomicU64,
}

impl Clock {
    pub const fn new(boot_ns: u64, tick_ns: u64) -> Self {
        Self {
            boot_ns,
            ticks: AtomicU64::new(0),
            tick_ns: AtomicU64::new(tick_ns),
            uptime_ns: AtomicU64::new(0),
        }
    }
}

static CLOCK: Clock = Clock::new(0, 1_000_000);

pub fn configure_tick_ns(tick_ns: u64) {
    CLOCK.tick_ns.store(tick_ns, Ordering::Relaxed);
}

pub fn tick() {
    CLOCK.ticks.fetch_add(1, Ordering::Relaxed);
    let delta_ns = CLOCK.tick_ns.load(Ordering::Relaxed);
    CLOCK.uptime_ns.fetch_add(delta_ns, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    CLOCK.ticks.load(Ordering::Relaxed)
}

pub fn uptime_ns() -> u64 {
    CLOCK
        .boot_ns
        .saturating_add(CLOCK.uptime_ns.load(Ordering::Relaxed))
}

pub fn uptime_ms() -> u64 {
    uptime_ns() / 1_000_000
}

pub fn uptime() -> Duration {
    Duration::from_nanos(uptime_ns())
}
