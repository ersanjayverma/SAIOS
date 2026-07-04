use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::io::{inl, outl};
use hal::arch::x86_64::sync::StaticCell;

const CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const CONFIG_DATA_PORT: u16 = 0xCFC;
const CONFIG_ADDRESS_ENABLE: u32 = 0x8000_0000;

#[derive(Debug, Copy, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,

    pub vendor_id: u16,
    pub device_id: u16,

    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,

    pub revision: u8,
}

#[derive(Debug, Copy, Clone)]
pub struct PciBar {
    pub index: u8,
    pub base: u64,
    pub is_io: bool,
    pub is_64bit: bool,
}

struct PciDatabase {
    initialized: bool,
    devices: Vec<PciDevice>,
}

impl PciDatabase {
    const fn new() -> Self {
        Self {
            initialized: false,
            devices: Vec::new(),
        }
    }
}

static DB: StaticCell<PciDatabase> = StaticCell::new(PciDatabase::new());
static DB_LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while DB_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    DB_LOCK.store(false, Ordering::Release);
}

fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    CONFIG_ADDRESS_ENABLE
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

pub fn read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = config_address(bus, device, function, offset);
    outl(CONFIG_ADDRESS_PORT, address);
    inl(CONFIG_DATA_PORT)
}

pub fn read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = read_u32(bus, device, function, offset & !0x02);
    let shift = ((offset & 0x02) * 8) as u32;
    ((value >> shift) & 0xFFFF) as u16
}

pub fn read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let value = read_u32(bus, device, function, offset & !0x03);
    let shift = ((offset & 0x03) * 8) as u32;
    ((value >> shift) & 0xFF) as u8
}

pub fn write_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = config_address(bus, device, function, offset);
    outl(CONFIG_ADDRESS_PORT, address);
    outl(CONFIG_DATA_PORT, value);
}

pub fn write_u16(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let aligned = offset & !0x02;
    let shift = ((offset & 0x02) * 8) as u32;
    let mask = !(0xFFFFu32 << shift);
    let current = read_u32(bus, device, function, aligned);
    let next = (current & mask) | ((value as u32) << shift);
    write_u32(bus, device, function, aligned, next);
}

pub fn read_bar(dev: &PciDevice, index: u8) -> Option<PciBar> {
    if index >= 6 {
        return None;
    }

    let offset = 0x10u8.saturating_add(index.saturating_mul(4));
    let low = read_u32(dev.bus, dev.device, dev.function, offset);
    if low == 0 || low == 0xFFFF_FFFF {
        return None;
    }

    if (low & 0x1) != 0 {
        let base = (low & 0xFFFF_FFFC) as u64;
        if base == 0 {
            return None;
        }

        return Some(PciBar {
            index,
            base,
            is_io: true,
            is_64bit: false,
        });
    }

    let bar_type = (low >> 1) & 0x3;
    let is_64bit = bar_type == 0x2;
    let base = if is_64bit {
        if index >= 5 {
            return None;
        }
        let high = read_u32(dev.bus, dev.device, dev.function, offset.saturating_add(4));
        ((high as u64) << 32) | ((low & 0xFFFF_FFF0) as u64)
    } else {
        (low & 0xFFFF_FFF0) as u64
    };

    if base == 0 {
        return None;
    }

    Some(PciBar {
        index,
        base,
        is_io: false,
        is_64bit,
    })
}

fn enumerate_into(devices: &mut Vec<PciDevice>) {
    devices.clear();

    for bus in 0u16..=255 {
        for device in 0u8..32 {
            for function in 0u8..8 {
                let id = read_u32(bus as u8, device, function, 0x00);
                let vendor_id = (id & 0xFFFF) as u16;
                if vendor_id == 0xFFFF {
                    continue;
                }

                let device_id = ((id >> 16) & 0xFFFF) as u16;
                let class_reg = read_u32(bus as u8, device, function, 0x08);

                let revision = (class_reg & 0xFF) as u8;
                let prog_if = ((class_reg >> 8) & 0xFF) as u8;
                let subclass = ((class_reg >> 16) & 0xFF) as u8;
                let class = ((class_reg >> 24) & 0xFF) as u8;

                devices.push(PciDevice {
                    bus: bus as u8,
                    device,
                    function,
                    vendor_id,
                    device_id,
                    class,
                    subclass,
                    prog_if,
                    revision,
                });
            }
        }
    }
}

pub fn init() {
    lock();
    let db = unsafe { &mut *DB.get() };
    if !db.initialized {
        enumerate_into(&mut db.devices);
        db.initialized = true;
    }
    unlock();
}

pub fn devices() -> Vec<PciDevice> {
    init();

    devices_snapshot()
}

/// Returns currently cached PCI devices without probing hardware.
pub fn devices_snapshot() -> Vec<PciDevice> {
    lock();
    let db = unsafe { &*DB.get() };
    let snapshot = db.devices.clone();
    unlock();

    snapshot
}

pub fn class_name(class: u8) -> &'static str {
    match class {
        0x01 => "Mass Storage",
        0x02 => "Network",
        0x03 => "Display",
        0x06 => "Bridge",
        0x0C => "Serial Bus",
        _ => "Other",
    }
}
