//! Intel High Definition Audio (HDA) driver.
//!
//! Supports any HDA-compatible audio controller:
//!   Intel ICH6+  PCI 8086:2668  â€” VirtualBox default audio
//!   Intel ICH9   PCI 8086:293E
//!   Intel PCH    PCI 8086:1C20
//!   AMD HDA      PCI 1002:4383
//!   VirtualBox   PCI 8086:2668  (ICH AC97 or HDA depending on settings)
//!
//! # What HDA provides
//!   - PCM audio output (beep, tones, system sounds)
//!   - Microphone input
//!   - HDMI audio (if codec supports it)
//!
//! Phase 1 implementation: initialise the controller, detect the codec,
//! and provide a `beep(frequency_hz, duration_ms)` function.
//! Full PCM mixing and /dev/snd support is a Phase 7 milestone.

const HDA_VENDOR: u16 = 0x8086;
const HDA_DEVICES: &[u16] = &[
    0x2668, // ICH6 HDA
    0x27D8, // ICH7 HDA
    0x269A, // ESB2 HDA
    0x284B, // ICH8 HDA
    0x293E, // ICH9 HDA
    0x3A3E, // ICH10 HDA
    0x1C20, // 6 Series HDA
    0x1E20, // 7 Series HDA
    0x8C20, // 8 Series HDA
    0x9C20, // 9 Series LP HDA
];

// HDA MMIO register offsets
const GCAP: u32 = 0x00; // Global Capabilities
const GCTL: u32 = 0x08; // Global Control  (bit 0 = CRST, bit 1 = FCNTRL)
const STATESTS: u32 = 0x0E; // State Change Status
const CORBBASE: u32 = 0x40; // CORB Base Address
const CORBRP: u32 = 0x4C; // CORB Read Pointer
const CORBWP: u32 = 0x48; // CORB Write Pointer
const CORBCTL: u32 = 0x4C; // CORB Control (actually 0x4C = CORBRP, 0x4D = CORBCTL)
const RIRBBAS: u32 = 0x50; // RIRB Base Address
const RIRBWP: u32 = 0x58; // RIRB Write Pointer
const RINTCNT: u32 = 0x5A; // RIRB Interrupt Count
const RIRBCTL: u32 = 0x5C; // RIRB Control

static mut HDA_BASE: u64 = 0;
static mut HDA_PRESENT: bool = false;

pub fn init() {
    if let Some((bus, dev, bar)) = find_hda() {
        // Validate BAR before MMIO: zero or very low addresses indicate an
        // unconfigured or mis-read BAR that would corrupt kernel memory.
        if !(0x1000..0x1_0000_0000_0000).contains(&bar) {
            crate::println!("[hda] HDA BAR {:#x} invalid — skipping", bar);
            return;
        }
        // Disable interrupts around PCI command enable + HDA reset.
        // The MMIO write to GCTL can trigger PCI bus activity that races with
        // LAPIC interrupt delivery, causing a double fault on some platforms
        // (observed in VirtualBox ICH6 HDA emulation).
        crate::arch::without_interrupts(|| {
            let cmd = pci_read(bus, dev, 0, 0x04);
            pci_write(bus, dev, 0, 0x04, cmd | 0x06);
            unsafe {
                HDA_BASE = bar;
            }
            reset_controller(bar);
        });
        unsafe {
            HDA_PRESENT = true;
        }
        crate::println!("[hda] Intel HDA audio controller at {:#x}", bar);
    } else {
        crate::println!("[hda] no HDA audio controller found");
    }
}

fn reset_controller(base: u64) {
    unsafe {
        // Assert CRST (bit 0 = 0 means reset)
        mmio_w32(base, GCTL, 0);
        // Wait for CRST to clear
        for _ in 0..100_000u32 {
            if mmio_r32(base, GCTL) & 1 == 0 {
                break;
            }
            crate::arch::nop();
        }
        // Deassert CRST (bring controller out of reset)
        mmio_w32(base, GCTL, 1);
        // Wait for codec to be ready
        for _ in 0..500_000u32 {
            if mmio_r32(base, GCTL) & 1 != 0 {
                break;
            }
            crate::arch::nop();
        }
        // Allow time for codec to enumerate
        for _ in 0..1_000_000u32 {
            crate::arch::nop();
        }
    }
}

/// Emit a short beep via the PC speaker (always available even without HDA).
/// Falls back to port 0x61 (PC speaker) which works on all x86 machines.
pub fn beep(freq_hz: u32, duration_ms: u32) {
    // PIT channel 2 generates the tone, port 0x61 gates the PC speaker
    let divisor = 1193180u32 / freq_hz.max(1);
    unsafe {
        // Configure PIT channel 2 for square wave
        crate::arch::port_write_u8(0x43, 0xB6);
        crate::arch::port_write_u8(0x42, (divisor & 0xFF) as u8);
        crate::arch::port_write_u8(0x42, ((divisor >> 8) & 0xFF) as u8);
        // Enable speaker output (bits 0 and 1 of port 0x61)
        let old = crate::arch::port_read_u8(0x61);
        crate::arch::port_write_u8(0x61, old | 0x03);
        // Wait for duration
        let target = crate::shell::commands::boot_ticks() + (duration_ms as u64 * 100 / 1000 + 1);
        while crate::shell::commands::boot_ticks() < target {
            crate::arch::halt();
        }
        // Disable speaker
        crate::arch::port_write_u8(0x61, old & !0x03);
    }
}

pub fn present() -> bool {
    unsafe { HDA_PRESENT }
}

fn find_hda() -> Option<(u8, u8, u64)> {
    for bus in 0u8..=3 {
        for dev in 0u8..32 {
            let id = pci_read(bus, dev, 0, 0x00);
            if id == 0xFFFF_FFFF {
                continue;
            }
            let vendor = (id & 0xFFFF) as u16;
            let device = (id >> 16) as u16;
            // Check Intel HDA or AMD HDA (class 04:03 = multimedia audio)
            let class = pci_read(bus, dev, 0, 0x08);
            let cl = (class >> 24) as u8;
            let sc = (class >> 16) as u8;
            let is_hda = (vendor == HDA_VENDOR && HDA_DEVICES.contains(&device))
                || (cl == 0x04 && sc == 0x03);
            if !is_hda {
                continue;
            }
            let bar0 = pci_read(bus, dev, 0, 0x10);
            if bar0 & 1 != 0 {
                continue;
            }
            return Some((bus, dev, (bar0 & !0xF) as u64));
        }
    }
    None
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
        crate::arch::port_write_u32(0xCF8, a);
        crate::arch::port_read_u32(0xCFC)
    }
}
fn pci_write(bus: u8, dev: u8, func: u8, off: u8, val: u32) {
    let a = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (off as u32 & 0xFC);
    unsafe {
        crate::arch::port_write_u32(0xCF8, a);
        crate::arch::port_write_u32(0xCFC, val);
    }
}
