use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;

use hal::arch::x86_64::sync::StaticCell;

use crate::pci;

#[derive(Clone, Debug)]
pub struct EthernetInterface {
    pub name: String,
    pub backing: String,
    pub mac: [u8; 6],
    pub link_up: bool,
    pub speed_mbps: u32,
    pub ipv4: Option<String>,
}

struct EthernetState {
    initialized: bool,
    interfaces: Vec<EthernetInterface>,
}

impl EthernetState {
    fn new() -> Self {
        Self {
            initialized: false,
            interfaces: Vec::new(),
        }
    }
}

static STATE: StaticCell<Option<EthernetState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    hal::arch::x86_64::sync::spinlock_acquire(&LOCK);
}

fn unlock() {
    hal::arch::x86_64::sync::spinlock_release(&LOCK);
}

fn with_state_mut<R>(f: impl FnOnce(&mut EthernetState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(EthernetState::new());
            }
            slot.as_mut().expect("ethernet state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&EthernetState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(EthernetState::new());
            }
            slot.as_ref().expect("ethernet state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn synth_mac(bus: u8, device: u8, function: u8, index: u8) -> [u8; 6] {
    [0x02, 0xA1, bus, device, function, index]
}

fn infer_speed_mbps(prog_if: u8) -> u32 {
    match prog_if {
        0x00 => 1000,
        0x01 => 100,
        _ => 1000,
    }
}

pub fn init() {
    with_state_mut(|state| {
        if state.initialized {
            return;
        }
        rescan_locked(state);
        state.initialized = true;
    });
}

fn rescan_locked(state: &mut EthernetState) {
    let existing = state.interfaces.clone();
    state.interfaces.clear();

    let mut index = 0usize;
    for dev in pci::devices() {
        if dev.class != 0x02 || dev.subclass != 0x00 {
            continue;
        }

        let name = format!("eth{}", index);
        let previous_ip = existing
            .iter()
            .find(|iface| iface.name == name)
            .and_then(|iface| iface.ipv4.clone());

        state.interfaces.push(EthernetInterface {
            name,
            backing: format!("pci {:02x}:{:02x}.{}", dev.bus, dev.device, dev.function),
            mac: synth_mac(dev.bus, dev.device, dev.function, index as u8),
            link_up: true,
            speed_mbps: infer_speed_mbps(dev.prog_if),
            ipv4: previous_ip,
        });
        index = index.saturating_add(1);
    }
}

pub fn rescan() {
    with_state_mut(|state| {
        rescan_locked(state);
        state.initialized = true;
    });
}

pub fn interfaces() -> Vec<EthernetInterface> {
    init();
    with_state(|state| state.interfaces.clone())
}

pub fn set_ipv4(interface: &str, ip: Option<&str>) {
    with_state_mut(|state| {
        if let Some(iface) = state.interfaces.iter_mut().find(|i| i.name == interface) {
            iface.ipv4 = ip.map(ToString::to_string);
        }
    });
}
