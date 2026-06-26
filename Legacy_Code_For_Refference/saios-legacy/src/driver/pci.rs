//! PCI bus scanner — enumerates all devices on buses 0-255.

use alloc::format;
use alloc::vec::Vec;

const CFG_ADDR: u16 = 0xCF8;
const CFG_DATA: u16 = 0xCFC;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

impl PciDevice {
    pub fn class_name(&self) -> &'static str {
        match (self.class, self.subclass) {
            (0x00, 0x01) => "VGA-compat",
            (0x01, 0x00) => "SCSI",
            (0x01, 0x01) => "IDE",
            (0x01, 0x06) => "SATA (AHCI)",
            (0x02, 0x00) => "Ethernet",
            (0x03, 0x00) => "VGA",
            (0x04, 0x01) => "Multimedia Audio",
            (0x06, 0x00) => "Host Bridge",
            (0x06, 0x01) => "ISA Bridge",
            (0x06, 0x04) => "PCI-to-PCI Bridge",
            (0x0C, 0x03) => "USB",
            _ => "Unknown",
        }
    }
}

pub fn scan() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for bus in 0u8..=255 {
        for dev in 0u8..32 {
            for func in 0u8..8 {
                let id = read(bus, dev, func, 0x00);
                if id == 0xFFFF_FFFF {
                    continue;
                }
                let vendor = (id & 0xFFFF) as u16;
                let device = (id >> 16) as u16;
                let class_reg = read(bus, dev, func, 0x08);
                let class = (class_reg >> 24) as u8;
                let subclass = (class_reg >> 16) as u8;
                let prog_if = (class_reg >> 8) as u8;
                devices.push(PciDevice {
                    bus,
                    dev,
                    func,
                    vendor,
                    device,
                    class,
                    subclass,
                    prog_if,
                });
                // If not multi-function, skip remaining functions
                let hdr = (read(bus, dev, func, 0x0C) >> 16) as u8;
                if func == 0 && hdr & 0x80 == 0 {
                    break;
                }
            }
        }
    }
    devices
}

fn read(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        crate::arch::port_write_u32(CFG_ADDR, addr);
        crate::arch::port_read_u32(CFG_DATA)
    }
}
