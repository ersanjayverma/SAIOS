//! Intel e1000 / e1000e Gigabit Ethernet driver.
//!
//! Supports:
//!   82540EM  PCI 8086:100E  - QEMU default NIC
//!   82545EM  PCI 8086:100F  - VirtualBox default NIC
//!   82573L   PCI 8086:109A  - e1000e
//!
//! Registers accessed via MMIO (BAR0) or I/O port (BAR1).

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering, fence};
use spin::Mutex;

use crate::network_contract::NetworkContract;

// -- PCI IDs ----------------------------------------------------------------

const INTEL_VENDOR: u16 = 0x8086;
const E1000_DEVICES: &[u16] = &[
    0x100E, // 82540EM  - QEMU e1000
    0x100F, // 82545EM  - VirtualBox
    0x109A, // 82573L   - e1000e
    0x10D3, // 82574L
    0x10EA, // 82577LM
    0x1502, // 82579LM
    0x1503, // 82579V
];

// -- Register offsets (MMIO) ------------------------------------------------

const CTRL: u32 = 0x0000; // Device Control
const STATUS: u32 = 0x0008; // Device Status
const EECD: u32 = 0x0010; // EEPROM/Flash Control
const EERD: u32 = 0x0014; // EEPROM Read
const ICR: u32 = 0x00C0; // Interrupt Cause Read
const IMS: u32 = 0x00D0; // Interrupt Mask Set
const IMC: u32 = 0x00D8; // Interrupt Mask Clear
const RCTL: u32 = 0x0100; // Receive Control
const TCTL: u32 = 0x0400; // Transmit Control
const TIPG: u32 = 0x0410; // Transmit Inter-Packet Gap
const RDBAL: u32 = 0x2800; // RX Descriptor Base Low
const RDBAH: u32 = 0x2804; // RX Descriptor Base High
const RDLEN: u32 = 0x2808; // RX Descriptor Ring Length
const RDH: u32 = 0x2810; // RX Descriptor Head
const RDT: u32 = 0x2818; // RX Descriptor Tail
const TDBAL: u32 = 0x3800; // TX Descriptor Base Low
const TDBAH: u32 = 0x3804; // TX Descriptor Base High
const TDLEN: u32 = 0x3808; // TX Descriptor Ring Length
const TDH: u32 = 0x3810; // TX Descriptor Head
const TDT: u32 = 0x3818; // TX Descriptor Tail
const RAL: u32 = 0x5400; // Receive Address Low[0]
const RAH: u32 = 0x5404; // Receive Address High[0]
const MTA: u32 = 0x5200; // Multicast Table Array (128 x u32)

// CTRL bits
const CTRL_RST: u32 = 1 << 26;
const CTRL_SLU: u32 = 1 << 6; // Set Link Up
const CTRL_ASDE: u32 = 1 << 5; // Auto-Speed Detect Enable

// RCTL bits
const RCTL_EN: u32 = 1 << 1;
const RCTL_SBP: u32 = 1 << 2;
const RCTL_UPE: u32 = 1 << 3; // Unicast promiscuous
const RCTL_MPE: u32 = 1 << 4; // Multicast promiscuous
const RCTL_LBM: u32 = 3 << 6; // Loopback mode (clear)
const RCTL_BAM: u32 = 1 << 15; // Broadcast accept
const RCTL_BSIZE: u32 = 0 << 16; // 2048-byte buffers (00)
const RCTL_SECRC: u32 = 1 << 26; // Strip CRC

// TCTL bits
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3; // Pad short packets
const TCTL_CT: u32 = 0x0F << 4; // collision threshold
const TCTL_COLD: u32 = 0x40 << 12; // collision distance (full duplex)

// TX descriptor CMD bits
const CMD_EOP: u8 = 1 << 0; // End of packet
const CMD_IFCS: u8 = 1 << 1; // Insert FCS
const CMD_RS: u8 = 1 << 3; // Report status

// -- Descriptor structures --------------------------------------------------

const NUM_TX_DESC: usize = 32;
// Large RX ring (256 x 2 KiB = 512 KiB) so a server blasting a full TCP window
// during a multi-MB download can't overflow it before we drain - overflow was
// dropping window tails and triggering massive retransmits (apt index truncated).
const NUM_RX_DESC: usize = 256;
const RX_BUF_SIZE: usize = 2048;

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
struct TxDesc {
    buf_addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
struct RxDesc {
    buf_addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

// -- Driver state structure (wrapped in Mutex) ------------------------------

#[repr(C, align(4096))]
struct TxRing([TxDesc; NUM_TX_DESC]);

#[repr(C, align(4096))]
struct RxRing([RxDesc; NUM_RX_DESC]);

#[repr(C, align(4096))]
struct TxBufs([[u8; 2048]; NUM_TX_DESC]);

#[repr(C, align(4096))]
struct RxBufs([[u8; RX_BUF_SIZE]; NUM_RX_DESC]);

struct DriverState {
    tx_ring: TxRing,
    rx_ring: RxRing,
    tx_bufs: TxBufs,
    rx_bufs: RxBufs,
    mmio_base: u64,
    pci_bus: u8,
    pci_dev: u8,
    tx_tail: usize,
    tx_head: usize,
    rx_tail: usize,
    nic_present: bool,
}

// -- Global driver state ----------------------------------------------------

static DRIVER_STATE: Mutex<DriverState> = Mutex::new(DriverState {
    tx_ring: TxRing(
        [TxDesc {
            buf_addr: 0,
            length: 0,
            cso: 0,
            cmd: 0,
            status: 0,
            css: 0,
            special: 0,
        }; NUM_TX_DESC],
    ),
    rx_ring: RxRing(
        [RxDesc {
            buf_addr: 0,
            length: 0,
            checksum: 0,
            status: 0,
            errors: 0,
            special: 0,
        }; NUM_RX_DESC],
    ),
    tx_bufs: TxBufs([[0u8; 2048]; NUM_TX_DESC]),
    rx_bufs: RxBufs([[0u8; RX_BUF_SIZE]; NUM_RX_DESC]),
    mmio_base: 0,
    pci_bus: 0,
    pci_dev: 0,
    tx_tail: 0,
    tx_head: 0,
    rx_tail: 0,
    nic_present: false,
});

// -- Public API -------------------------------------------------------------

pub fn probe() -> bool {
    // Scan all PCI buses - VirtualBox may place the NIC on bus 0 or higher
    for bus in 0u8..=255 {
        for dev in 0u8..32 {
            let id = pci_read(bus, dev, 0, 0);
            if id == 0xFFFF_FFFF {
                continue;
            }
            let vendor = (id & 0xFFFF) as u16;
            let device = (id >> 16) as u16;
            if vendor == INTEL_VENDOR && E1000_DEVICES.contains(&device) {
                // Enable bus mastering + MMIO space
                let cmd = pci_read(bus, dev, 0, 0x04);
                pci_write(bus, dev, 0, 0x04, cmd | 0x06);

                // BAR0 is the 32-bit MMIO base address register
                let bar0_raw = pci_read(bus, dev, 0, 0x10);
                // Bit 0 = 0 means memory BAR; skip I/O BARs (bit 0 = 1)
                let mmio_base = if bar0_raw & 1 != 0 {
                    // Unexpected I/O BAR - try BAR2
                    let bar2 = pci_read(bus, dev, 0, 0x18);
                    if bar2 & 1 != 0 {
                        continue;
                    }
                    (bar2 & !0xF) as u64
                } else {
                    (bar0_raw & !0xF) as u64
                };

                if mmio_base == 0 {
                    continue;
                }

                // Initialize driver state
                init(mmio_base, bus, dev);
                return true;
            }
        }
        // Stop after bus 3 on first pass; only continue if nothing found yet
        let state = DRIVER_STATE.lock();
        if bus == 3 && !state.nic_present && bus < 7 {
            continue;
        }
        if bus >= 7 {
            break;
        }
    }
    false
}

/// MMIO 32-bit read. Must be called with an unsafe block.
#[inline]
unsafe fn mmio_r32(base: u64, reg: u32) -> u32 {
    unsafe { core::ptr::read_volatile((base + reg as u64) as *const u32) }
}

/// MMIO 32-bit write. Must be called with an unsafe block.
#[inline]
unsafe fn mmio_w32(base: u64, reg: u32, val: u32) {
    unsafe {
        core::ptr::write_volatile((base + reg as u64) as *mut u32, val);
    }
}

/// Read MMIO register without unsafe (for external callers).
/// Uses a temporary unsafe block internally.
#[allow(dead_code)]
unsafe fn safe_mmio_r32(base: u64, reg: u32) -> u32 {
    unsafe { mmio_r32(base, reg) }
}

fn init(mmio_base: u64, pci_bus: u8, pci_dev: u8) {
    // Lock the driver state for initialization
    let mut state = DRIVER_STATE.lock();

    // -- 1. Software reset ----------------------------------------------
    // Write RST bit, wait for hardware to clear it (self-clearing).
    unsafe {
        mmio_w32(mmio_base, CTRL, CTRL_RST);
    }
    for _ in 0..200_000u32 {
        crate::arch::nop();
    }
    let mut tries = 0u32;
    while unsafe { mmio_r32(mmio_base, CTRL) } & CTRL_RST != 0 {
        tries += 1;
        if tries > 1_000_000 {
            break;
        } // timeout guard
        crate::arch::nop();
    }

    // -- 2. Set link up + auto-speed detect ----------------------------
    unsafe {
        mmio_w32(mmio_base, CTRL, CTRL_SLU | CTRL_ASDE);
    }

    // -- 3. Disable all interrupts -------------------------------------
    unsafe {
        mmio_w32(mmio_base, 0x00D8, 0xFFFF_FFFF);
    } // IMC - mask all
    unsafe {
        mmio_r32(mmio_base, 0x00C0);
    } // ICR read to clear pending

    // -- 4. Clear multicast table --------------------------------------
    for i in 0u32..128 {
        unsafe {
            mmio_w32(mmio_base, MTA + i * 4, 0);
        }
    }

    // -- 5. Read MAC address -------------------------------------------
    // Strategy: try RAL/RAH first (most reliable in VMs), then EEPROM,
    // finally generate a locally-administered MAC so the NIC still works.
    let mac = {
        let ral = unsafe { mmio_r32(mmio_base, RAL) };
        let rah = unsafe { mmio_r32(mmio_base, RAH) };
        if ral != 0 || (rah & 0xFF) != 0 {
            // RAL/RAH are populated (typical in VirtualBox after reset)
            [
                (ral & 0xFF) as u8,
                (ral >> 8 & 0xFF) as u8,
                (ral >> 16 & 0xFF) as u8,
                (ral >> 24) as u8,
                (rah & 0xFF) as u8,
                (rah >> 8 & 0xFF) as u8,
            ]
        } else {
            // RAL/RAH cleared by reset - read from EEPROM
            let eeprom = eeprom_read_mac(mmio_base);
            if eeprom != [0u8; 6] {
                eeprom
            } else {
                // Last resort: locally-administered MAC
                crate::println!("[e1000] WARNING: could not read MAC, using fallback");
                [0x02, 0x00, 0x00, 0x00, 0x00, 0x01]
            }
        }
    };

    NetworkContract::set_identity(mac, NetworkContract::default_ip(), "e1000");

    let ip = NetworkContract::ip();
    crate::println!(
        "[e1000] MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}  IP {}.{}.{}.{}",
        mac[0],
        mac[1],
        mac[2],
        mac[3],
        mac[4],
        mac[5],
        ip[0],
        ip[1],
        ip[2],
        ip[3]
    );

    // Initialize rings and buffers using helper that takes raw pointers
    init_tx(mmio_base, &mut state);
    init_rx(mmio_base, &mut state);

    state.mmio_base = mmio_base;
    state.pci_bus = pci_bus;
    state.pci_dev = pci_dev;
    state.nic_present = true;

    crate::println!(
        "[e1000] ready - TX={} RX={} descriptors",
        NUM_TX_DESC,
        NUM_RX_DESC
    );
}

/// Initialize TX ring - sets up descriptors and configures hardware
fn init_tx(base: u64, state: &mut DriverState) {
    // Give each TX descriptor a buffer
    // For a struct field that is itself a tuple struct [T; N], we need to:
    // 1. addr_of!(*state.tx_ring) -> *const TxRing
    // 2. dereference to get &TxRing, then access .0 (the array)
    // 3. addr_of! of the first element of the array
    let tx_ring_ptr = core::ptr::addr_of!(state.tx_ring.0) as *const TxDesc;
    let tx_phys = tx_ring_ptr as u64;

    for i in 0..NUM_TX_DESC {
        let tx_buf_ptr = core::ptr::addr_of!(state.tx_bufs.0[i]) as *const u8;
        let buf_addr = tx_buf_ptr as u64;
        let desc = core::ptr::addr_of_mut!(state.tx_ring.0[i]);
        unsafe {
            (*desc).buf_addr = buf_addr;
            (*desc).status = 1; // mark as done
        }
    }

    unsafe {
        mmio_w32(base, TDBAH, (tx_phys >> 32) as u32);
    }
    unsafe {
        mmio_w32(base, TDBAL, tx_phys as u32);
    }
    unsafe {
        mmio_w32(base, TDLEN, (NUM_TX_DESC * 16) as u32);
    }
    unsafe {
        mmio_w32(base, TDH, 0);
    }
    unsafe {
        mmio_w32(base, TDT, 0);
    }
    unsafe {
        mmio_w32(base, TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD);
    }
    unsafe {
        mmio_w32(base, TIPG, 0x0060200A);
    } // standard inter-packet gap
}

/// Initialize RX ring - sets up descriptors and configures hardware
fn init_rx(base: u64, state: &mut DriverState) {
    let rx_ring_ptr = core::ptr::addr_of!(state.rx_ring.0) as *const RxDesc;
    let rx_phys = rx_ring_ptr as u64;

    for i in 0..NUM_RX_DESC {
        let rx_buf_ptr = core::ptr::addr_of!(state.rx_bufs.0[i]) as *const u8;
        let buf_addr = rx_buf_ptr as u64;
        let desc = core::ptr::addr_of_mut!(state.rx_ring.0[i]);
        unsafe {
            (*desc).buf_addr = buf_addr;
        }
    }

    unsafe {
        mmio_w32(base, RDBAH, (rx_phys >> 32) as u32);
    }
    unsafe {
        mmio_w32(base, RDBAL, rx_phys as u32);
    }
    unsafe {
        mmio_w32(base, RDLEN, (NUM_RX_DESC * 16) as u32);
    }
    unsafe {
        mmio_w32(base, RDH, 0);
    }
    // Set tail to last descriptor so device owns all but the last slot
    state.rx_tail = NUM_RX_DESC - 1;
    unsafe {
        mmio_w32(base, RDT, state.rx_tail as u32);
    }
    // RCTL_UPE = unicast promiscuous - accept ALL unicast even if MAC differs.
    // Essential during early init when the MAC read might be imperfect.
    unsafe {
        mmio_w32(
            base,
            RCTL,
            RCTL_EN | RCTL_UPE | RCTL_MPE | RCTL_BAM | RCTL_BSIZE | RCTL_SECRC,
        );
    }
}

/// Dump the key RX/TX/control registers and the ring physical addresses the
/// hardware actually holds vs what we intended.  RX=0 => the RX engine never
/// DMAs; this reveals a bad ring base, RDLEN, RCTL, or link-down.
#[allow(dead_code)]
pub unsafe fn dump_regs(base: u64) {
    unsafe {
        let state = DRIVER_STATE.lock();

        let rx_ring_ptr = core::ptr::addr_of!(state.rx_ring.0) as *const RxDesc;
        let tx_ring_ptr = core::ptr::addr_of!(state.tx_ring.0) as *const TxDesc;
        let rx_phys = rx_ring_ptr as u64;
        let tx_phys = tx_ring_ptr as u64;

        drop(state);

        crate::println!(
            "[e1000] STATUS={:#010x} (LU bit1={})  RCTL={:#010x}  TCTL={:#010x}",
            safe_mmio_r32(base, STATUS),
            (safe_mmio_r32(base, STATUS) >> 1) & 1,
            safe_mmio_r32(base, RCTL),
            safe_mmio_r32(base, TCTL)
        );
        crate::println!(
            "[e1000] RX ring: want={:#x}  RDBAL={:#010x} RDBAH={:#010x} RDLEN={} RDH={} RDT={}",
            rx_phys,
            safe_mmio_r32(base, RDBAL),
            safe_mmio_r32(base, RDBAH),
            safe_mmio_r32(base, RDLEN),
            safe_mmio_r32(base, RDH),
            safe_mmio_r32(base, RDT)
        );
        crate::println!(
            "[e1000] TX ring: want={:#x}  TDBAL={:#010x} TDBAH={:#010x} TDLEN={} TDH={} TDT={}",
            tx_phys,
            safe_mmio_r32(base, TDBAL),
            safe_mmio_r32(base, TDBAH),
            safe_mmio_r32(base, TDLEN),
            safe_mmio_r32(base, TDH),
            safe_mmio_r32(base, TDT)
        );
        crate::println!(
            "[e1000] RAL={:#010x} RAH={:#010x}  IMS={:#010x}",
            safe_mmio_r32(base, RAL),
            safe_mmio_r32(base, RAH),
            safe_mmio_r32(base, IMS)
        );
    }
}

/// Re-dump RX state after traffic: descriptor statuses + RDH/RDT, so we can see
/// whether the hardware advanced the head / set any DD bits.
#[allow(dead_code)]
pub fn dump_rx_status() {
    let state = DRIVER_STATE.lock();
    let base = state.mmio_base;

    crate::println!(
        "[e1000] post: STATUS={:#x} LU={} RDH={} RDT={} TDH={} TDT={} RX_TAIL={}",
        unsafe { safe_mmio_r32(base, STATUS) },
        (unsafe { safe_mmio_r32(base, STATUS) } >> 1) & 1,
        unsafe { safe_mmio_r32(base, RDH) },
        unsafe { safe_mmio_r32(base, RDT) },
        unsafe { safe_mmio_r32(base, TDH) },
        unsafe { safe_mmio_r32(base, TDT) },
        state.rx_tail
    );

    for i in 0..8usize {
        let s = state.rx_ring.0[i].status;
        crate::print!("[rx{} st={:02x}] ", i, s);
    }
    crate::println!();
}

/// Conclusive test of whether MMIO writes actually reach the device:
///   1. PCI command register (Memory Space + Bus Master must be enabled).
///   2. Write garbage to the READ-ONLY STATUS register and read it back.
///   3. Write/read the R/W TDT register.
#[allow(dead_code)]
pub unsafe fn mmio_probe() {
    let state = DRIVER_STATE.lock();
    let base = state.mmio_base;
    let cmd = pci_read(state.pci_bus, state.pci_dev, 0, 0x04) & 0xFFFF;

    drop(state);

    crate::println!(
        "[e1000] PCI cmd={:#06x}  MemSpace={} BusMaster={}  BAR0={:#x}",
        cmd,
        (cmd >> 1) & 1,
        (cmd >> 2) & 1,
        pci_read(0, 0, 0, 0x10) & !0xF
    );

    let st_before = unsafe { safe_mmio_r32(base, STATUS) };
    unsafe {
        mmio_w32(base, STATUS, 0x1234_5678);
    } // STATUS is read-only
    let st_after = unsafe { safe_mmio_r32(base, STATUS) };
    crate::println!(
        "[e1000] RO-write test: STATUS before={:#x} after-writing-0x12345678={:#x} => {}",
        st_before,
        st_after,
        if st_after == 0x1234_5678 {
            "CACHED (device NOT seeing MMIO writes)"
        } else {
            "device-owned (MMIO writes DO reach the device)"
        }
    );

    let t0 = unsafe { safe_mmio_r32(base, TDT) };
    unsafe {
        mmio_w32(base, TDT, 7);
    }
    let a = unsafe { safe_mmio_r32(base, TDT) };
    unsafe {
        mmio_w32(base, TDT, 0);
    }
    let b = unsafe { safe_mmio_r32(base, TDT) };
    crate::println!(
        "[e1000] TDT R/W test: old={} after_write_7={} after_write_0={}",
        t0,
        a,
        b
    );
}

/// Force the link up and wait for STATUS.LU.  Some emulated e1000s (notably
/// VirtualBox's 82540EM) need SLU re-asserted and a poll for the link bit before
/// they will DMA the descriptor rings.  Returns true if the link came up.
pub fn bring_link_up() -> bool {
    let state = DRIVER_STATE.lock();
    let base = state.mmio_base;

    // Re-assert Set-Link-Up + auto-speed-detect.
    let ctrl = unsafe { safe_mmio_r32(base, CTRL) };
    unsafe {
        mmio_w32(base, CTRL, ctrl | CTRL_SLU | CTRL_ASDE);
    }

    // VirtualBox's e1000 raises STATUS.LU ~5 WALL-CLOCK seconds after SLU (it
    // simulates cable/auto-negotiation with a device timer).  nop spins burn far
    // less real time than that, so wait on the timer IRQ via hlt instead.
    // Requires interrupts enabled (call from task context, not early init).
    for _ in 0..1300u32 {
        // ~13 s at the 100 Hz PIT
        if (unsafe { safe_mmio_r32(base, STATUS) } >> 1) & 1 == 1 {
            return true;
        }
        crate::arch::halt();
    }
    (unsafe { safe_mmio_r32(base, STATUS) } >> 1) & 1 == 1
}

/// Poll for received packets
pub fn poll_rx() {
    let mut state = DRIVER_STATE.lock();
    if !state.nic_present {
        return;
    }

    // Check the next RX descriptor after the last one we returned.
    let candidate = (state.rx_tail + 1) % NUM_RX_DESC;
    let status = state.rx_ring.0[candidate].status;

    if status & 0x01 == 0 {
        return;
    } // hardware hasn't written here yet

    // Ensure the DD-bit read is ordered before reading the length the device
    // DMA'd into the descriptor.
    core::sync::atomic::fence(Ordering::Acquire);
    let len = state.rx_ring.0[candidate].length as usize;
    if len > 0 && len <= RX_BUF_SIZE {
        let frame = state.rx_bufs.0[candidate][..len].to_vec();
        NetworkContract::enqueue_rx(frame, "e1000");
        RX_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    // Clear status and return descriptor to hardware.
    state.rx_ring.0[candidate].status = 0;
    let base = state.mmio_base;
    state.rx_tail = candidate;
    drop(state);
    unsafe {
        mmio_w32(base, RDT, candidate as u32);
    }
}

/// Flush pending packets from TX queue
pub fn flush_tx() {
    let mut state = DRIVER_STATE.lock();
    if !state.nic_present {
        return;
    }

    let base = state.mmio_base;
    for frame in NetworkContract::drain_tx() {
        // Wait for a free TX slot (status DD bit)
        let mut attempts = 0u32;
        // Wait briefly for the slot to free (DD set by a prior completed
        // send).  A working NIC completes in well under this; if it never
        // does, drop the frame (protocols retransmit) - NEVER spin a million
        // times, which froze the whole single-threaded kernel.
        loop {
            if state.tx_ring.0[state.tx_tail].status & 1 != 0 {
                break;
            }
            attempts += 1;
            if attempts > 10_000 {
                TX_TIMEOUT.fetch_add(1, Ordering::Relaxed);
                break; // drop this frame, keep draining the rest
            }
            crate::arch::nop();
        }
        if state.tx_ring.0[state.tx_tail].status & 1 == 0 {
            continue;
        } // still busy => skip

        let slot = state.tx_tail;
        let copy_len = frame.len().min(2048);
        state.tx_bufs.0[slot][..copy_len].copy_from_slice(&frame[..copy_len]);

        state.tx_ring.0[slot].length = copy_len as u16;
        state.tx_ring.0[slot].cmd = CMD_EOP | CMD_IFCS | CMD_RS;
        state.tx_ring.0[slot].status = 0;

        fence(Ordering::Release);
        state.tx_tail = (state.tx_tail + 1) % NUM_TX_DESC;
        unsafe {
            mmio_w32(base, TDT, state.tx_tail as u32);
        }
        TX_OK.fetch_add(1, Ordering::Relaxed);

        // Update state
        drop(state);
        state = DRIVER_STATE.lock();
    }
}

pub fn present() -> bool {
    let state = DRIVER_STATE.lock();
    state.nic_present
}

// -- Diagnostics counters ----------------------------------------------------
use core::sync::atomic::AtomicU32;
pub static TX_OK: AtomicU32 = AtomicU32::new(0);
pub static TX_TIMEOUT: AtomicU32 = AtomicU32::new(0);
pub static RX_COUNT: AtomicU32 = AtomicU32::new(0);

/// (frames transmitted, TX-completion timeouts, frames received)
#[allow(dead_code)]
pub fn net_stats() -> (u32, u32, u32) {
    (
        TX_OK.load(Ordering::Relaxed),
        TX_TIMEOUT.load(Ordering::Relaxed),
        RX_COUNT.load(Ordering::Relaxed),
    )
}

// -- EEPROM / MAC read -----------------------------------------------------

fn eeprom_read_mac(base: u64) -> [u8; 6] {
    let mut mac = [0u8; 6];
    for i in 0..3u32 {
        // EERD: start=1, addr=i
        unsafe {
            mmio_w32(base, EERD, (i << 8) | 1);
        }
        let mut v;
        loop {
            v = unsafe { mmio_r32(base, EERD) };
            if v & (1 << 4) != 0 {
                break;
            } // done bit
            crate::arch::nop();
        }
        let word = (v >> 16) as u16;
        mac[(i * 2) as usize] = (word & 0xFF) as u8;
        mac[(i * 2 + 1) as usize] = (word >> 8) as u8;
    }
    mac
}

// -- PCI helpers -----------------------------------------------------------

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
