//! VirtIO-Net NIC driver (legacy VirtIO 0.9.5 / MMIO-via-I/O-port).
//!
//! Implements the full virtqueue descriptor ring protocol:
//!   - PCI enumeration → BAR0 I/O base
//!   - VirtIO device negotiation (status, features)
//!   - RX queue (queue 0): pre-filled with guest buffers for device to write into
//!   - TX queue (queue 1): driver posts frames, device consumes them
//!   - poll_rx(): harvests completed RX descriptors → RX_QUEUE
//!   - flush_tx(): posts frames from TX_QUEUE → TX virtqueue → kicks device
//!
//! Identity mapping (physical == virtual) is assumed — valid for our 1:1 page tables.

use crate::network_contract::NetworkContract;
use core::sync::atomic::{Ordering, fence};

// -- PCI constants ----------------------------------------------------------

const PCI_CFG_ADDR: u16 = 0xCF8;
const PCI_CFG_DATA: u16 = 0xCFC;
const VIRTIO_VENDOR: u16 = 0x1AF4;
const VIRTIO_NET_DEV: u16 = 0x1000;

// -- VirtIO legacy I/O register offsets ------------------------------------

const REG_DEVICE_FEAT: u16 = 0x00; // device feature bits (read)
const REG_DRIVER_FEAT: u16 = 0x04; // driver feature bits (write)
const REG_QUEUE_PFN: u16 = 0x08; // virtqueue page frame number (write)
const REG_QUEUE_SIZE: u16 = 0x0C; // virtqueue size (read)
const REG_QUEUE_SEL: u16 = 0x0E; // virtqueue selector (write)
const REG_QUEUE_NOTIFY: u16 = 0x10; // virtqueue notify / kick (write)
const REG_DEV_STATUS: u16 = 0x12; // device status byte (read/write)
const REG_ISR: u16 = 0x13; // ISR status (read, clears on read)
const REG_MAC_BASE: u16 = 0x14; // device-specific: 6-byte MAC address

// VirtIO device status bits
const S_ACKNOWLEDGE: u8 = 0x01;
const S_DRIVER: u8 = 0x02;
const S_DRIVER_OK: u8 = 0x04;

// VirtIO-Net feature bits
const VIRTIO_NET_F_MAC: u32 = 1 << 5;

// Virtqueue descriptor flags
const VRING_DESC_F_NEXT: u16 = 0x01; // descriptor chains to .next
const VRING_DESC_F_WRITE: u16 = 0x02; // device writes (RX buffers)

// -- Virtqueue geometry -----------------------------------------------------

/// Number of descriptors per queue. Must be a power of two; QEMU supports up to 256.
const QUEUE_SIZE: usize = 64;

/// Size of each pre-allocated RX packet buffer (1 virtio_net_hdr + 1514 bytes ETH).
const RX_BUF_SIZE: usize = 10 + 1514; // virtio_net_hdr(10) + max ethernet frame

// -- Virtqueue data structures (VirtIO spec §2.4) --------------------------

/// One descriptor: points to a guest-memory buffer.
#[derive(Copy, Clone)]
#[repr(C)]
struct Desc {
    addr: u64, // guest physical address
    len: u32,
    flags: u16,
    next: u16,
}

/// Available ring: driver → device ("here are buffers for you to use").
#[repr(C)]
struct AvailRing {
    flags: u16,
    idx: u16,
    ring: [u16; QUEUE_SIZE],
    // used_event omitted (we don't suppress interrupts yet)
}

/// One entry in the used ring.
#[derive(Copy, Clone)]
#[repr(C)]
struct UsedElem {
    id: u32,  // descriptor index
    len: u32, // bytes written by device
}

/// Used ring: device → driver ("I'm done with these buffers").
#[repr(C)]
struct UsedRing {
    flags: u16,
    idx: u16,
    ring: [UsedElem; QUEUE_SIZE],
}

// -- Virtqueue memory layout ------------------------------------------------
//
// The legacy VirtIO spec requires the virtqueue to be a contiguous,
// page-aligned (4 KiB) region laid out as:
//
//   [ Descriptor table  : 16 * QUEUE_SIZE bytes           ]
//   [ Available ring    : 6  + 2*QUEUE_SIZE bytes         ]
//   [ padding to 4 KiB                                    ]
//   [ Used ring         : 6  + 8*QUEUE_SIZE bytes         ]
//
// We encode this as a single #[repr(C, align(4096))] struct.

const DESC_TABLE_BYTES: usize = 16 * QUEUE_SIZE;
const AVAIL_RING_BYTES: usize = 6 + 2 * QUEUE_SIZE;
const _PAD: usize = 4096 - (DESC_TABLE_BYTES + AVAIL_RING_BYTES) % 4096;
const USED_RING_OFFSET: usize = DESC_TABLE_BYTES + AVAIL_RING_BYTES + _PAD;
const VQUEUE_TOTAL: usize = USED_RING_OFFSET + 6 + 8 * QUEUE_SIZE;

#[repr(C, align(4096))]
struct Virtqueue {
    descs: [Desc; QUEUE_SIZE],
    avail: AvailRing,
    _pad: [u8; _PAD],
    used: UsedRing,
}

impl Virtqueue {
    const fn zeroed() -> Self {
        // SAFETY: all-zero is valid for these POD types.
        unsafe { core::mem::zeroed() }
    }
}

// -- RX packet buffers (device writes received frames here) ----------------

#[repr(C, align(4096))]
struct RxBuffers([[u8; RX_BUF_SIZE]; QUEUE_SIZE]);

// -- Static state (allocated in BSS, not the heap) -------------------------

static mut RX_QUEUE_MEM: Virtqueue = Virtqueue::zeroed();
static mut TX_QUEUE_MEM: Virtqueue = Virtqueue::zeroed();
static mut RX_BUFS: RxBuffers = RxBuffers([[0u8; RX_BUF_SIZE]; QUEUE_SIZE]);

static mut IO_BASE: u16 = 0;
static mut NIC_PRESENT: bool = false;

/// Index into the used ring last checked during poll_rx.
static mut RX_LAST_SEEN: u16 = 0;
/// Index into the used ring last checked during flush (for TX completion cleanup).
static mut TX_LAST_SEEN: u16 = 0;
/// Next TX descriptor index to use.
static mut TX_DESC_IDX: u16 = 0;

// -- Public API -------------------------------------------------------------

pub fn init() {
    match pci_find_virtio_net() {
        Some(io_base) => {
            unsafe {
                IO_BASE = io_base;
            }
            negotiate(io_base);
            setup_rx_queue(io_base);
            setup_tx_queue(io_base);
            finalize(io_base);

            let mac = read_mac(io_base);
            NetworkContract::set_identity(mac, NetworkContract::default_ip(), "virtio-net");

            let ip = NetworkContract::ip();
            crate::println!(
                "[net] VirtIO-Net ready  MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}  \
                 IP {}.{}.{}.{}",
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
            unsafe {
                NIC_PRESENT = true;
            }
        }
        None => {
            crate::serial_println!(
                "[net] No VirtIO-Net NIC found (run QEMU with -device virtio-net-pci)"
            );
        }
    }
}

/// Poll the RX used ring; push any completed frames into the global RX_QUEUE.
pub fn poll_rx() {
    if !unsafe { NIC_PRESENT } {
        return;
    }

    unsafe {
        let used_idx = core::ptr::read_volatile(&raw const RX_QUEUE_MEM.used.idx);
        while RX_LAST_SEEN != used_idx {
            let slot = (RX_LAST_SEEN as usize) % QUEUE_SIZE;
            let elem = core::ptr::read_volatile(&RX_QUEUE_MEM.used.ring[slot]);
            let desc_idx = elem.id as usize;
            let written = elem.len as usize;

            // The descriptor points into RX_BUFS; copy the ethernet frame out
            // (skip the 10-byte virtio_net_hdr at the start of the buffer)
            if written > 10 {
                let buf = &RX_BUFS.0[desc_idx % QUEUE_SIZE];
                let frame = &buf[10..written];
                NetworkContract::enqueue_rx(alloc::vec::Vec::from(frame), "virtio-net");
            }

            // Return descriptor to the available ring for re-use
            recycle_rx_desc(desc_idx as u16);

            RX_LAST_SEEN = RX_LAST_SEEN.wrapping_add(1);
        }
    }
}

/// Drain the global TX_QUEUE: build descriptors and kick the NIC.
pub fn flush_tx() {
    if !unsafe { NIC_PRESENT } {
        return;
    }

    // Reclaim completed TX descriptors first
    reclaim_tx_descs();

    let frames = NetworkContract::drain_tx();

    for frame in frames {
        transmit_frame(&frame);
    }
}

// -- Initialisation helpers -------------------------------------------------

/// Step 1: acknowledge device, claim driver role, negotiate features.
fn negotiate(io: u16) {
    unsafe {
        // Reset
        pio_write8(io + REG_DEV_STATUS, 0);
        // Acknowledge
        pio_write8(io + REG_DEV_STATUS, S_ACKNOWLEDGE);
        // We have a driver
        pio_write8(io + REG_DEV_STATUS, S_ACKNOWLEDGE | S_DRIVER);
        // Accept MAC feature only; ignore everything else
        let dev_feat = pio_read32(io + REG_DEVICE_FEAT);
        pio_write32(io + REG_DRIVER_FEAT, dev_feat & VIRTIO_NET_F_MAC);
    }
}

/// Step 2: set up the RX virtqueue (queue 0).
fn setup_rx_queue(io: u16) {
    unsafe {
        // Select queue 0 (RX)
        pio_write16(io + REG_QUEUE_SEL, 0);
        let qsize = pio_read16(io + REG_QUEUE_SIZE) as usize;
        let qsize = qsize.min(QUEUE_SIZE);

        // Tell the device where the virtqueue lives (page frame number = addr >> 12)
        let pfn = (&raw const RX_QUEUE_MEM as *const _ as u32) >> 12;
        pio_write32(io + REG_QUEUE_PFN, pfn);

        // Pre-fill all descriptors with guest RX buffers and add them to the avail ring
        for i in 0..qsize {
            let buf_phys = RX_BUFS.0[i].as_ptr() as u64;
            RX_QUEUE_MEM.descs[i] = Desc {
                addr: buf_phys,
                len: RX_BUF_SIZE as u32,
                flags: VRING_DESC_F_WRITE, // device writes into these
                next: 0,
            };
            RX_QUEUE_MEM.avail.ring[i] = i as u16;
        }
        fence(Ordering::Release);
        RX_QUEUE_MEM.avail.idx = qsize as u16;
        fence(Ordering::Release);

        // Kick queue 0 so the device picks up the fresh buffers
        pio_write16(io + REG_QUEUE_NOTIFY, 0);
    }
}

/// Step 3: set up the TX virtqueue (queue 1).
fn setup_tx_queue(io: u16) {
    unsafe {
        pio_write16(io + REG_QUEUE_SEL, 1);
        let pfn = (&raw const TX_QUEUE_MEM as *const _ as u32) >> 12;
        pio_write32(io + REG_QUEUE_PFN, pfn);
        // TX descriptors are filled on demand; available ring starts empty.
    }
}

/// Step 4: set DRIVER_OK — device starts processing.
fn finalize(io: u16) {
    unsafe {
        pio_write8(io + REG_DEV_STATUS, S_ACKNOWLEDGE | S_DRIVER | S_DRIVER_OK);
    }
}

// -- TX path ---------------------------------------------------------------

/// A small static pool of TX frame buffers (avoids heap allocation on the hot path).
const TX_BUF_SIZE: usize = 10 + 1514;
const TX_BUF_COUNT: usize = QUEUE_SIZE;

#[repr(C, align(64))]
struct TxBuffers([[u8; TX_BUF_SIZE]; TX_BUF_COUNT]);
static mut TX_BUFS: TxBuffers = TxBuffers([[0u8; TX_BUF_SIZE]; TX_BUF_COUNT]);

fn transmit_frame(frame: &[u8]) {
    unsafe {
        let desc_idx = (TX_DESC_IDX as usize) % TX_BUF_COUNT;

        // Copy: virtio_net_hdr (10 zero bytes) + ethernet frame
        let buf = &mut TX_BUFS.0[desc_idx];
        buf[..10].fill(0); // virtio_net_hdr: no offloads, no GSO
        let copy_len = frame.len().min(TX_BUF_SIZE - 10);
        buf[10..10 + copy_len].copy_from_slice(&frame[..copy_len]);

        TX_QUEUE_MEM.descs[desc_idx] = Desc {
            addr: buf.as_ptr() as u64,
            len: (10 + copy_len) as u32,
            flags: 0, // host reads this; no WRITE flag, no chaining
            next: 0,
        };

        // Add to the available ring
        let avail_slot = (TX_QUEUE_MEM.avail.idx as usize) % QUEUE_SIZE;
        TX_QUEUE_MEM.avail.ring[avail_slot] = desc_idx as u16;

        fence(Ordering::Release); // ensure descriptor is visible before idx bump
        TX_QUEUE_MEM.avail.idx = TX_QUEUE_MEM.avail.idx.wrapping_add(1);
        fence(Ordering::Release); // ensure idx is visible before notify

        // Kick queue 1 (TX)
        pio_write16(IO_BASE + REG_QUEUE_NOTIFY, 1);

        TX_DESC_IDX = TX_DESC_IDX.wrapping_add(1);
    }
}

/// Recycle TX descriptors the device has finished with.
fn reclaim_tx_descs() {
    unsafe {
        let used_idx = core::ptr::read_volatile(&raw const TX_QUEUE_MEM.used.idx);
        while TX_LAST_SEEN != used_idx {
            // Nothing to free — TX_BUFS is static, we just advance the pointer.
            TX_LAST_SEEN = TX_LAST_SEEN.wrapping_add(1);
        }
    }
}

// -- RX recycling ----------------------------------------------------------

/// Return a used RX descriptor back to the available ring.
fn recycle_rx_desc(desc_idx: u16) {
    unsafe {
        let avail_slot = (RX_QUEUE_MEM.avail.idx as usize) % QUEUE_SIZE;
        RX_QUEUE_MEM.avail.ring[avail_slot] = desc_idx;
        fence(Ordering::Release);
        RX_QUEUE_MEM.avail.idx = RX_QUEUE_MEM.avail.idx.wrapping_add(1);
        fence(Ordering::Release);
        pio_write16(IO_BASE + REG_QUEUE_NOTIFY, 0); // kick RX queue
    }
}

// -- PCI enumeration --------------------------------------------------------

fn pci_find_virtio_net() -> Option<u16> {
    for bus in 0u8..=3 {
        for dev in 0u8..32 {
            let id = pci_read(bus, dev, 0, 0x00);
            if id == 0xFFFF_FFFF {
                continue;
            }
            let vendor = (id & 0xFFFF) as u16;
            let device = (id >> 16) as u16;
            if vendor == VIRTIO_VENDOR && device == VIRTIO_NET_DEV {
                // Enable bus-mastering + I/O space in PCI command register
                let cmd = pci_read(bus, dev, 0, 0x04);
                pci_write(bus, dev, 0, 0x04, cmd | 0x0005);

                let bar0 = pci_read(bus, dev, 0, 0x10);
                if bar0 & 1 == 1 {
                    // I/O BAR: strip indicator bits
                    return Some((bar0 & 0xFFFC) as u16);
                }
            }
        }
    }
    None
}

fn pci_read(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = pci_addr(bus, dev, func, offset);
    unsafe {
        crate::arch::port_write_u32(PCI_CFG_ADDR, addr);
        crate::arch::port_read_u32(PCI_CFG_DATA)
    }
}

fn pci_write(bus: u8, dev: u8, func: u8, offset: u8, val: u32) {
    let addr = pci_addr(bus, dev, func, offset);
    unsafe {
        crate::arch::port_write_u32(PCI_CFG_ADDR, addr);
        crate::arch::port_write_u32(PCI_CFG_DATA, val);
    }
}

fn pci_addr(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC)
}

// -- MAC address ------------------------------------------------------------

fn read_mac(io_base: u16) -> [u8; 6] {
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = unsafe { pio_read8(io_base + REG_MAC_BASE + i as u16) };
    }
    mac
}

// -- I/O port helpers -------------------------------------------------------

unsafe fn pio_read8(port: u16) -> u8 {
    unsafe { crate::arch::port_read_u8(port) }
}
unsafe fn pio_read16(port: u16) -> u16 {
    unsafe { crate::arch::port_read_u16(port) }
}
unsafe fn pio_read32(port: u16) -> u32 {
    unsafe { crate::arch::port_read_u32(port) }
}

unsafe fn pio_write8(port: u16, v: u8) {
    unsafe { crate::arch::port_write_u8(port, v) }
}
unsafe fn pio_write16(port: u16, v: u16) {
    unsafe { crate::arch::port_write_u16(port, v) }
}
unsafe fn pio_write32(port: u16, v: u32) {
    unsafe { crate::arch::port_write_u32(port, v) }
}
