//! AHCI (Advanced Host Controller Interface) SATA driver.
//!
//! Supports any AHCI 1.x compliant controller:
//!   Intel ICH9   PCI 8086:2922  - VirtualBox default SATA controller
//!   Intel ICH10  PCI 8086:3A22
//!   Intel 6-Series PCI 8086:1C02
//!   Any PCI class=0x01 subclass=0x06 progif=0x01
//!
//! # How AHCI works
//!
//! The AHCI controller exposes an MMIO region (BAR5) called the HBA Memory
//! Space. It contains global registers plus one "port register set" per
//! implemented port (max 32).
//!
//! For each port we set up two DMA buffers in physical memory:
//!   - Command List  - 32 x 32-byte command headers
//!   - FIS Buffer    - 256 bytes of received FIS data
//!
//! Each command header points to a Command Table that contains:
//!   - A Register H2D FIS (20 bytes) - the ATA command
//!   - A Physical Region Descriptor Table (PRDT) - buffer descriptors
//!
//! We use a single command slot and poll for completion (no interrupts).

use super::{BlockDevice, BlockDeviceInfo, StorageController};
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering, fence};
use spin::Mutex;

// -- PCI detection ----------------------------------------------------------

/// PCI class / subclass / progif that identifies AHCI controllers.
const AHCI_CLASS: u8 = 0x01; // Mass Storage
const AHCI_SUBCLASS: u8 = 0x06; // Serial ATA
const AHCI_PROGIF: u8 = 0x01; // AHCI 1.0

// -- HBA global register offsets (from BAR5 base) --------------------------

const HBA_CAP: u32 = 0x00; // Host Capabilities
const HBA_GHC: u32 = 0x04; // Global Host Control  (bit 31 = AHCI enable, bit 0 = HBA reset)
const HBA_IS: u32 = 0x08; // Interrupt Status (write-1-to-clear)
const HBA_PI: u32 = 0x0C; // Ports Implemented bitmask
const HBA_VS: u32 = 0x10; // AHCI Version

// -- Per-port register offsets (from port base = HBA_BASE + 0x100 + port*0x80) --

const PORT_CLB: u32 = 0x00; // Command List Base Address (low 32 bits)
const PORT_CLBU: u32 = 0x04; // Command List Base Address (high 32 bits)
const PORT_FB: u32 = 0x08; // FIS Base Address (low)
const PORT_FBU: u32 = 0x0C; // FIS Base Address (high)
const PORT_IS: u32 = 0x10; // Interrupt Status (write-1-to-clear)
const PORT_IE: u32 = 0x14; // Interrupt Enable
const PORT_CMD: u32 = 0x18; // Command and Status
const PORT_TFD: u32 = 0x20; // Task File Data
const PORT_SIG: u32 = 0x24; // Signature
const PORT_SSTS: u32 = 0x28; // Serial ATA Status (DET + IPM fields)
const PORT_SACT: u32 = 0x34; // SATA Active (NCQ tags outstanding)
const PORT_SERR: u32 = 0x30; // Serial ATA Error (write-1-to-clear)
const PORT_CI: u32 = 0x38; // Command Issue

// PORT_CMD bits
const CMD_ST: u32 = 1 << 0; // Start - process commands from the list
const CMD_FRE: u32 = 1 << 4; // FIS Receive Enable
const CMD_FR: u32 = 1 << 14; // FIS Receive Running
const CMD_CR: u32 = 1 << 15; // Command List Running

// PORT_SSTS (Serial ATA Status) fields
const SSTS_DET_PRESENT: u32 = 0x3; // Device detected + communication established
const SSTS_IPM_ACTIVE: u32 = 0x1; // Interface in active state

// PORT_SIG values - identifies device type
const SIG_ATA: u32 = 0x0000_0101; // SATA disk
const SIG_ATAPI: u32 = 0xEB14_0101; // SATAPI (optical)

// ATA commands
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;
const ATA_IDENTIFY: u8 = 0xEC;
const ATA_FLUSH_CACHE_EXT: u8 = 0xEA;

// FIS types
const FIS_TYPE_REG_H2D: u8 = 0x27; // Register FIS - Host to Device

// -- DMA buffer sizes -------------------------------------------------------

const CMD_LIST_SIZE: usize = 32 * 32; // 32 command headers x 32 bytes = 1 KiB
const FIS_BUF_SIZE: usize = 256; // Received FIS buffer
const CMD_TABLE_SIZE: usize = 128 + 16 * 16; // FIS + PRDT entries (16 entries max)
const DATA_BUF_SIZE: usize = 512 * 128; // 128 sectors = 64 KiB max per transfer

// -- Driver state structure (wrapped in Mutex) ------------------------------

#[repr(C, align(1024))]
struct CmdList([u8; CMD_LIST_SIZE]);

#[repr(C, align(256))]
struct FisBuf([u8; FIS_BUF_SIZE]);

#[repr(C, align(128))]
#[derive(Clone, Copy)]
struct CmdTable([u8; CMD_TABLE_SIZE]);

#[repr(C, align(4096))]
struct DataBuf([u8; DATA_BUF_SIZE]);

struct DriverState {
    cmd_list: CmdList,
    fis_buf: FisBuf,
    cmd_table: CmdTable,
    data_buf: DataBuf,
}

// -- Global driver state ----------------------------------------------------

static DRIVER_STATE: Mutex<DriverState> = Mutex::new(DriverState {
    cmd_list: CmdList([0u8; CMD_LIST_SIZE]),
    fis_buf: FisBuf([0u8; FIS_BUF_SIZE]),
    cmd_table: CmdTable([0u8; CMD_TABLE_SIZE]),
    data_buf: DataBuf([0u8; DATA_BUF_SIZE]),
});
static IO_LOCK: Mutex<()> = Mutex::new(());

// -- Driver state -----------------------------------------------------------

pub struct Ahci {
    /// MMIO base address of the HBA (BAR5).
    hba_base: u64,
    /// Physical port number (0-31) of the first detected disk.
    port: u32,
    /// Total number of 512-byte sectors on the disk.
    sector_count: u64,
}

// -- Public interface -------------------------------------------------------

impl Ahci {
    /// Probe all PCI buses for an AHCI controller, initialise the first
    /// attached disk, and return a driver instance or `None`.
    pub fn probe() -> Option<Arc<dyn BlockDevice>> {
        let (bus, dev, func, bar5) = find_ahci_pci()?;

        // Enable PCI bus mastering + MMIO (command register bits 2 and 1)
        let cmd = pci_read(bus, dev, func, 0x04);
        pci_write(bus, dev, func, 0x04, cmd | 0x06);

        let hba_base = (bar5 & !0xF) as u64;

        crate::println!(
            "[ahci] controller at PCI {:02x}:{:02x}.{} MMIO={:#x}",
            bus,
            dev,
            func,
            hba_base
        );

        // Enable AHCI mode in GHC.AE (bit 31)
        let ghc = unsafe { mmio_r32(hba_base, HBA_GHC) };
        unsafe { mmio_w32(hba_base, HBA_GHC, ghc | (1 << 31)) };

        // Find the first port with a connected SATA disk
        let pi = unsafe { mmio_r32(hba_base, HBA_PI) };
        for port in 0..32u32 {
            if pi & (1 << port) == 0 {
                continue;
            }
            if let Some(sectors) = init_port(hba_base, port) {
                crate::println!(
                    "[ahci] port {} - {} MiB ({} sectors)",
                    port,
                    sectors * 512 / (1024 * 1024),
                    sectors
                );
                return Some(Arc::new(Ahci {
                    hba_base,
                    port,
                    sector_count: sectors,
                }));
            }
        }

        crate::println!("[ahci] no disk found on any port");
        None
    }
}

impl BlockDevice for Ahci {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64 {
        self.sector_count
    }

    fn device_info(&self) -> BlockDeviceInfo {
        BlockDeviceInfo {
            controller: StorageController::Ahci,
            port: Some(self.port),
            sector_count: self.sector_count,
            sector_size: 512,
        }
    }

    fn read_sectors(&self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let _io = IO_LOCK.lock();
        let count = buf.len() / 512;
        if count == 0 || count > 128 {
            return Err("ahci: bad read size");
        }
        for i in 0..count {
            unsafe {
                issue_command(
                    self.hba_base,
                    self.port,
                    lba + i as u64,
                    1,
                    AtaOp::Read,
                    i * 512,
                )?;
            }
            let state = DRIVER_STATE.lock();
            buf[i * 512..(i + 1) * 512].copy_from_slice(&state.data_buf.0[i * 512..(i + 1) * 512]);
        }
        Ok(())
    }

    fn write_sectors(&self, lba: u64, buf: &[u8]) -> Result<(), &'static str> {
        let _io = IO_LOCK.lock();
        let count = buf.len() / 512;
        if count == 0 || count > 128 {
            return Err("ahci: bad write size");
        }

        // Write each sector
        for i in 0..count {
            // Copy data to state buffer first
            {
                let mut state = DRIVER_STATE.lock();
                state.data_buf.0[..512].copy_from_slice(&buf[i * 512..(i + 1) * 512]);
            }
            unsafe {
                issue_command(self.hba_base, self.port, lba + i as u64, 1, AtaOp::Write, 0)?;
            }
        }

        // FLUSH once per call instead of per sector
        if !DEFER_FLUSH.load(Ordering::Relaxed) {
            unsafe {
                issue_command(self.hba_base, self.port, 0, 0, AtaOp::Flush, 0)?;
            }
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), &'static str> {
        let _io = IO_LOCK.lock();
        unsafe { issue_command(self.hba_base, self.port, 0, 0, AtaOp::Flush, 0) }
    }

    fn set_write_through(&self, on: bool) {
        DEFER_FLUSH.store(!on, Ordering::Relaxed);
    }
}

// -- Defers flush state -----------------------------------------------------

static DEFER_FLUSH: AtomicBool = AtomicBool::new(false);

// -- Port initialisation ----------------------------------------------------

/// Initialise one AHCI port and return its sector count, or `None` if no disk.
fn init_port(base: u64, port: u32) -> Option<u64> {
    let pb = port_base(base, port);

    // Check device presence via SSTS.DET and SSTS.IPM
    let ssts = unsafe { mmio_r32(base, pb + PORT_SSTS) };
    let det = ssts & 0x0F;
    let ipm = (ssts >> 8) & 0x0F;
    if det != SSTS_DET_PRESENT || ipm != SSTS_IPM_ACTIVE {
        return None; // no device
    }

    // Only handle plain SATA disks (not SATAPI / port multipliers)
    let sig = unsafe { mmio_r32(base, pb + PORT_SIG) };
    if sig != SIG_ATA {
        return None;
    }

    // Stop the port engine before touching DMA pointers
    stop_port(base, pb);

    // Lock state and set up DMA buffer pointers - physical addresses (identity-mapped)
    let state = DRIVER_STATE.lock();

    // Get addresses of inner arrays directly from state
    let cl_phys = core::ptr::addr_of!(state.cmd_list.0) as u64;
    let fb_phys = core::ptr::addr_of!(state.fis_buf.0) as u64;

    unsafe {
        mmio_w32(base, pb + PORT_CLB, cl_phys as u32);
    }
    unsafe {
        mmio_w32(base, pb + PORT_CLBU, (cl_phys >> 32) as u32);
    }
    unsafe {
        mmio_w32(base, pb + PORT_FB, fb_phys as u32);
    }
    unsafe {
        mmio_w32(base, pb + PORT_FBU, (fb_phys >> 32) as u32);
    }

    // Zero out the DMA areas using the inner array pointer
    let ct_ptr = core::ptr::addr_of!(state.cmd_table.0) as *mut u8;
    let cl_ptr = core::ptr::addr_of!(state.cmd_list.0) as *mut u8;
    let fb_ptr = core::ptr::addr_of!(state.fis_buf.0) as *mut u8;
    let db_ptr = core::ptr::addr_of!(state.data_buf.0) as *mut u8;

    unsafe {
        core::ptr::write_bytes(cl_ptr, 0, CMD_LIST_SIZE);
        core::ptr::write_bytes(fb_ptr, 0, FIS_BUF_SIZE);
        core::ptr::write_bytes(ct_ptr, 0, CMD_TABLE_SIZE);
        core::ptr::write_bytes(db_ptr, 0, DATA_BUF_SIZE);
    }

    // Clear interrupt and error status
    unsafe {
        mmio_w32(base, pb + PORT_IS, 0xFFFF_FFFF);
    }
    unsafe {
        mmio_w32(base, pb + PORT_SERR, 0xFFFF_FFFF);
    }

    drop(state);

    // Restart the port engine
    start_port(base, pb);

    // Issue ATA IDENTIFY to read disk capacity
    let sectors = identify_disk(base, port)?;
    Some(sectors)
}

/// Issue ATA IDENTIFY DEVICE and extract the sector count.
fn identify_disk(base: u64, port: u32) -> Option<u64> {
    let _io = IO_LOCK.lock();
    unsafe {
        issue_command(base, port, 0, 1, AtaOp::Identify, 0).ok()?;

        // Lock state and get data
        let state = DRIVER_STATE.lock();
        // IDENTIFY response is 512 bytes in DATA_BUF.
        // Words 60-61 = 28-bit LBA sector count (legacy)
        // Words 100-103 = 48-bit LBA sector count (extended)
        let data = &state.data_buf.0[..512];
        let w = |off: usize| u16::from_le_bytes([data[off * 2], data[off * 2 + 1]]) as u64;

        // Prefer 48-bit count (words 100-103)
        let lba48 = w(100) | (w(101) << 16) | (w(102) << 32) | (w(103) << 48);
        if lba48 > 0 {
            return Some(lba48);
        }

        // Fall back to 28-bit count (words 60-61)
        let lba28 = w(60) | (w(61) << 16);
        if lba28 > 0 {
            return Some(lba28);
        }

        None
    }
}

// -- Command issue ---------------------------------------------------------

/// Build and issue one ATA DMA command (read or write) using slot 0.
///
/// # Safety
/// Caller must hold no other reference to `DATA_BUF` or `CMD_TABLE`.
/// ATA operation to perform - chosen explicitly by the caller so a normal
/// read of LBA 0 is never mistaken for an IDENTIFY command.
#[derive(Clone, Copy, PartialEq)]
enum AtaOp {
    Read,
    Write,
    Identify,
    Flush,
}

unsafe fn issue_command(
    base: u64,
    port: u32,
    lba: u64,
    sectors: u16,
    op: AtaOp,
    buf_off: usize,
) -> Result<(), &'static str> {
    unsafe {
        let pb = port_base(base, port);
        let write = op == AtaOp::Write;

        // Lock state and get buffer pointers
        let state = DRIVER_STATE.lock();

        // Wait for the port to be idle (BSY/DRQ clear) BEFORE touching the shared
        // command table - a previous command may still be in flight reading it.
        let mut pspin = 0u64;
        while { mmio_r32(base, pb + PORT_TFD) } & 0x88 != 0 {
            // BSY (0x80) | DRQ (0x08)
            pspin += 1;
            if pspin > 50_000_000 {
                return Err("ahci: port busy before setup");
            }
            crate::arch::nop();
        }

        // Get pointer to command table
        let ct_ptr = core::ptr::addr_of!(state.cmd_table.0) as *mut u8;

        // -- Command Table: H2D Register FIS (bytes 0-19) ----------------------
        core::ptr::write_bytes(ct_ptr, 0, CMD_TABLE_SIZE);

        // FIS type and C bit (bit 7 of byte 1 = command register update)
        *ct_ptr.add(0) = FIS_TYPE_REG_H2D;
        *ct_ptr.add(1) = 0x80; // C = 1 (this is a command, not a control write)

        // ATA command byte - selected from the explicit op, never inferred.
        let cmd = match op {
            AtaOp::Write => ATA_WRITE_DMA_EXT,
            AtaOp::Read => ATA_READ_DMA_EXT,
            AtaOp::Identify => ATA_IDENTIFY,
            AtaOp::Flush => ATA_FLUSH_CACHE_EXT,
        };
        *ct_ptr.add(2) = cmd;

        // Device register: bit 6 = LBA mode
        *ct_ptr.add(7) = 0x40;

        // H2D Register FIS LBA layout (THE bug fix):
        //   byte 3  = Features[7:0]   (NOT LBA - must stay 0 for read/write)
        //   byte 4  = LBA[7:0]
        //   byte 5  = LBA[15:8]
        //   byte 6  = LBA[23:16]
        //   byte 8  = LBA[31:24]
        //   byte 9  = LBA[39:32]
        //   byte 10 = LBA[47:40]
        // The old code wrote LBA starting at byte 3, putting LBA[7:0] into Features
        // (ignored) and skipping byte 6 - so any two LBAs differing only in their
        // low 8 bits aliased to the same sector.
        *ct_ptr.add(3) = 0; // Features[7:0]
        *ct_ptr.add(4) = (lba & 0xFF) as u8; // LBA[7:0]
        *ct_ptr.add(5) = ((lba >> 8) & 0xFF) as u8; // LBA[15:8]
        *ct_ptr.add(6) = ((lba >> 16) & 0xFF) as u8; // LBA[23:16]
        *ct_ptr.add(8) = ((lba >> 24) & 0xFF) as u8; // LBA[31:24]
        *ct_ptr.add(9) = ((lba >> 32) & 0xFF) as u8; // LBA[39:32]
        *ct_ptr.add(10) = ((lba >> 40) & 0xFF) as u8; // LBA[47:40]

        // Sector count (bytes 12-13)
        *ct_ptr.add(12) = (sectors & 0xFF) as u8;
        *ct_ptr.add(13) = (sectors >> 8) as u8;

        // FLUSH CACHE takes no LBA/sector operands - keep the taskfile clean so the
        // controller never sees a malformed flush.
        if op == AtaOp::Flush {
            *ct_ptr.add(4) = 0;
            *ct_ptr.add(5) = 0;
            *ct_ptr.add(6) = 0;
            *ct_ptr.add(8) = 0;
            *ct_ptr.add(9) = 0;
            *ct_ptr.add(10) = 0;
            *ct_ptr.add(12) = 0;
            *ct_ptr.add(13) = 0;
        }

        // -- PRDT entry at offset 128 in command table -------------------------
        // FLUSH CACHE transfers no data, so it uses zero PRDT entries.
        let prdtl: u16 = if op == AtaOp::Flush {
            0
        } else {
            let prdt = ct_ptr.add(128);
            let db_ptr = core::ptr::addr_of!(state.data_buf.0) as *const u8;
            let data_phys = db_ptr as u64 + buf_off as u64;
            // PRDT entry: DBA (4), DBAU (4), reserved (4), DBC (4)
            // DBC = byte count - 1, interrupt on completion = bit 31
            let byte_count = (sectors as u32) * 512;
            core::ptr::write_unaligned(prdt.add(0) as *mut u32, data_phys as u32);
            core::ptr::write_unaligned(prdt.add(4) as *mut u32, (data_phys >> 32) as u32);
            core::ptr::write_unaligned(prdt.add(8) as *mut u32, 0);
            core::ptr::write_unaligned(prdt.add(12) as *mut u32, byte_count - 1); // DBC, IOC cleared (we poll)
            1
        };

        // -- Command Header (slot 0 in Command List) ---------------------------
        // Flags: FIS length (CFIS size in dwords = 5), write flag (bit 6)
        let cl_ptr = core::ptr::addr_of!(state.cmd_list.0) as *mut u8;
        let flags: u16 = 5 | if write { 1 << 6 } else { 0 };

        core::ptr::write_unaligned(cl_ptr.add(0) as *mut u16, flags); // DW0 low: flags (FIS len + W)
        core::ptr::write_unaligned(cl_ptr.add(2) as *mut u16, prdtl); // DW0 high: PRDTL
        core::ptr::write_unaligned(cl_ptr.add(4) as *mut u32, 0); // DW1: PRDBC (filled by HBA)

        let ct_ptr_phys = core::ptr::addr_of!(state.cmd_table.0) as u64;
        core::ptr::write_unaligned(cl_ptr.add(8) as *mut u32, ct_ptr_phys as u32); // DW2: CTBA
        core::ptr::write_unaligned(cl_ptr.add(12) as *mut u32, (ct_ptr_phys >> 32) as u32); // DW3: CTBAU

        drop(state);

        // Full barrier (real `mfence` on x86) so ALL descriptor writes - FIS, PRDT,
        // command header - are globally visible before we ring the CI doorbell.
        // (Ordering::Release alone is a compiler-only fence on x86 and emits nothing.)
        fence(Ordering::SeqCst);

        // Clear port interrupt status and error
        mmio_w32(base, pb + PORT_IS, 0xFFFF_FFFF);
        mmio_w32(base, pb + PORT_SERR, 0xFFFF_FFFF);

        // Known-good drivers (OSDev/Linux libahci) wait for the port to be idle -
        // PxTFD.BSY and PxTFD.DRQ clear - BEFORE issuing.  Issuing while the previous
        // command's DMA is still in flight corrupts the shared command table /
        // DATA_BUF, which is exactly the bug where back-to-back writes all persisted
        // the LAST sector's data.
        let mut wspin = 0u64;
        while { mmio_r32(base, pb + PORT_TFD) } & 0x88 != 0 {
            // BSY (0x80) | DRQ (0x08)
            wspin += 1;
            if wspin > 50_000_000 {
                return Err("ahci: port busy before issue");
            }
            crate::arch::nop();
        }

        // Issue command in slot 0
        mmio_w32(base, pb + PORT_CI, 1);

        // Poll until slot 0 clears (command complete), watching for task-file errors
        // via PxIS.TFES (bit 30) and the TFD ERR/DF status bits.
        let mut spins = 0u64;
        loop {
            let is = { mmio_r32(base, pb + PORT_IS) };
            let tfd = { mmio_r32(base, pb + PORT_TFD) };
            if is & (1 << 30) != 0 || tfd & 0x01 != 0 || tfd & 0x20 != 0 {
                return Err("ahci: ATA command error");
            }
            if { mmio_r32(base, pb + PORT_CI) } & 1 == 0 {
                break;
            } // slot 0 done
            spins += 1;
            if spins > 50_000_000 {
                return Err("ahci: command timeout");
            }
            crate::arch::nop();
        }

        // Wait for the device to return fully idle before returning, so the caller
        // can safely reuse DATA_BUF / the command table for the next command - CI
        // clearing alone does not guarantee the data DMA has drained under VBox.
        let mut dspin = 0u64;
        while { mmio_r32(base, pb + PORT_TFD) } & 0x88 != 0 {
            dspin += 1;
            if dspin > 50_000_000 {
                return Err("ahci: device busy after command");
            }
            crate::arch::nop();
        }

        // Also require SATA Active (outstanding NCQ tags) to drain to zero.
        let mut aspin = 0u64;
        while { mmio_r32(base, pb + PORT_SACT) } != 0 {
            aspin += 1;
            if aspin > 50_000_000 {
                break;
            }
            crate::arch::nop();
        }

        Ok(())
    }
}

// -- Port engine start / stop ----------------------------------------------

/// Stop the port DMA engine cleanly before modifying DMA pointers.
fn stop_port(base: u64, pb: u32) {
    // Clear ST (start) - stops command processing
    let cmd = unsafe { mmio_r32(base, pb + PORT_CMD) };
    unsafe {
        mmio_w32(base, pb + PORT_CMD, cmd & !(CMD_ST));
    }
    // Wait for CR (command list running) to clear
    for _ in 0..500_000u32 {
        if unsafe { mmio_r32(base, pb + PORT_CMD) } & CMD_CR == 0 {
            break;
        }
        crate::arch::nop();
    }
    // Clear FRE (FIS receive enable)
    let cmd = unsafe { mmio_r32(base, pb + PORT_CMD) };
    unsafe {
        mmio_w32(base, pb + PORT_CMD, cmd & !CMD_FRE);
    }
    // Wait for FR (FIS receive running) to clear
    for _ in 0..500_000u32 {
        if unsafe { mmio_r32(base, pb + PORT_CMD) } & CMD_FR == 0 {
            break;
        }
        crate::arch::nop();
    }
}

/// Start the port DMA engine (enable FRE then ST).
fn start_port(base: u64, pb: u32) {
    let cmd = unsafe { mmio_r32(base, pb + PORT_CMD) };
    // Enable FIS receive first
    unsafe {
        mmio_w32(base, pb + PORT_CMD, cmd | CMD_FRE);
    }
    // Then start command processing
    let cmd = unsafe { mmio_r32(base, pb + PORT_CMD) };
    unsafe {
        mmio_w32(base, pb + PORT_CMD, cmd | CMD_ST);
    }
}

// -- Helpers ---------------------------------------------------------------

/// Byte offset of a port's register set relative to the HBA MMIO base.
fn port_base(hba: u64, port: u32) -> u32 {
    // Port register sets start at HBA+0x100, each 0x80 bytes apart
    0x100 + port * 0x80
}

unsafe fn mmio_r32(base: u64, reg: u32) -> u32 {
    unsafe { core::ptr::read_volatile((base + reg as u64) as *const u32) }
}
unsafe fn mmio_w32(base: u64, reg: u32, val: u32) {
    unsafe {
        core::ptr::write_volatile((base + reg as u64) as *mut u32, val);
    }
}

// -- PCI scanner -----------------------------------------------------------

/// Scan PCI bus for an AHCI controller.
/// Returns (bus, device, function, bar5_value) or None.
fn find_ahci_pci() -> Option<(u8, u8, u8, u32)> {
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

                if class == AHCI_CLASS && subclass == AHCI_SUBCLASS && progif == AHCI_PROGIF {
                    // BAR5 is the AHCI base address register
                    let bar5 = pci_read(bus, dev, func, 0x24);
                    if bar5 & 1 != 0 {
                        continue;
                    } // I/O BAR - skip
                    crate::println!(
                        "[ahci] found at {:02x}:{:02x}.{} bar5={:#x}",
                        bus,
                        dev,
                        func,
                        bar5
                    );
                    return Some((bus, dev, func, bar5));
                }
            }
        }
    }
    None
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
