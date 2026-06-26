//! Realtek RTL8139 Fast Ethernet driver.
//! PCI 10EC:8139 - common in QEMU (-netdev user -device rtl8139).

use crate::network_contract::NetworkContract;
use alloc::vec::Vec;
use core::sync::atomic::{Ordering, fence};
use spin::Mutex;

const RTL_VENDOR: u16 = 0x10EC;
const RTL_DEVICE: u16 = 0x8139;

// I/O register offsets
const IDR0: u16 = 0x00; // MAC address (6 bytes)
const MAR0: u16 = 0x08; // Multicast filter
const TSD0: u16 = 0x10; // TX Status (x 4)
const TSAD0: u16 = 0x20; // TX Start Address (x 4)
const RBSTART: u16 = 0x30; // RX Buffer Start
const ERBCR: u16 = 0x34; // Early RX Byte Count
const ERSR: u16 = 0x36; // Early RX Status
const CR: u16 = 0x37; // Command Register
const CAPR: u16 = 0x38; // Current Address of Packet Read
const CBA: u16 = 0x3A; // Current Buffer Address
const IMR: u16 = 0x3C; // Interrupt Mask Register
const ISR: u16 = 0x3E; // Interrupt Status Register
const TCR: u16 = 0x40; // TX Configuration
const RCR: u16 = 0x44; // RX Configuration
const TCTR: u16 = 0x48; // Timer Counter
const MPC: u16 = 0x4C; // Missed Packet Counter
const CONFIG1: u16 = 0x52; // Configuration Register 1

// CR bits
const CR_RST: u8 = 0x10;
const CR_RE: u8 = 0x08;
const CR_TE: u8 = 0x04;

// RCR bits
const RCR_AAP: u32 = 1 << 0; // Accept all packets
const RCR_APM: u32 = 1 << 1; // Accept physical match
const RCR_AM: u32 = 1 << 2; // Accept multicast
const RCR_AB: u32 = 1 << 3; // Accept broadcast
const RCR_WRAP: u32 = 1 << 7; // Wrap at buffer end
const RCR_RBLEN: u32 = 0 << 11; // 8 KiB RX buffer

// TSD bits
const TSD_TOK: u32 = 1 << 15; // Transmit OK
const TSD_SIZE_MASK: u32 = 0x1FFF;

const RX_BUF_LEN: usize = 8192 + 16 + 1500; // ring + overflow space

// -- Driver state structure (wrapped in Mutex) ------------------------------

#[repr(C, align(4096))]
struct RxBuf([u8; RX_BUF_LEN]);

#[repr(C, align(4))]
struct TxBuf([u8; 1536]);

struct DriverState {
    rx_buf: RxBuf,
    tx_buf0: TxBuf,
    tx_buf1: TxBuf,
    tx_buf2: TxBuf,
    tx_buf3: TxBuf,
    io_base: u16,
    rx_off: usize, // current RX read offset
    tx_slot: u8,   // current TX descriptor (0-3)
    rtl_ok: bool,
}

// -- Global driver state ----------------------------------------------------

static DRIVER_STATE: Mutex<DriverState> = Mutex::new(DriverState {
    rx_buf: RxBuf([0u8; RX_BUF_LEN]),
    tx_buf0: TxBuf([0u8; 1536]),
    tx_buf1: TxBuf([0u8; 1536]),
    tx_buf2: TxBuf([0u8; 1536]),
    tx_buf3: TxBuf([0u8; 1536]),
    io_base: 0,
    rx_off: 0,
    tx_slot: 0,
    rtl_ok: false,
});

// -- Public API -------------------------------------------------------------

pub fn probe() -> bool {
    for bus in 0u8..=7 {
        for dev in 0u8..32 {
            let id = pci_read(bus, dev, 0, 0);
            if id == 0xFFFF_FFFF {
                continue;
            }
            if (id & 0xFFFF) as u16 == RTL_VENDOR && (id >> 16) as u16 == RTL_DEVICE {
                let cmd = pci_read(bus, dev, 0, 0x04);
                pci_write(bus, dev, 0, 0x04, cmd | 0x05);
                let bar1 = (pci_read(bus, dev, 0, 0x14) & 0xFFFC) as u16;
                if bar1 > 0 {
                    // Initialize driver state
                    init(bar1);
                    return true;
                }
            }
        }
    }
    false
}

fn init(io: u16) {
    // Lock driver state for initialization
    let mut state = DRIVER_STATE.lock();

    // Power on
    unsafe {
        pio_w8(io + CONFIG1, 0x00);
    }

    // Software reset
    unsafe {
        pio_w8(io + CR, CR_RST);
    }
    while unsafe { pio_r8(io + CR) } & CR_RST != 0 {
        crate::arch::nop();
    }

    // Read MAC
    let mut mac = [0u8; 6];
    for (i, byte) in mac.iter_mut().enumerate() {
        unsafe {
            *byte = pio_r8(io + IDR0 + i as u16);
        }
    }
    NetworkContract::set_identity(mac, NetworkContract::default_ip(), "rtl8139");

    let ip = NetworkContract::ip();
    crate::println!(
        "[rtl8139] MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}  IP {}.{}.{}.{}",
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

    // Set RX buffer pointer
    let rx_ptr = core::ptr::addr_of!(state.rx_buf.0) as u32;
    unsafe {
        pio_w32(io + RBSTART, rx_ptr);
    }

    // Set TX buffer pointers
    let tx_ptr0 = core::ptr::addr_of!(state.tx_buf0.0) as u32;
    let tx_ptr1 = core::ptr::addr_of!(state.tx_buf1.0) as u32;
    let tx_ptr2 = core::ptr::addr_of!(state.tx_buf2.0) as u32;
    let tx_ptr3 = core::ptr::addr_of!(state.tx_buf3.0) as u32;

    unsafe {
        pio_w32(io + TSAD0, tx_ptr0);
    }
    unsafe {
        pio_w32(io + TSAD0 + 4, tx_ptr1);
    }
    unsafe {
        pio_w32(io + TSAD0 + 8, tx_ptr2);
    }
    unsafe {
        pio_w32(io + TSAD0 + 12, tx_ptr3);
    }

    // Enable RX + TX
    unsafe {
        pio_w8(io + CR, CR_RE | CR_TE);
    }

    // Accept: unicast + broadcast; 8 KiB ring; wrap
    unsafe {
        pio_w32(io + RCR, RCR_APM | RCR_AB | RCR_WRAP | RCR_RBLEN);
    }

    // Mask all interrupts (we poll)
    unsafe {
        pio_w16(io + IMR, 0x0000);
    }

    state.io_base = io;
    state.rtl_ok = true;

    drop(state);
}

pub fn poll_rx() {
    let mut state = DRIVER_STATE.lock();
    if !state.rtl_ok {
        return;
    }

    let io = state.io_base;

    loop {
        let cr = unsafe { pio_r8(io + CR) };
        if cr & 0x01 != 0 {
            break;
        } // Buffer empty (BUFE bit)

        let off = state.rx_off;
        let buf: &[u8] = &state.rx_buf.0;

        // Each received packet is preceded by a 4-byte header:
        // [status u16][length u16][packet data...][padding]
        let status = u16::from_le_bytes([buf[off], buf[off + 1]]);
        let pkt_len = u16::from_le_bytes([buf[off + 2], buf[off + 3]]) as usize;

        if status & 0x01 == 0 {
            break;
        } // ROK bit not set
        if !(4..=1536).contains(&pkt_len) {
            break;
        }

        // Copy packet (subtract 4-byte CRC)
        let data_start = (off + 4) % RX_BUF_LEN;
        let data_len = pkt_len - 4;
        let mut frame = alloc::vec![0u8; data_len];
        for i in 0..data_len {
            frame[i] = buf[(data_start + i) % RX_BUF_LEN];
        }
        NetworkContract::enqueue_rx(frame, "rtl8139");

        // Advance read pointer (align to 4 bytes)
        let new_rx_off = (off + 4 + pkt_len + 3) & !3;
        let new_rx_off = new_rx_off % RX_BUF_LEN;
        unsafe {
            pio_w16(io + CAPR, (new_rx_off as u16).wrapping_sub(16));
        }

        // Update state - do not drop the lock yet, just update the field
        state.rx_off = new_rx_off;
    }
}

pub fn flush_tx() {
    let mut state = DRIVER_STATE.lock();
    if !state.rtl_ok {
        return;
    }

    let io = state.io_base;

    // Get buffer pointers from state
    let bufs: [*mut u8; 4] = [
        core::ptr::addr_of!(state.tx_buf0.0) as *mut u8,
        core::ptr::addr_of!(state.tx_buf1.0) as *mut u8,
        core::ptr::addr_of!(state.tx_buf2.0) as *mut u8,
        core::ptr::addr_of!(state.tx_buf3.0) as *mut u8,
    ];

    for frame in NetworkContract::drain_tx() {
        let slot = state.tx_slot as usize;
        // Wait for previous TX on this slot to complete
        let mut spins = 0u32;
        while unsafe { pio_r32(io + TSD0 + slot as u16 * 4) } & TSD_TOK == 0 && spins < 100_000 {
            spins += 1;
            crate::arch::nop();
        }
        let copy = frame.len().min(1536);
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), bufs[slot], copy);
        }
        fence(Ordering::Release);
        // Write length to TSD to kick transmission
        unsafe {
            pio_w32(io + TSD0 + slot as u16 * 4, copy as u32 & TSD_SIZE_MASK);
        }
        let new_tx_slot = (state.tx_slot + 1) % 4;

        // Update state - do not drop the lock, just update the field
        state.tx_slot = new_tx_slot;
    }
}

pub fn present() -> bool {
    let state = DRIVER_STATE.lock();
    state.rtl_ok
}

// -- I/O port helpers ------------------------------------------------------

unsafe fn pio_r8(p: u16) -> u8 {
    unsafe { crate::arch::port_read_u8(p) }
}
unsafe fn pio_w8(p: u16, v: u8) {
    unsafe {
        crate::arch::port_write_u8(p, v);
    }
}
unsafe fn pio_w16(p: u16, v: u16) {
    unsafe {
        crate::arch::port_write_u16(p, v);
    }
}
unsafe fn pio_r32(p: u16) -> u32 {
    unsafe { crate::arch::port_read_u32(p) }
}
unsafe fn pio_w32(p: u16, v: u32) {
    unsafe {
        crate::arch::port_write_u32(p, v);
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
