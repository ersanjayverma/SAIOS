use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

#[derive(Clone, Debug)]
pub struct LoopbackInterface {
    pub name: String,
    pub ipv4: String,
    pub netmask: String,
}

struct LoopbackState {
    initialized: bool,
    interfaces: Vec<LoopbackInterface>,
}

impl LoopbackState {
    fn new() -> Self {
        Self {
            initialized: false,
            interfaces: Vec::new(),
        }
    }
}

static STATE: StaticCell<Option<LoopbackState>> = StaticCell::new(None);
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

fn with_state_mut<R>(f: impl FnOnce(&mut LoopbackState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(LoopbackState::new());
            }
            slot.as_mut().expect("loopback state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&LoopbackState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(LoopbackState::new());
            }
            slot.as_ref().expect("loopback state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

pub fn init() {
    with_state_mut(|state| {
        if state.initialized {
            return;
        }

        state.interfaces.clear();
        state.interfaces.push(LoopbackInterface {
            name: "lo".to_string(),
            ipv4: "127.0.0.1".to_string(),
            netmask: "255.0.0.0".to_string(),
        });
        state.initialized = true;
    });
}

pub fn interfaces() -> Vec<LoopbackInterface> {
    init();
    with_state(|state| state.interfaces.clone())
}
