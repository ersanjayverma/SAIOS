use core::cell::UnsafeCell;

use hal::timer::TimerHal;
use hal::timer::tsc::TscTimer;
use hal::timer::uefi::UefiTimer;

pub mod clock;
pub mod deadline;
pub mod scheduler;
pub mod sleep;
pub mod types;
pub mod uptime;

pub struct TimerManager {
    system_timer: &'static mut dyn TimerHal,
    boot_counter: u64,
}

impl TimerManager {
    pub fn new() -> Self {
        let timer = choose_best_timer();
        timer.enable();
        let boot_counter = timer.counter();
        Self {
            system_timer: timer,
            boot_counter,
        }
    }

    pub fn initialize_timers(&mut self) {
        self.system_timer.enable();
    }

    pub fn calibrate(&mut self) {}

    pub fn choose_best_timer(&mut self) {
        self.system_timer = choose_best_timer();
        self.system_timer.enable();
        self.boot_counter = self.system_timer.counter();
    }

    pub fn monotonic_ns(&self) -> u64 {
        let freq = self.system_timer.frequency_hz();
        let delta = self.system_timer.counter().wrapping_sub(self.boot_counter);
        if freq == 0 {
            return delta;
        }

        delta.saturating_mul(1_000_000_000) / freq
    }

    pub fn monotonic_ms(&self) -> u64 {
        self.monotonic_ns() / 1_000_000
    }

    pub fn system_timer_name(&self) -> &'static str {
        self.system_timer.name()
    }

    pub fn system_timer_hz(&self) -> u64 {
        self.system_timer.frequency_hz()
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

fn choose_best_timer() -> &'static mut dyn TimerHal {
    let tsc = unsafe { &mut *TSC_TIMER.get() };
    tsc.init();
    if tsc.frequency_hz() != 0 {
        return tsc;
    }

    unsafe { &mut *UEFI_TIMER.get() }
}

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
