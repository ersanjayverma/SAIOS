use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{compiler_fence, fence, AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::pci;
use crate::{pmm, vmm};

const PCI_COMMAND_OFFSET: u8 = 0x04;
const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

const AHCI_REG_GHC: usize = 0x04;
const AHCI_REG_PI: usize = 0x0C;

const AHCI_GHC_AE: u32 = 1 << 31;

const AHCI_PORT_BASE: usize = 0x100;
const AHCI_PORT_STRIDE: usize = 0x80;

const AHCI_PX_CLB: usize = 0x00;
const AHCI_PX_CLBU: usize = 0x04;
const AHCI_PX_FB: usize = 0x08;
const AHCI_PX_FBU: usize = 0x0C;
const AHCI_PX_IS: usize = 0x10;
const AHCI_PX_CMD: usize = 0x18;
const AHCI_PX_TFD: usize = 0x20;
const AHCI_PX_SIG: usize = 0x24;
const AHCI_PX_SSTS: usize = 0x28;
const AHCI_PX_SERR: usize = 0x30;
const AHCI_PX_CI: usize = 0x38;

const AHCI_PX_CMD_ST: u32 = 1 << 0;
const AHCI_PX_CMD_FRE: u32 = 1 << 4;
const AHCI_PX_CMD_FR: u32 = 1 << 14;
const AHCI_PX_CMD_CR: u32 = 1 << 15;

const AHCI_TFD_BSY: u32 = 1 << 7;
const AHCI_TFD_DRQ: u32 = 1 << 3;
const AHCI_PXIS_TFES: u32 = 1 << 30;

const SATA_SIG_ATA: u32 = 0x0000_0101;

const ATA_CMD_IDENTIFY_DEVICE: u8 = 0xEC;
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;

const FIS_TYPE_REG_H2D: u8 = 0x27;

// Reduced from 1M to prevent long stalls on unresponsive hardware
const AHCI_WAIT_ITERS: usize = 50_000;
const AHCI_QUICK_ITERS: usize = 5_000;

#[repr(C, packed)]
struct HbaCmdHeader {
    flags: u16,
    prdtl: u16,
    prdbc: u32,
    ctba: u32,
    ctbau: u32,
    reserved: [u32; 4],
}

#[repr(C, packed)]
struct HbaPrdtEntry {
    dba: u32,
    dbau: u32,
    reserved: u32,
    dbc_ioc: u32,
}

#[repr(C, packed)]
struct HbaCmdTable {
    cfis: [u8; 64],
    acmd: [u8; 16],
    reserved: [u8; 48],
    prdt: [HbaPrdtEntry; 1],
}

struct DmaAllocation {
    phys: u64,
    virt: u64,
    _pages: usize,
}

#[derive(Clone, Debug)]
pub struct AhciDisk {
    pub id: u32,
    pub name: String,
    pub controller: String,
    pub port: u8,
    pub backing: String,
    pub sector_size: u16,
    pub total_sectors: u64,
    pub model: String,
}

struct AhciPortRuntime {
    port: u8,
    sector_size: u16,
    total_sectors: u64,
    model: String,
    command_list: DmaAllocation,
    _received_fis: DmaAllocation,
    command_table: DmaAllocation,
    data_buffer: DmaAllocation,
}

struct AhciControllerRuntime {
    _mmio_mapping_virt: u64,
    mmio: *mut u8,
    ports: Vec<AhciPortRuntime>,
}

struct AhciDiskBinding {
    disk_id: u32,
    controller_index: usize,
    port_index: usize,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AhciControllerState {
    Discovered,
    Faulted,
}

#[derive(Clone, Debug)]
pub struct AhciController {
    pub name: String,
    pub backing: String,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub abar: Option<u64>,
    pub state: AhciControllerState,
    pub last_error: Option<String>,
}

struct AhciState {
    initialized: bool,
    controllers: Vec<AhciController>,
    runtimes: Vec<AhciControllerRuntime>,
    disks: Vec<AhciDisk>,
    bindings: Vec<AhciDiskBinding>,
    diagnostics: Vec<String>,
    next_disk_id: u32,
}

impl AhciState {
    fn new() -> Self {
        Self {
            initialized: false,
            controllers: Vec::new(),
            runtimes: Vec::new(),
            disks: Vec::new(),
            bindings: Vec::new(),
            diagnostics: Vec::new(),
            next_disk_id: 1,
        }
    }
}

static STATE: StaticCell<Option<AhciState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_state_mut<R>(f: impl FnOnce(&mut AhciState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(AhciState::new());
            }
            slot.as_mut().expect("ahci state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&AhciState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(AhciState::new());
            }
            slot.as_ref().expect("ahci state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn looks_like_ahci(dev: &pci::PciDevice) -> bool {
    dev.class == 0x01 && dev.subclass == 0x06 && dev.prog_if == 0x01
}

fn read32(base: *mut u8, offset: usize) -> u32 {
    compiler_fence(Ordering::SeqCst);
    let value = unsafe { ptr::read_volatile(base.add(offset).cast::<u32>()) };
    fence(Ordering::SeqCst);
    value
}

fn write32(base: *mut u8, offset: usize, value: u32) {
    compiler_fence(Ordering::SeqCst);
    unsafe {
        ptr::write_volatile(base.add(offset).cast::<u32>(), value);
    }
    fence(Ordering::SeqCst);
}

fn record_diag(
    state: &mut AhciState,
    stage: &str,
    dev: &pci::PciDevice,
    port: Option<u8>,
    detail: &str,
) {
    let location = format!("{:02x}:{:02x}.{}", dev.bus, dev.device, dev.function);
    let entry = if let Some(port_idx) = port {
        format!(
            "stage={} controller={} port={} vendor={:04x} device={:04x} detail={}",
            stage, location, port_idx, dev.vendor_id, dev.device_id, detail
        )
    } else {
        format!(
            "stage={} controller={} vendor={:04x} device={:04x} detail={}",
            stage, location, dev.vendor_id, dev.device_id, detail
        )
    };
    state.diagnostics.push(entry);
}

fn resolve_abar(dev: &pci::PciDevice) -> Result<u64, &'static str> {
    let bar = pci::read_bar(dev, 5).ok_or("ahci: missing BAR5")?;
    if bar.is_io {
        return Err("ahci: BAR5 is I/O, MMIO required");
    }
    if bar.base < 0x1000 {
        return Err("ahci: BAR5 base appears invalid");
    }
    Ok(bar.base)
}

fn map_mmio_window(phys_base: u64, size_bytes: usize, owner: &str) -> Result<(u64, *mut u8), &'static str> {
    let aligned_phys = phys_base & !(vmm::PAGE_SIZE - 1);
    let offset = (phys_base - aligned_phys) as usize;
    let total = offset.checked_add(size_bytes).ok_or("ahci: mmio window overflow")?;
    let pages = total.div_ceil(vmm::PAGE_SIZE as usize);
    let virt = vmm::map_physical_anywhere(
        aligned_phys,
        pages,
        vmm::FLAG_READ | vmm::FLAG_WRITE | vmm::FLAG_DEVICE,
        owner,
    )?;
    let mmio = virt
        .checked_add(offset as u64)
        .ok_or("ahci: mmio virtual overflow")? as *mut u8;
    Ok((virt, mmio))
}

fn alloc_dma_pages(pages: usize, owner: &str) -> Result<DmaAllocation, &'static str> {
    if pages == 0 {
        return Err("ahci: dma pages must be > 0");
    }
    let phys = pmm::alloc_pages(pages).ok_or("ahci: dma physical allocation failed")?;
    let virt = match vmm::map_physical_anywhere(phys, pages, vmm::FLAG_READ | vmm::FLAG_WRITE, owner) {
        Ok(virt) => virt,
        Err(err) => {
            let _ = pmm::free_pages_range(phys, pages);
            return Err(err);
        }
    };

    unsafe {
        ptr::write_bytes(virt as *mut u8, 0, pages * vmm::PAGE_SIZE as usize);
    }

    Ok(DmaAllocation {
        phys,
        virt,
        _pages: pages,
    })
}

fn port_offset(port: u8) -> usize {
    AHCI_PORT_BASE + (port as usize) * AHCI_PORT_STRIDE
}

fn port_read32(mmio: *mut u8, port: u8, reg: usize) -> u32 {
    read32(mmio, port_offset(port) + reg)
}

fn port_write32(mmio: *mut u8, port: u8, reg: usize, value: u32) {
    write32(mmio, port_offset(port) + reg, value);
}

fn wait_until_port_timeout(mmio: *mut u8, port: u8, reg: usize, mask: u32, set: bool, iters: usize) -> bool {
    for i in 0..iters {
        let value = port_read32(mmio, port, reg);
        let matches = (value & mask) != 0;
        if matches == set {
            return true;
        }
        if (i & 0x3FF) == 0 {
            crate::scheduler::maybe_preempt();
            if (i & 0x3FFF) == 0 {
                crate::scheduler::yield_now();
            }
        }
        core::hint::spin_loop();
    }
    false
}

fn enable_pci_command(dev: &pci::PciDevice, bits: u16) {
    let command = pci::read_u16(dev.bus, dev.device, dev.function, PCI_COMMAND_OFFSET);
    if (command & bits) != bits {
        pci::write_u16(dev.bus, dev.device, dev.function, PCI_COMMAND_OFFSET, command | bits);
    }
}

fn port_has_sata_device(mmio: *mut u8, port: u8) -> bool {
    let ssts = port_read32(mmio, port, AHCI_PX_SSTS);
    let det = ssts & 0x0F;
    let ipm = (ssts >> 8) & 0x0F;
    if det != 0x03 || ipm != 0x01 {
        return false;
    }

    let sig = port_read32(mmio, port, AHCI_PX_SIG);
    sig == SATA_SIG_ATA
}

fn stop_port_engine(mmio: *mut u8, port: u8) -> Result<(), &'static str> {
    let mut cmd = port_read32(mmio, port, AHCI_PX_CMD);
    cmd &= !AHCI_PX_CMD_ST;
    cmd &= !AHCI_PX_CMD_FRE;
    port_write32(mmio, port, AHCI_PX_CMD, cmd);

    // Use quick timeout for port stop - if it doesn't respond fast, skip it
    if !wait_until_port_timeout(mmio, port, AHCI_PX_CMD, AHCI_PX_CMD_CR | AHCI_PX_CMD_FR, false, AHCI_QUICK_ITERS) {
        return Err("ahci: port stop timed out");
    }
    Ok(())
}

fn start_port_engine(mmio: *mut u8, port: u8) {
    let mut cmd = port_read32(mmio, port, AHCI_PX_CMD);
    cmd |= AHCI_PX_CMD_FRE;
    port_write32(mmio, port, AHCI_PX_CMD, cmd);
    cmd |= AHCI_PX_CMD_ST;
    port_write32(mmio, port, AHCI_PX_CMD, cmd);
}

fn init_port_runtime(mmio: *mut u8, port: u8, owner: &str) -> Result<AhciPortRuntime, &'static str> {
    stop_port_engine(mmio, port)?;

    let command_list = alloc_dma_pages(1, owner)?;
    let received_fis = alloc_dma_pages(1, owner)?;
    let command_table = alloc_dma_pages(1, owner)?;
    let data_buffer = alloc_dma_pages(1, owner)?;

    port_write32(mmio, port, AHCI_PX_CLB, command_list.phys as u32);
    port_write32(mmio, port, AHCI_PX_CLBU, (command_list.phys >> 32) as u32);
    port_write32(mmio, port, AHCI_PX_FB, received_fis.phys as u32);
    port_write32(mmio, port, AHCI_PX_FBU, (received_fis.phys >> 32) as u32);

    port_write32(mmio, port, AHCI_PX_IS, u32::MAX);
    port_write32(mmio, port, AHCI_PX_SERR, u32::MAX);

    start_port_engine(mmio, port);

    Ok(AhciPortRuntime {
        port,
        sector_size: 512,
        total_sectors: 0,
        model: String::new(),
        command_list,
        _received_fis: received_fis,
        command_table,
        data_buffer,
    })
}

fn wait_port_ready(mmio: *mut u8, port: u8) -> Result<(), &'static str> {
    for i in 0..AHCI_WAIT_ITERS {
        let tfd = port_read32(mmio, port, AHCI_PX_TFD);
        if (tfd & (AHCI_TFD_BSY | AHCI_TFD_DRQ)) == 0 {
            return Ok(());
        }
        if (i & 0x3FF) == 0 {
            crate::scheduler::maybe_preempt();
            if (i & 0x3FFF) == 0 {
                crate::scheduler::yield_now();
            }
        }
        core::hint::spin_loop();
    }
    Err("ahci: port busy timeout")
}

fn issue_ata_command(
    mmio: *mut u8,
    runtime: &mut AhciPortRuntime,
    command: u8,
    lba: u64,
    count: u16,
    write: bool,
    transfer_len: usize,
) -> Result<(), &'static str> {
    if transfer_len == 0 || transfer_len > vmm::PAGE_SIZE as usize {
        return Err("ahci: transfer length unsupported");
    }

    wait_port_ready(mmio, runtime.port)?;
    port_write32(mmio, runtime.port, AHCI_PX_IS, u32::MAX);

    unsafe {
        let header = runtime.command_list.virt as *mut HbaCmdHeader;
        ptr::write_bytes(header as *mut u8, 0, core::mem::size_of::<HbaCmdHeader>());

        (*header).flags = (5u16 & 0x1F) | if write { 1 << 6 } else { 0 };
        (*header).prdtl = 1;
        (*header).ctba = runtime.command_table.phys as u32;
        (*header).ctbau = (runtime.command_table.phys >> 32) as u32;

        let table = runtime.command_table.virt as *mut HbaCmdTable;
        ptr::write_bytes(table as *mut u8, 0, core::mem::size_of::<HbaCmdTable>());

        (*table).prdt[0].dba = runtime.data_buffer.phys as u32;
        (*table).prdt[0].dbau = (runtime.data_buffer.phys >> 32) as u32;
        (*table).prdt[0].dbc_ioc = (((transfer_len as u32).saturating_sub(1)) & 0x3F_FFFF) | (1 << 31);

        let cfis = &mut (*table).cfis;
        cfis[0] = FIS_TYPE_REG_H2D;
        cfis[1] = 1 << 7;
        cfis[2] = command;
        cfis[7] = 1 << 6;
        cfis[4] = (lba & 0xFF) as u8;
        cfis[5] = ((lba >> 8) & 0xFF) as u8;
        cfis[6] = ((lba >> 16) & 0xFF) as u8;
        cfis[8] = ((lba >> 24) & 0xFF) as u8;
        cfis[9] = ((lba >> 32) & 0xFF) as u8;
        cfis[10] = ((lba >> 40) & 0xFF) as u8;
        cfis[12] = (count & 0xFF) as u8;
        cfis[13] = ((count >> 8) & 0xFF) as u8;
    }

    port_write32(mmio, runtime.port, AHCI_PX_CI, 1);

    for i in 0..AHCI_WAIT_ITERS {
        let ci = port_read32(mmio, runtime.port, AHCI_PX_CI);
        let is = port_read32(mmio, runtime.port, AHCI_PX_IS);
        if (is & AHCI_PXIS_TFES) != 0 {
            return Err("ahci: task file error");
        }
        if (ci & 1) == 0 {
            return Ok(());
        }
        if (i & 0x3FF) == 0 {
            crate::scheduler::maybe_preempt();
            if (i & 0x3FFF) == 0 {
                crate::scheduler::yield_now();
            }
        }
        core::hint::spin_loop();
    }

    Err("ahci: command timeout")
}

fn parse_identify_model(words: &[u16]) -> String {
    let mut bytes = [0u8; 40];
    for i in 0..20usize {
        let w = words[27 + i];
        bytes[i * 2] = (w >> 8) as u8;
        bytes[i * 2 + 1] = (w & 0xFF) as u8;
    }
    let s = core::str::from_utf8(&bytes).unwrap_or("unknown").trim();
    s.to_string()
}

fn identify_port(mmio: *mut u8, runtime: &mut AhciPortRuntime) -> Result<(), &'static str> {
    issue_ata_command(mmio, runtime, ATA_CMD_IDENTIFY_DEVICE, 0, 1, false, 512)?;

    let mut words = [0u16; 256];
    unsafe {
        let src = runtime.data_buffer.virt as *const u16;
        for (idx, slot) in words.iter_mut().enumerate() {
            *slot = ptr::read_unaligned(src.add(idx));
        }
    }

    let lba48 = (words[100] as u64)
        | ((words[101] as u64) << 16)
        | ((words[102] as u64) << 32)
        | ((words[103] as u64) << 48);
    let lba28 = (words[60] as u64) | ((words[61] as u64) << 16);
    runtime.total_sectors = if lba48 != 0 { lba48 } else { lba28 };
    runtime.sector_size = 512;
    runtime.model = parse_identify_model(&words);
    Ok(())
}

fn read_sector_runtime(mmio: *mut u8, runtime: &mut AhciPortRuntime, lba: u64, out: &mut [u8]) -> Result<(), &'static str> {
    if out.len() != runtime.sector_size as usize {
        return Err("ahci: invalid read buffer size");
    }
    issue_ata_command(
        mmio,
        runtime,
        ATA_CMD_READ_DMA_EXT,
        lba,
        1,
        false,
        runtime.sector_size as usize,
    )?;
    unsafe {
        ptr::copy_nonoverlapping(runtime.data_buffer.virt as *const u8, out.as_mut_ptr(), out.len());
    }
    Ok(())
}

fn write_sector_runtime(mmio: *mut u8, runtime: &mut AhciPortRuntime, lba: u64, data: &[u8]) -> Result<(), &'static str> {
    if data.len() != runtime.sector_size as usize {
        return Err("ahci: invalid write buffer size");
    }
    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), runtime.data_buffer.virt as *mut u8, data.len());
    }
    issue_ata_command(
        mmio,
        runtime,
        ATA_CMD_WRITE_DMA_EXT,
        lba,
        1,
        true,
        runtime.sector_size as usize,
    )
}

fn rescan_locked(state: &mut AhciState) {
    state.controllers.clear();
    state.runtimes.clear();
    state.disks.clear();
    state.bindings.clear();
    state.diagnostics.clear();
    state.next_disk_id = 1;

    let mut index = 0usize;
    let mut detected_any = false;
    for dev in pci::devices() {
        if !looks_like_ahci(&dev) {
            continue;
        }
        detected_any = true;

        record_diag(state, "pci_detection", &dev, None, "ahci class device discovered");

        let abar = match resolve_abar(&dev) {
            Ok(phys) => Some(phys),
            Err(err) => {
                record_diag(state, "bar_mapping", &dev, None, err);
                None
            }
        };

        let mut controller_state = AhciControllerState::Discovered;
        let mut last_error = None;

        if let Some(abar_phys) = abar {
            enable_pci_command(&dev, PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER);
            match map_mmio_window(abar_phys, 0x2000, "ahci-abar") {
                Ok((mapping, mmio)) => {
                    let ghc = read32(mmio, AHCI_REG_GHC);
                    write32(mmio, AHCI_REG_GHC, ghc | AHCI_GHC_AE);
                    let ghc_now = read32(mmio, AHCI_REG_GHC);
                    if (ghc_now & AHCI_GHC_AE) == 0 {
                        controller_state = AhciControllerState::Faulted;
                        let detail = "ahci: controller enable bit did not latch".to_string();
                        record_diag(state, "controller_init", &dev, None, detail.as_str());
                        last_error = Some(detail);
                    }

                    let pi = read32(mmio, AHCI_REG_PI);
                    let mut runtime = AhciControllerRuntime {
                        _mmio_mapping_virt: mapping,
                        mmio,
                        ports: Vec::new(),
                    };

                    if pi == 0 {
                        record_diag(
                            state,
                            "port_scan",
                            &dev,
                            None,
                            "ahci: no implemented ports in PI register",
                        );
                    }

                    for port in 0u8..32u8 {
                        if (pi & (1u32 << port)) == 0 {
                            continue;
                        }
                        if !port_has_sata_device(mmio, port) {
                            continue;
                        }

                        let mut port_runtime = match init_port_runtime(mmio, port, "ahci-port") {
                            Ok(p) => p,
                            Err(e) => {
                                controller_state = AhciControllerState::Faulted;
                                let detail = e.to_string();
                                record_diag(state, "port_scan", &dev, Some(port), detail.as_str());
                                last_error = Some(detail);
                                continue;
                            }
                        };

                        if let Err(e) = identify_port(mmio, &mut port_runtime) {
                            controller_state = AhciControllerState::Faulted;
                            let detail = e.to_string();
                            record_diag(state, "identify", &dev, Some(port), detail.as_str());
                            last_error = Some(detail);
                            continue;
                        }

                        if port_runtime.total_sectors == 0 {
                            controller_state = AhciControllerState::Faulted;
                            let detail = "ahci: IDENTIFY returned zero sectors".to_string();
                            record_diag(state, "identify", &dev, Some(port), detail.as_str());
                            last_error = Some(detail);
                            continue;
                        }

                        let disk_id = state.next_disk_id;
                        state.next_disk_id = state.next_disk_id.wrapping_add(1);

                        let disk = AhciDisk {
                            id: disk_id,
                            name: format!("sata{}p{}", index, port),
                            controller: format!("ahci{}", index),
                            port,
                            backing: format!("ahci{}:{}", index, port),
                            sector_size: port_runtime.sector_size,
                            total_sectors: port_runtime.total_sectors,
                            model: port_runtime.model.clone(),
                        };
                        state.disks.push(disk);

                        let port_index = runtime.ports.len();
                        runtime.ports.push(port_runtime);
                        state.bindings.push(AhciDiskBinding {
                            disk_id,
                            controller_index: state.runtimes.len(),
                            port_index,
                        });
                    }

                    state.runtimes.push(runtime);
                }
                Err(err) => {
                    controller_state = AhciControllerState::Faulted;
                    let detail = err.to_string();
                    record_diag(state, "bar_mapping", &dev, None, detail.as_str());
                    last_error = Some(detail);
                }
            }
        } else {
            controller_state = AhciControllerState::Faulted;
            let detail = "ahci: missing ABAR MMIO window".to_string();
            last_error = Some(detail);
        }

        state.controllers.push(AhciController {
            name: format!("ahci{}", index),
            backing: format!("pci {:02x}:{:02x}.{}", dev.bus, dev.device, dev.function),
            bus: dev.bus,
            device: dev.device,
            function: dev.function,
            vendor_id: dev.vendor_id,
            device_id: dev.device_id,
            abar,
            state: controller_state,
            last_error,
        });
        index = index.saturating_add(1);
    }

    if !detected_any {
        state
            .diagnostics
            .push("stage=pci_detection detail=no AHCI controllers discovered".to_string());
    }
}

pub fn init() {
    with_state_mut(|state| {
        if state.initialized {
            return;
        }
        rescan_locked(state);
        state.initialized = true;
    });
}

pub fn rescan() {
    with_state_mut(|state| {
        rescan_locked(state);
        state.initialized = true;
    });
}

pub fn controllers() -> Vec<AhciController> {
    init();
    with_state(|state| state.controllers.clone())
}

pub fn controllers_cached() -> Vec<AhciController> {
    with_state(|state| state.controllers.clone())
}

pub fn controller_count() -> usize {
    init();
    with_state(|state| state.controllers.len())
}

pub fn disks() -> Vec<AhciDisk> {
    init();
    with_state(|state| state.disks.clone())
}

pub fn disks_cached() -> Vec<AhciDisk> {
    with_state(|state| state.disks.clone())
}

pub fn diagnostics() -> Vec<String> {
    init();
    with_state(|state| state.diagnostics.clone())
}

pub fn diagnostics_cached() -> Vec<String> {
    with_state(|state| state.diagnostics.clone())
}

pub fn read_sector(disk_id: u32, lba: u64, out: &mut [u8]) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let binding = state
            .bindings
            .iter()
            .find(|b| b.disk_id == disk_id)
            .ok_or("ahci: disk not found")?;

        let runtime = state
            .runtimes
            .get_mut(binding.controller_index)
            .ok_or("ahci: controller runtime missing")?;
        let port = runtime
            .ports
            .get_mut(binding.port_index)
            .ok_or("ahci: port runtime missing")?;

        if lba >= port.total_sectors {
            return Err("ahci: lba out of range");
        }

        read_sector_runtime(runtime.mmio, port, lba, out)
    })
}

pub fn write_sector(disk_id: u32, lba: u64, data: &[u8]) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let binding = state
            .bindings
            .iter()
            .find(|b| b.disk_id == disk_id)
            .ok_or("ahci: disk not found")?;

        let runtime = state
            .runtimes
            .get_mut(binding.controller_index)
            .ok_or("ahci: controller runtime missing")?;
        let port = runtime
            .ports
            .get_mut(binding.port_index)
            .ok_or("ahci: port runtime missing")?;

        if lba >= port.total_sectors {
            return Err("ahci: lba out of range");
        }

        write_sector_runtime(runtime.mmio, port, lba, data)
    })
}

pub fn flush(_disk_id: u32) -> Result<(), &'static str> {
    Ok(())
}
