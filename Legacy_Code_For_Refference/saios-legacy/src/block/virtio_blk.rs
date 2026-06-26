//! VirtIO-Block driver (legacy VirtIO 0.9 / PCI).
//! Implements synchronous read/write via the virtqueue request protocol.

use super::{BlockDevice, BlockDeviceInfo, StorageController};
use alloc::sync::Arc;
use core::sync::atomic::{Ordering, fence};

const VIRTIO_VENDOR: u16 = 0x1AF4;
const VIRTIO_BLK_DEV: u16 = 0x1001;

const REG_DEVICE_FEAT: u16 = 0x00;
const REG_DRIVER_FEAT: u16 = 0x04;
const REG_QUEUE_PFN: u16 = 0x08;
const REG_QUEUE_SIZE: u16 = 0x0C;
const REG_QUEUE_SEL: u16 = 0x0E;
const REG_QUEUE_NOTIFY: u16 = 0x10;
const REG_DEV_STATUS: u16 = 0x12;
const REG_SECTOR_COUNT: u16 = 0x14; // device-specific: 8 bytes

const PCI_CFG_ADDR: u16 = 0xCF8;
const PCI_CFG_DATA: u16 = 0xCFC;

const QUEUE_SIZE: usize = 16;

// VirtIO block request types
const VIRTIO_BLK_T_IN: u32 = 0; // read
const VIRTIO_BLK_T_OUT: u32 = 1; // write

#[repr(C, align(4096))]
struct Virtqueue {
    descs: [Desc; QUEUE_SIZE],
    avail: AvailRing,
    _pad:
        [u8; 4096 - core::mem::size_of::<[Desc; QUEUE_SIZE]>() - core::mem::size_of::<AvailRing>()],
    used: UsedRing,
}

#[derive(Copy, Clone)]
#[repr(C)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct AvailRing {
    flags: u16,
    idx: u16,
    ring: [u16; QUEUE_SIZE],
}

#[derive(Copy, Clone)]
#[repr(C)]
struct UsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct UsedRing {
    flags: u16,
    idx: u16,
    ring: [UsedElem; QUEUE_SIZE],
}

#[repr(C)]
struct BlkReqHeader {
    type_: u32,
    _reserved: u32,
    sector: u64,
}

pub struct VirtioBlk {
    io_base: u16,
    sector_count: u64,
}

static mut VQ: Virtqueue = unsafe { core::mem::zeroed() };

// Header, status byte for the one in-flight request
static mut BLK_HDR: BlkReqHeader = BlkReqHeader {
    type_: 0,
    _reserved: 0,
    sector: 0,
};
static mut BLK_STATUS: u8 = 0xFF;
// Data buffer (max 32 sectors = 16 KiB)
static mut BLK_DATA: [u8; 512 * 32] = [0u8; 512 * 32];

impl VirtioBlk {
    pub fn probe() -> Option<Arc<dyn BlockDevice>> {
        for bus in 0u8..=3 {
            for dev in 0u8..32 {
                let id = pci_read(bus, dev, 0, 0);
                if id == 0xFFFF_FFFF {
                    continue;
                }
                let vendor = (id & 0xFFFF) as u16;
                let device = (id >> 16) as u16;
                if vendor == VIRTIO_VENDOR && device == VIRTIO_BLK_DEV {
                    let bar0 = pci_read(bus, dev, 0, 0x10);
                    if bar0 & 1 == 1 {
                        let io_base = (bar0 & 0xFFFC) as u16;
                        // Enable bus-mastering
                        let cmd = pci_read(bus, dev, 0, 0x04);
                        pci_write(bus, dev, 0, 0x04, cmd | 0x05);

                        let drv = Self::init(io_base);
                        return Some(Arc::new(drv));
                    }
                }
            }
        }
        None
    }

    fn init(io: u16) -> Self {
        unsafe {
            pio_w8(io + REG_DEV_STATUS, 0); // reset
            pio_w8(io + REG_DEV_STATUS, 0x01); // ACK
            pio_w8(io + REG_DEV_STATUS, 0x03); // ACK | DRIVER
            pio_w32(io + REG_DRIVER_FEAT, pio_r32(io + REG_DEVICE_FEAT));

            // Read sector count (device-specific config at +0x14)
            let sc_lo = pio_r32(io + REG_SECTOR_COUNT) as u64;
            let sc_hi = pio_r32(io + REG_SECTOR_COUNT + 4) as u64;
            let sector_count = sc_lo | (sc_hi << 32);

            // Set up queue 0
            pio_w16(io + REG_QUEUE_SEL, 0);
            let pfn = (&raw const VQ as *const _ as u32) >> 12;
            pio_w32(io + REG_QUEUE_PFN, pfn);

            pio_w8(io + REG_DEV_STATUS, 0x0F); // DRIVER_OK

            Self {
                io_base: io,
                sector_count,
            }
        }
    }

    fn do_request(&self, read: bool, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let sectors = buf.len() / 512;
        if sectors > 32 {
            return Err("virtio-blk: request too large");
        }

        unsafe {
            BLK_HDR.type_ = if read {
                VIRTIO_BLK_T_IN
            } else {
                VIRTIO_BLK_T_OUT
            };
            BLK_HDR._reserved = 0;
            BLK_HDR.sector = lba;
            BLK_STATUS = 0xFF;

            if !read {
                BLK_DATA[..buf.len()].copy_from_slice(buf);
            }

            // Descriptor 0: header (read by device)
            VQ.descs[0] = Desc {
                addr: &raw const BLK_HDR as u64,
                len: core::mem::size_of::<BlkReqHeader>() as u32,
                flags: 0x01, // NEXT
                next: 1,
            };
            // Descriptor 1: data (WRITE flag if reading, so device writes into it)
            VQ.descs[1] = Desc {
                addr: &raw const BLK_DATA as u64,
                len: buf.len() as u32,
                flags: if read { 0x03 } else { 0x01 }, // WRITE|NEXT or just NEXT
                next: 2,
            };
            // Descriptor 2: status byte (device writes here)
            VQ.descs[2] = Desc {
                addr: &raw const BLK_STATUS as u64,
                len: 1,
                flags: 0x02, // WRITE
                next: 0,
            };

            let avail_slot = (VQ.avail.idx as usize) % QUEUE_SIZE;
            VQ.avail.ring[avail_slot] = 0; // head descriptor index
            fence(Ordering::Release);
            VQ.avail.idx = VQ.avail.idx.wrapping_add(1);
            fence(Ordering::Release);

            // Kick device
            pio_w16(self.io_base + REG_QUEUE_NOTIFY, 0);

            // Busy-wait for completion (polling, no interrupts)
            let mut spins = 0u64;
            loop {
                fence(Ordering::Acquire);
                let used_idx = core::ptr::read_volatile(&raw const VQ.used.idx);
                if used_idx != VQ.avail.idx.wrapping_sub(1) {
                    // Not yet — check again
                    if spins > 10_000_000 {
                        return Err("virtio-blk: timeout");
                    }
                    spins += 1;
                    crate::arch::nop();
                    continue;
                }
                break;
            }

            if BLK_STATUS != 0 {
                return Err("virtio-blk: device returned error");
            }
            if read {
                buf.copy_from_slice(&BLK_DATA[..buf.len()]);
            }
        }
        Ok(())
    }
}

impl BlockDevice for VirtioBlk {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64 {
        self.sector_count
    }

    fn device_info(&self) -> BlockDeviceInfo {
        BlockDeviceInfo {
            controller: StorageController::VirtioBlk,
            port: None,
            sector_count: self.sector_count,
            sector_size: 512,
        }
    }

    fn read_sectors(&self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        // Handle multi-sector reads in 32-sector chunks
        let mut done = 0usize;
        let total = buf.len();
        while done < total {
            let chunk = (total - done).min(512 * 32);
            // round up to sector boundary
            let chunk = chunk.div_ceil(512) * 512;
            let chunk = chunk.min(total - done);
            let chunk = chunk.div_ceil(512) * 512;
            let sectors = chunk / 512;
            let lba_off = (done / 512) as u64;
            let mut tmp = alloc::vec![0u8; sectors * 512];
            self.do_request(true, lba + lba_off, &mut tmp)?;
            let copy = (total - done).min(sectors * 512);
            buf[done..done + copy].copy_from_slice(&tmp[..copy]);
            done += copy;
        }
        Ok(())
    }

    fn write_sectors(&self, lba: u64, buf: &[u8]) -> Result<(), &'static str> {
        let mut tmp = alloc::vec![0u8; buf.len()];
        tmp.copy_from_slice(buf);
        self.do_request(false, lba, &mut tmp)
    }
}

// -- PCI helpers ------------------------------------------------------------
fn pci_read(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        crate::arch::port_write_u32(PCI_CFG_ADDR, addr);
        crate::arch::port_read_u32(PCI_CFG_DATA)
    }
}
fn pci_write(bus: u8, dev: u8, func: u8, offset: u8, val: u32) {
    let addr = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);
    unsafe {
        crate::arch::port_write_u32(PCI_CFG_ADDR, addr);
        crate::arch::port_write_u32(PCI_CFG_DATA, val);
    }
}
unsafe fn pio_r32(p: u16) -> u32 {
    unsafe { crate::arch::port_read_u32(p) }
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
unsafe fn pio_w32(p: u16, v: u32) {
    unsafe {
        crate::arch::port_write_u32(p, v);
    }
}
