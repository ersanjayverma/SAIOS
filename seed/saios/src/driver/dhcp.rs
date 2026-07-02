use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::driver::{ethernet, wifi};
use crate::timer;

#[derive(Clone, Debug)]
pub struct DhcpLease {
    pub interface: String,
    pub address: String,
    pub subnet_mask: String,
    pub gateway: String,
    pub dns_server: String,
    pub lease_seconds: u32,
    pub acquired_at_ticks: u64,
}

struct DhcpState {
    initialized: bool,
    leases: Vec<DhcpLease>,
}

impl DhcpState {
    fn new() -> Self {
        Self {
            initialized: false,
            leases: Vec::new(),
        }
    }
}

static STATE: StaticCell<Option<DhcpState>> = StaticCell::new(None);
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

fn with_state_mut<R>(f: impl FnOnce(&mut DhcpState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(DhcpState::new());
            }
            slot.as_mut().expect("dhcp state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&DhcpState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(DhcpState::new());
            }
            slot.as_ref().expect("dhcp state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn lease_for_ethernet(index: usize, interface: &str) -> DhcpLease {
    let host = 100u8.saturating_add(index as u8);
    DhcpLease {
        interface: interface.to_string(),
        address: format!("192.168.10.{}", host),
        subnet_mask: "255.255.255.0".to_string(),
        gateway: "192.168.10.1".to_string(),
        dns_server: "1.1.1.1".to_string(),
        lease_seconds: 3600,
        acquired_at_ticks: timer::ticks(),
    }
}

fn lease_for_wifi(index: usize, interface: &str) -> DhcpLease {
    let host = 100u8.saturating_add(index as u8);
    DhcpLease {
        interface: interface.to_string(),
        address: format!("192.168.20.{}", host),
        subnet_mask: "255.255.255.0".to_string(),
        gateway: "192.168.20.1".to_string(),
        dns_server: "8.8.8.8".to_string(),
        lease_seconds: 3600,
        acquired_at_ticks: timer::ticks(),
    }
}

pub fn init() {
    with_state_mut(|state| {
        if state.initialized {
            return;
        }
        renew_all_locked(state);
        state.initialized = true;
    });
}

fn renew_all_locked(state: &mut DhcpState) {
    state.leases.clear();

    let eth = ethernet::interfaces();
    for (index, iface) in eth.iter().enumerate() {
        let lease = lease_for_ethernet(index, iface.name.as_str());
        ethernet::set_ipv4(iface.name.as_str(), Some(lease.address.as_str()));
        state.leases.push(lease);
    }

    let wlan = wifi::interfaces();
    for (index, iface) in wlan.iter().enumerate() {
        let lease = lease_for_wifi(index, iface.name.as_str());
        wifi::set_ipv4(iface.name.as_str(), Some(lease.address.as_str()));
        state.leases.push(lease);
    }
}

pub fn renew_all() {
    with_state_mut(|state| {
        renew_all_locked(state);
        state.initialized = true;
    });
}

pub fn clear() {
    with_state_mut(|state| {
        for iface in ethernet::interfaces() {
            ethernet::set_ipv4(iface.name.as_str(), None);
        }
        for iface in wifi::interfaces() {
            wifi::set_ipv4(iface.name.as_str(), None);
        }
        state.leases.clear();
    });
}

pub fn leases() -> Vec<DhcpLease> {
    init();
    with_state(|state| state.leases.clone())
}

pub fn lease_for(interface: &str) -> Option<DhcpLease> {
    init();
    with_state(|state| {
        state
            .leases
            .iter()
            .find(|lease| lease.interface == interface)
            .cloned()
    })
}
