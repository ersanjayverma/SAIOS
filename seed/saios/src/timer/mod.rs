use core::cell::UnsafeCell;

use hal::timer::tsc::TscTimer;
use hal::timer::uefi::UefiTimer;
use hal::timer::{ClockEvent, ClockSource};

pub mod clock;
pub mod deadline;
pub mod scheduler;
pub mod sleep;
pub mod types;
pub mod uptime;

pub struct TimerManager {
    clock_source: &'static mut dyn ClockSource,
    clock_event: &'static mut dyn ClockEvent,
    boot_counter: u64,
}

impl TimerManager {
    pub fn new() -> Self {
        let clock_source = Self::choose_clock_source();
        let clock_event = Self::choose_clock_event();
        clock_source.init();
        clock_event.enable();
        let boot_counter = clock_source.counter();
        Self {
            clock_source,
            clock_event,
            boot_counter,
        }
    }

    pub fn initialize_timers(&mut self) {
        self.clock_event.enable();
    }
   
fn choose_clock_source() -> &'static mut dyn ClockSource {
    let tsc = unsafe { &mut *TSC_TIMER.get() };

    tsc.init();

    if tsc.frequency_hz() != 0 {
        return tsc;
    }

    unsafe { &mut *UEFI_TIMER.get() }
}       

fn choose_clock_event() -> &'static mut dyn ClockEvent {
    // Temporary until PIT/LAPIC exist.
   panic!("No clock event source available. Please implement PIT or LAPIC timer support."); 
}
    pub fn monotonic_ns(&self) -> u64 {
        let freq = self.clock_source.frequency_hz();
        let delta = self.clock_source.counter().wrapping_sub(self.boot_counter);
        if freq == 0 {
            return delta;
        }

        delta.saturating_mul(1_000_000_000) / freq
    }

    pub fn monotonic_ms(&self) -> u64 {
        self.monotonic_ns() / 1_000_000
    }

    pub fn system_timer_name(&self) -> &'static str {
        self.clock_source.name()
    }

    pub fn system_timer_hz(&self) -> u64 {
        self.clock_source.frequency_hz()
    }
}

struct GlobalCell<T>(UnsafeCell<T>);

impl<T> GlobalCell<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    fn get(&self) -> *mut T {
        self.0.get()
    }
}

unsafe impl<T> Sync for GlobalCell<T> {}

static TSC_TIMER: GlobalCell<TscTimer> = GlobalCell::new(TscTimer::new());
static UEFI_TIMER: GlobalCell<UefiTimer> = GlobalCell::new(UefiTimer::new());
static GLOBAL_MANAGER: GlobalCell<Option<TimerManager>> = GlobalCell::new(None);



pub fn init() {
    let manager = TimerManager::new();
    let global = unsafe { &mut *GLOBAL_MANAGER.get() };
    *global = Some(manager);
}

pub fn manager() -> &'static mut TimerManager {
    let global = unsafe { &mut *GLOBAL_MANAGER.get() };
    if global.is_none() {
        *global = Some(TimerManager::new());
    }

    global.as_mut().unwrap()
}
