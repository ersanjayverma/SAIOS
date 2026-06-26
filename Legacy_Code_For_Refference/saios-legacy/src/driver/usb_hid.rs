//! USB controller detection and deferred HID bring-up.
//!
//! Current status: PARTIAL. The module scans PCI for USB controllers and logs
//! them, but deliberately defers BIOS handoff because the native USB keyboard
//! and mouse path is not mature enough to replace firmware-provided PS/2
//! emulation. Do not describe this as a complete USB stack.

use alloc::vec::Vec;
use x86_64::instructions::port::Port;

// PCI class/subclass/progif for USB controllers
const USB_CLASS: u8 = 0x0C;
const USB_SUBCLASS: u8 = 0x03;
const PROGIF_UHCI: u8 = 0x00;
const PROGIF_OHCI: u8 = 0x10;
const PROGIF_EHCI: u8 = 0x20;
const PROGIF_XHCI: u8 = 0x30;

/// Detected USB controller type.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::upper_case_acronyms)]
pub enum UsbControllerType {
    UHCI,
    OHCI,
    EHCI,
    XHCI,
}

pub struct UsbController {
    pub ctype: UsbControllerType,
    pub bar_base: u64,
    pub pci_bus: u8,
    pub pci_dev: u8,
}

/// Scan PCI for USB controllers and log them.
///
/// NOTE: We deliberately do NOT perform BIOS handoff here.
/// The BIOS provides PS/2 keyboard/mouse emulation through USB HID.
/// Taking ownership of the xHCI/EHCI controller before we have a USB HID
/// driver would break keyboard and mouse input.
///
/// BIOS handoff stays deferred until the native USB input path is mature enough
/// to replace firmware-provided PS/2 emulation without losing keyboard/mouse IO.
pub fn init() {
    let controllers = find_usb_controllers();
    for c in &controllers {
        crate::println!(
            "[usb] detected {:?} controller at {:#x} (handoff deferred)",
            c.ctype,
            c.bar_base
        );
    }
    if controllers.is_empty() {
        crate::println!("[usb] no USB controller found");
    }
}

/// Release xHCI controller from BIOS ownership to OS (XHCI BIOS handoff).
/// Without this, BIOS may intercept USB interrupts causing keyboard issues.
fn xhci_handoff(c: &UsbController) {
    // xHCI Extended Capabilities: walk the linked list starting at HCCPARAMS1
    let base = c.bar_base;
    let hccparams1 = unsafe { mmio_r32(base, 0x10) };
    let ecp_offset = ((hccparams1 >> 16) & 0xFFFF) as u64 * 4;
    if ecp_offset == 0 {
        return;
    }

    let mut cap = base + ecp_offset;
    loop {
        let cap_id = unsafe { mmio_r32(base, (cap - base) as u32) } & 0xFF;
        let next_ptr = (unsafe { mmio_r32(base, (cap - base) as u32) } >> 8) & 0xFF;

        if cap_id == 1 {
            // USB Legacy Support capability (USBLEGSUP)
            let usblegsup_off = (cap - base) as u32;
            let legsup = unsafe { mmio_r32(base, usblegsup_off) };
            if legsup & (1 << 16) != 0 {
                // BIOS owns it — request OS ownership
                unsafe {
                    mmio_w32(base, usblegsup_off, legsup | (1 << 24));
                }
                // Wait up to 1s for BIOS to release
                for _ in 0..1_000_000u32 {
                    if unsafe { mmio_r32(base, usblegsup_off) } & (1 << 16) == 0 {
                        break;
                    }
                    x86_64::instructions::nop();
                }
            }
        }

        if next_ptr == 0 {
            break;
        }
        cap += next_ptr as u64 * 4;
    }
}

/// Release EHCI controller from BIOS (EECP BIOS handoff).
fn ehci_handoff(c: &UsbController) {
    let pci_caps_ptr = pci_read(c.pci_bus, c.pci_dev, 0, 0x34) & 0xFF;
    let mut ptr = pci_caps_ptr as u8;
    while ptr != 0 {
        let cap_id = pci_read(c.pci_bus, c.pci_dev, 0, ptr) & 0xFF;
        let next_ptr = (pci_read(c.pci_bus, c.pci_dev, 0, ptr) >> 8) & 0xFF;
        if cap_id == 0x01 {
            // EHCI BIOS owned
            let val = pci_read(c.pci_bus, c.pci_dev, 0, ptr);
            if val & (1 << 16) != 0 {
                pci_write(c.pci_bus, c.pci_dev, 0, ptr, val | (1 << 24));
                for _ in 0..100_000u32 {
                    if pci_read(c.pci_bus, c.pci_dev, 0, ptr) & (1 << 16) == 0 {
                        break;
                    }
                    x86_64::instructions::nop();
                }
            }
        }
        ptr = next_ptr as u8;
    }
}

fn find_usb_controllers() -> Vec<UsbController> {
    let mut result = Vec::new();
    for bus in 0u8..=7 {
        for dev in 0u8..32 {
            for func in 0u8..8 {
                let id = pci_read(bus, dev, func, 0x00);
                if id == 0xFFFF_FFFF {
                    continue;
                }
                let class_reg = pci_read(bus, dev, func, 0x08);
                let class = (class_reg >> 24) as u8;
                let subclass = (class_reg >> 16) as u8;
                let progif = (class_reg >> 8) as u8;
                if class != USB_CLASS || subclass != USB_SUBCLASS {
                    continue;
                }

                let bar_idx = if progif == PROGIF_EHCI || progif == PROGIF_XHCI {
                    0x10
                } else {
                    0x20
                };
                let bar = pci_read(bus, dev, func, bar_idx);
                if bar & 1 != 0 {
                    continue;
                } // I/O BAR

                let ctype = match progif {
                    PROGIF_UHCI => UsbControllerType::UHCI,
                    PROGIF_OHCI => UsbControllerType::OHCI,
                    PROGIF_EHCI => UsbControllerType::EHCI,
                    PROGIF_XHCI => UsbControllerType::XHCI,
                    _ => continue,
                };

                // Enable bus mastering + MMIO
                let cmd = pci_read(bus, dev, func, 0x04);
                pci_write(bus, dev, func, 0x04, cmd | 0x06);

                result.push(UsbController {
                    ctype,
                    bar_base: (bar & !0xF) as u64,
                    pci_bus: bus,
                    pci_dev: dev,
                });
            }
        }
    }
    result
}

unsafe fn mmio_r32(base: u64, off: u32) -> u32 {
    unsafe { core::ptr::read_volatile((base + off as u64) as *const u32) }
}
unsafe fn mmio_w32(base: u64, off: u32, v: u32) {
    unsafe {
        core::ptr::write_volatile((base + off as u64) as *mut u32, v);
    }
}
fn pci_read(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    let a = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (off as u32 & 0xFC);
    unsafe {
        Port::<u32>::new(0xCF8).write(a);
        Port::<u32>::new(0xCFC).read()
    }
}
fn pci_write(bus: u8, dev: u8, func: u8, off: u8, val: u32) {
    let a = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (off as u32 & 0xFC);
    unsafe {
        Port::<u32>::new(0xCF8).write(a);
        Port::<u32>::new(0xCFC).write(val);
    }
}
