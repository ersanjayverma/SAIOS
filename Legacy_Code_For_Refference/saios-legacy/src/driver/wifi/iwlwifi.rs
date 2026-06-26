//! Intel Centrino / Wireless-N / AC / AX driver (iwlwifi).
//!
//! Supports (PCI device IDs):
//!   7260  8086:08B1  Intel Wireless-N 7260
//!   7265  8086:095A  Intel Dual Band Wireless-AC 7265
//!   8260  8086:24F3  Intel Wireless-AC 8260
//!   8265  8086:24FD  Intel Wireless-AC 8265
//!   AX200 8086:2723  Intel Wi-Fi 6 AX200
//!   AX201 8086:06F0  Intel Wi-Fi 6 AX201
//!
//! Intel Wi-Fi requires:
//!   - PCIe MMIO access
//!   - Microcode firmware loaded from /lib/firmware/iwlwifi-*.ucode
//!   - CSR (Control/Status Register) protocol
//!   - Command queue + notification queue (Tx/Rx rings)

use alloc::string::String;
use alloc::vec::Vec;
use x86_64::instructions::port::Port;

use super::WifiDriver;
use super::mac80211::MacAddr;

const INTEL_VENDOR: u16 = 0x8086;

/// Known Intel Wi-Fi PCI device IDs.
const IWL_DEVICES: &[(u16, &str)] = &[
    (0x08B1, "Intel Wireless-N 7260"),
    (0x08B2, "Intel Wireless-N 7260"),
    (0x095A, "Intel Dual Band AC 7265"),
    (0x095B, "Intel Dual Band AC 7265"),
    (0x24F3, "Intel Wireless-AC 8260"),
    (0x24FD, "Intel Wireless-AC 8265"),
    (0x2723, "Intel Wi-Fi 6 AX200"),
    (0x06F0, "Intel Wi-Fi 6 AX201"),
    (0x51F0, "Intel Wi-Fi 6E AX211"),
    (0xA840, "Intel Wi-Fi 6E AX211"),
];

// -- CSR register offsets ---------------------------------------------------

const CSR_HW_IF_CONFIG_REG: u32 = 0x000;
const CSR_INT_COALESCING: u32 = 0x004;
const CSR_INT: u32 = 0x008;
const CSR_INT_MASK: u32 = 0x00C;
const CSR_FH_INT_STATUS: u32 = 0x010;
const CSR_GPIO_IN: u32 = 0x018;
const CSR_RESET: u32 = 0x020;
const CSR_GP_CNTRL: u32 = 0x024;
const CSR_HW_REV: u32 = 0x028;
const CSR_EEPROM_REG: u32 = 0x02C;
const CSR_EEPROM_GP: u32 = 0x030;
const CSR_OTP_GP_REG: u32 = 0x034;
const CSR_GIO_REG: u32 = 0x03C;
const CSR_GP_UCODE_REG: u32 = 0x048;
const CSR_GP_DRIVER_REG: u32 = 0x050;
const CSR_UCODE_DRV_GP1: u32 = 0x054;
const CSR_UCODE_DRV_GP1_SET: u32 = 0x058;
const CSR_UCODE_DRV_GP1_CLR: u32 = 0x05C;
const CSR_UCODE_DRV_GP2: u32 = 0x060;
const CSR_LED_REG: u32 = 0x094;
const CSR_DRAM_INT_TBL_REG: u32 = 0x0A0;
const CSR_MAC_SHADOW_REG_CTRL: u32 = 0x0A8;

// Reset bits
const CSR_RESET_REG_FLAG_NEVO_RESET: u32 = 0x01000000;
const CSR_RESET_REG_FLAG_FORCE_NMI: u32 = 0x00020000;
const CSR_RESET_REG_FLAG_SW_RESET: u32 = 0x00000080;

// GP_CNTRL bits
const CSR_GP_CNTRL_REG_FLAG_MAC_CLOCK_READY: u32 = 0x00000001;
const CSR_GP_CNTRL_REG_FLAG_INIT_DONE: u32 = 0x00000004;

pub struct IwlWifi {
    mmio_base: u64,
    device_name: &'static str,
    mac: MacAddr,
    fw_loaded: bool,
}

impl IwlWifi {
    pub fn probe() -> Option<Self> {
        for bus in 0u8..=7 {
            for dev in 0u8..32 {
                let id = pci_read(bus, dev, 0, 0);
                if id == 0xFFFF_FFFF {
                    continue;
                }
                let vendor = (id & 0xFFFF) as u16;
                let device = (id >> 16) as u16;
                if vendor != INTEL_VENDOR {
                    continue;
                }
                if let Some(&(_, name)) = IWL_DEVICES.iter().find(|&&(d, _)| d == device) {
                    // Enable PCIe bus mastering + MMIO
                    let cmd = pci_read(bus, dev, 0, 0x04);
                    pci_write(bus, dev, 0, 0x04, cmd | 0x06);
                    let bar0 = (pci_read(bus, dev, 0, 0x10) as u64) & !0xF;
                    crate::println!(
                        "[iwlwifi] found {} at PCI {:02x}:{:02x} MMIO={:#x}",
                        name,
                        bus,
                        dev,
                        bar0
                    );
                    let mut drv = Self {
                        mmio_base: bar0,
                        device_name: name,
                        mac: MacAddr([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
                        fw_loaded: false,
                    };
                    drv.early_init();
                    return Some(drv);
                }
            }
        }
        None
    }

    fn early_init(&mut self) {
        unsafe {
            // Disable interrupts
            self.write32(CSR_INT_MASK, 0xFFFFFFFF);
            self.write32(CSR_INT, 0xFFFFFFFF);

            // Software reset
            self.write32(CSR_RESET, CSR_RESET_REG_FLAG_SW_RESET);
            for _ in 0..100_000u32 {
                x86_64::instructions::nop();
            }
        }
        crate::println!(
            "[iwlwifi] {} initialised (firmware not loaded yet)",
            self.device_name
        );
        crate::println!("[iwlwifi] Load firmware: copy iwlwifi-*.ucode to /lib/firmware/");
    }

    /// Attempt to load firmware from /lib/firmware/
    pub fn load_firmware(&mut self, fw_name: &str) -> Result<(), &'static str> {
        if crate::compatibility_contract::CompatibilityContract::require_placeholder_available(
            "iwlwifi.microcode.connect",
        )
        .is_err()
        {
            return Err("iwlwifi: firmware loader placeholder gated by hardware roadmap");
        }
        let path = alloc::format!("/lib/firmware/{}", fw_name);
        let data = crate::vfs_contract::VfsContract::read_file(&path)
            .map_err(|_| "iwlwifi: read failed")?;
        crate::println!(
            "[iwlwifi] loaded firmware {} ({} KiB)",
            fw_name,
            data.len() / 1024
        );
        self.fw_loaded = true;
        Ok(())
    }

    unsafe fn read32(&self, reg: u32) -> u32 {
        unsafe { core::ptr::read_volatile((self.mmio_base + reg as u64) as *const u32) }
    }
    unsafe fn write32(&self, reg: u32, val: u32) {
        unsafe {
            core::ptr::write_volatile((self.mmio_base + reg as u64) as *mut u32, val);
        }
    }
}

impl WifiDriver for IwlWifi {
    fn name(&self) -> &str {
        self.device_name
    }
    fn mac(&self) -> MacAddr {
        self.mac.clone()
    }
    fn scan(&mut self) -> Vec<super::mac80211::BeaconInfo> {
        Vec::new()
    }
    fn connect(&mut self, _ssid: &str, _password: Option<&str>) -> Result<(), &'static str> {
        if crate::compatibility_contract::CompatibilityContract::require_placeholder_available(
            "iwlwifi.microcode.connect",
        )
        .is_err()
        {
            return Err("iwlwifi: connect placeholder gated by hardware roadmap");
        }
        if !self.fw_loaded {
            return Err("iwlwifi: firmware not loaded — copy iwlwifi-*.ucode to /lib/firmware/");
        }
        Err("iwlwifi: connect not yet implemented")
    }
    fn send(&mut self, _frame: &[u8]) {}
    fn poll(&mut self) -> Vec<Vec<u8>> {
        Vec::new()
    }
    fn is_connected(&self) -> bool {
        false
    }
    fn ssid(&self) -> Option<String> {
        None
    }
    fn signal_strength(&self) -> i8 {
        -100
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
