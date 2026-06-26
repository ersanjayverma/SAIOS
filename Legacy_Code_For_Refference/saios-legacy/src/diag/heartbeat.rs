//! 1 Hz kernel heartbeat. Every 100th PIT tick (the timer fires at
//! 100 Hz, see `interrupts::init_pics`) we bump
//! [`HEARTBEAT_LAST_TICK`] for the watchdog's consumption.
//!
//! The counter still advances independently of userspace activity, but
//! it stays silent on the serial port unless a higher-level diagnostic
//! command chooses to report it.

use crate::interrupts::TIMER_IRQS;
use core::sync::atomic::{AtomicU64, Ordering};

/// Monotonic counter: number of 1 Hz heartbeats that have fired since
/// boot.  Read by the watchdog (forward-progress check) and by the
/// `diag` shell command.  Updated by [`tick`].
pub static HEARTBEAT_LAST_TICK: AtomicU64 = AtomicU64::new(0);

/// Total number of heartbeats observed since boot.
pub static HEARTBEAT_COUNT: AtomicU64 = AtomicU64::new(0);

/// PIT ticks per heartbeat.  PIT is 100 Hz (see `init_pics`), so 100
/// ticks = 1 s.  We divide by a constant rather than wall-clock time
/// so the heartbeat is robust to TSC calibration errors.
const PIT_TICKS_PER_HEARTBEAT: u64 = 100;

/// Snapshot of [`TIMER_IRQS`] taken at the previous heartbeat.  Used
/// by the watchdog: if `TIMER_IRQS - prev` is zero in 5 s, the PIT
/// has stopped firing and the system is wedged in a CLI/STI hole.
pub(crate) static LAST_TIMER_IRQS_AT_HEARTBEAT: AtomicU64 = AtomicU64::new(0);

/// Number of PIT ticks since the last heartbeat.  Updated from the
/// timer handler; when it hits [`PIT_TICKS_PER_HEARTBEAT`] we print
/// the heartbeat line.  Kept in a static so we don't need a `Mutex`
/// on the hot IRQ path.
static TICKS_SINCE_HEARTBEAT: AtomicU64 = AtomicU64::new(0);

/// One-time init: print the banner so the user can see the
/// diagnostic module came up.  The actual ticking happens from
/// `interrupts::timer_handler` via [`tick`].
pub fn init() {
    LAST_TIMER_IRQS_AT_HEARTBEAT.store(TIMER_IRQS.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// Called from the PIT timer handler. Cheap: a `fetch_add`, a compare,
/// and a likely-false branch. Updates the heartbeat counters silently.
pub fn tick() {
    let n = TICKS_SINCE_HEARTBEAT.fetch_add(1, Ordering::Relaxed) + 1;
    if n < PIT_TICKS_PER_HEARTBEAT {
        return;
    }
    TICKS_SINCE_HEARTBEAT.store(0, Ordering::Relaxed);

    let now = TIMER_IRQS.load(Ordering::Relaxed);
    let prev = LAST_TIMER_IRQS_AT_HEARTBEAT.load(Ordering::Relaxed);
    LAST_TIMER_IRQS_AT_HEARTBEAT.store(now, Ordering::Relaxed);

    let total = HEARTBEAT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    HEARTBEAT_LAST_TICK.store(total, Ordering::Relaxed);
    crate::kds::flush_aggregates();
    let _ = now.saturating_sub(prev);
}
