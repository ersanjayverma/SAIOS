use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

#[derive(Clone, Debug)]
pub struct DnsConfig {
    pub servers: Vec<String>,
    pub search_domains: Vec<String>,
}

struct DnsState {
    initialized: bool,
    config: DnsConfig,
}

impl DnsState {
    fn new() -> Self {
        Self {
            initialized: false,
            config: DnsConfig {
                servers: Vec::new(),
                search_domains: Vec::new(),
            },
        }
    }
}

static STATE: StaticCell<Option<DnsState>> = StaticCell::new(None);
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

fn with_state_mut<R>(f: impl FnOnce(&mut DnsState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(DnsState::new());
            }
            slot.as_mut().expect("dns state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&DnsState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(DnsState::new());
            }
            slot.as_ref().expect("dns state unavailable")
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

        state.config.servers = alloc::vec!["1.1.1.1".to_string(), "8.8.8.8".to_string()];
        state.config.search_domains = alloc::vec!["saios.local".to_string()];
        state.initialized = true;
    });
}

pub fn set_servers(servers: Vec<String>) {
    with_state_mut(|state| {
        state.config.servers = servers;
        state.initialized = true;
    });
}

pub fn config() -> DnsConfig {
    init();
    with_state(|state| state.config.clone())
}
