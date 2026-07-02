use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::pci;

#[derive(Clone, Debug)]
pub struct WifiInterface {
    pub name: String,
    pub backing: String,
    pub mac: [u8; 6],
    pub connected: bool,
    pub ssid: Option<String>,
    pub signal_dbm: i8,
    pub ipv4: Option<String>,
}

struct WifiState {
    initialized: bool,
    interfaces: Vec<WifiInterface>,
}

impl WifiState {
    fn new() -> Self {
        Self {
            initialized: false,
            interfaces: Vec::new(),
        }
    }
}

static STATE: StaticCell<Option<WifiState>> = StaticCell::new(None);
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

fn with_state_mut<R>(f: impl FnOnce(&mut WifiState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(WifiState::new());
            }
            slot.as_mut().expect("wifi state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&WifiState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(WifiState::new());
            }
            slot.as_ref().expect("wifi state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn synth_mac(bus: u8, device: u8, function: u8, index: u8) -> [u8; 6] {
    [0x02, 0xB2, bus, device, function, index]
}

fn looks_like_wifi(dev: &pci::PciDevice) -> bool {
    if dev.class != 0x02 {
        return false;
    }

    // Accept explicit wireless subclass and known vendor IDs often used by Wi-Fi chipsets.
    dev.subclass == 0x80
        || matches!(dev.vendor_id, 0x8086 | 0x168C | 0x14E4 | 0x10EC)
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

fn rescan_locked(state: &mut WifiState) {
    let existing = state.interfaces.clone();
    state.interfaces.clear();

    let mut index = 0usize;
    for dev in pci::devices() {
        if !looks_like_wifi(&dev) {
            continue;
        }

        let name = format!("wlan{}", index);
        let previous_ip = existing
            .iter()
            .find(|iface| iface.name == name)
            .and_then(|iface| iface.ipv4.clone());

        state.interfaces.push(WifiInterface {
            name,
            backing: format!("pci {:02x}:{:02x}.{}", dev.bus, dev.device, dev.function),
            mac: synth_mac(dev.bus, dev.device, dev.function, index as u8),
            connected: true,
            ssid: Some("SAIOS-NET".to_string()),
            signal_dbm: -52,
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

pub fn interfaces() -> Vec<WifiInterface> {
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
