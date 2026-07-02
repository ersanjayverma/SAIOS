use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum EventKind {
    ProcessStarted,
    ProcessStopped,
    DriverLoaded,
    DriverReloaded,
    DriverFaulted,
    DeviceAttached,
    DeviceFaulted,
    Oom,
    MountFailed,
    Irq,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::ProcessStarted => "ProcessStarted",
            EventKind::ProcessStopped => "ProcessStopped",
            EventKind::DriverLoaded => "DriverLoaded",
            EventKind::DriverReloaded => "DriverReloaded",
            EventKind::DriverFaulted => "DriverFaulted",
            EventKind::DeviceAttached => "DeviceAttached",
            EventKind::DeviceFaulted => "DeviceFaulted",
            EventKind::Oom => "Oom",
            EventKind::MountFailed => "MountFailed",
            EventKind::Irq => "Irq",
        }
    }
}

#[derive(Clone, Debug)]
pub struct EventRecord {
    pub seq: u64,
    pub kind: EventKind,
    pub source: String,
    pub detail: String,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct EventCounters {
    pub total: u64,
    pub process_started: u64,
    pub process_stopped: u64,
    pub driver_loaded: u64,
    pub driver_reloaded: u64,
    pub driver_faulted: u64,
    pub device_attached: u64,
    pub device_faulted: u64,
    pub oom: u64,
    pub mount_failed: u64,
    pub irq: u64,
}

struct EventBus {
    next_seq: u64,
    events: Vec<EventRecord>,
    counters: EventCounters,
}

impl EventBus {
    fn new() -> Self {
        Self {
            next_seq: 1,
            events: Vec::new(),
            counters: EventCounters::default(),
        }
    }

    fn publish(&mut self, kind: EventKind, source: &str, detail: &str) {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.events.push(EventRecord {
            seq,
            kind,
            source: source.to_string(),
            detail: detail.to_string(),
        });
        if self.events.len() > 512 {
            let _ = self.events.remove(0);
        }

        self.counters.total = self.counters.total.saturating_add(1);
        match kind {
            EventKind::ProcessStarted => {
                self.counters.process_started = self.counters.process_started.saturating_add(1)
            }
            EventKind::ProcessStopped => {
                self.counters.process_stopped = self.counters.process_stopped.saturating_add(1)
            }
            EventKind::DriverLoaded => {
                self.counters.driver_loaded = self.counters.driver_loaded.saturating_add(1)
            }
            EventKind::DriverReloaded => {
                self.counters.driver_reloaded = self.counters.driver_reloaded.saturating_add(1)
            }
            EventKind::DriverFaulted => {
                self.counters.driver_faulted = self.counters.driver_faulted.saturating_add(1)
            }
            EventKind::DeviceAttached => {
                self.counters.device_attached = self.counters.device_attached.saturating_add(1)
            }
            EventKind::DeviceFaulted => {
                self.counters.device_faulted = self.counters.device_faulted.saturating_add(1)
            }
            EventKind::Oom => self.counters.oom = self.counters.oom.saturating_add(1),
            EventKind::MountFailed => {
                self.counters.mount_failed = self.counters.mount_failed.saturating_add(1)
            }
            EventKind::Irq => self.counters.irq = self.counters.irq.saturating_add(1),
        }
    }
}

static BUS: StaticCell<Option<EventBus>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_bus_mut<R>(f: impl FnOnce(&mut EventBus) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *BUS.get() };
    if slot.is_none() {
        *slot = Some(EventBus::new());
    }
    let out = f(slot.as_mut().expect("event bus unavailable"));
    unlock();
    out
}

fn with_bus<R>(f: impl FnOnce(&EventBus) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *BUS.get() };
    if slot.is_none() {
        *slot = Some(EventBus::new());
    }
    let out = f(slot.as_ref().expect("event bus unavailable"));
    unlock();
    out
}

pub fn init() {
    with_bus_mut(|_| {});
}

pub fn publish(kind: EventKind, source: &str, detail: &str) {
    with_bus_mut(|b| b.publish(kind, source, detail));
}

pub fn recent(limit: usize) -> Vec<EventRecord> {
    with_bus(|b| {
        let take = core::cmp::min(limit, b.events.len());
        b.events[b.events.len().saturating_sub(take)..].to_vec()
    })
}

pub fn counters() -> EventCounters {
    with_bus(|b| b.counters)
}
