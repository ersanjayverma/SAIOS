use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use heapless::Deque;
use hal::arch::x86_64::sync::StaticCell;

use crate::console::{KeyEvent, MouseButtons, MouseEvent};
use crate::pci::{self, PciBar, PciDevice};
use crate::pmm;
use crate::vmm;

const PCI_COMMAND_OFFSET: u8 = 0x04;
const PCI_COMMAND_MEMORY_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

const XHCI_CAP_CAPLENGTH: usize = 0x00;
const XHCI_CAP_HCIVERSION: usize = 0x02;
const XHCI_CAP_HCSPARAMS1: usize = 0x04;
const XHCI_CAP_HCSPARAMS2: usize = 0x08;
const XHCI_CAP_HCCPARAMS1: usize = 0x10;
const XHCI_CAP_RTSOFF: usize = 0x18;

const XHCI_OP_USBCMD: usize = 0x00;
const XHCI_OP_USBSTS: usize = 0x04;
const XHCI_OP_PAGESIZE: usize = 0x08;
const XHCI_OP_CRCR: usize = 0x18;
const XHCI_OP_DCBAAP: usize = 0x30;
const XHCI_OP_CONFIG: usize = 0x38;
const XHCI_OP_PORTREGS_BASE: usize = 0x400;
const XHCI_OP_PORTREGS_STRIDE: usize = 0x10;

const XHCI_RT_IR0: usize = 0x20;
const XHCI_IR_IMAN: usize = 0x00;
const XHCI_IR_IMOD: usize = 0x04;
const XHCI_IR_ERSTSZ: usize = 0x08;
const XHCI_IR_ERSTBA: usize = 0x10;
const XHCI_IR_ERDP: usize = 0x18;

const XHCI_USBCMD_RUN_STOP: u32 = 1 << 0;
const XHCI_USBCMD_HCRST: u32 = 1 << 1;
const XHCI_USBCMD_INTE: u32 = 1 << 2;

const XHCI_USBSTS_HCHALTED: u32 = 1 << 0;
const XHCI_USBSTS_CNR: u32 = 1 << 11;

const XHCI_PORTSC_CCS: u32 = 1 << 0;

const XHCI_EXTCAP_ID_LEGACY_SUPPORT: u8 = 0x01;
const XHCI_WAIT_ITERS: usize = 1_000_000;
const XHCI_TRB_TYPE_LINK: u32 = 6;

#[repr(C, align(16))]
struct Trb {
    parameter_lo: u32,
    parameter_hi: u32,
    status: u32,
    control: u32,
}

#[repr(C, align(64))]
struct EventRingSegmentTableEntry {
    ring_segment_base_lo: u32,
    ring_segment_base_hi: u32,
    ring_segment_size: u32,
    reserved: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbDeviceKind {
    Unknown,
    HidKeyboard,
    HidMouse,
}

struct DmaAllocation {
    phys: u64,
    virt: u64,
}

#[allow(dead_code)]
struct XhciRuntime {
    mmio_mapping_virt: u64,
    mmio_virt: u64,
    dcbaa: DmaAllocation,
    scratchpad_array: Option<DmaAllocation>,
    scratchpads: Vec<DmaAllocation>,
    command_ring: DmaAllocation,
    event_ring: DmaAllocation,
    erst: DmaAllocation,
}

struct XhciProbeResult {
    version: u16,
    max_ports: u8,
    connected_ports: u8,
    max_slots: u8,
    handed_off: bool,
    runtime: Option<XhciRuntime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbControllerState {
    Discovered,
    Initialized,
    Faulted,
}

#[derive(Debug, Clone)]
pub struct UsbController {
    pub name: String,
    pub kind: &'static str,
    pub backing: String,
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub prog_if: u8,
    pub mmio_base: Option<u64>,
    pub io_base: Option<u16>,
    pub version: Option<u16>,
    pub port_count: u8,
    pub connected_ports: u8,
    pub max_slots: u8,
    pub runtime_ready: bool,
    pub hid_device_kind: UsbDeviceKind,
    pub state: UsbControllerState,
    pub last_error: Option<String>,
}

struct UsbState {
    initialized: bool,
    controllers: Vec<UsbController>,
}

impl UsbState {
    const fn new() -> Self {
        Self {
            initialized: false,
            controllers: Vec::new(),
        }
    }
}

static STATE: StaticCell<Option<UsbState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);
static KEY_QUEUE: StaticCell<Deque<KeyEvent, 128>> = StaticCell::new(Deque::new());
static KEY_QUEUE_LOCK: AtomicBool = AtomicBool::new(false);
static MOUSE_QUEUE: StaticCell<Deque<MouseEvent, 64>> = StaticCell::new(Deque::new());
static MOUSE_QUEUE_LOCK: AtomicBool = AtomicBool::new(false);
static HID_KEYBOARD_STATE: StaticCell<HidKeyboardState> = StaticCell::new(HidKeyboardState::new());

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

fn queue_lock(lock: &AtomicBool) {
    while lock
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn queue_unlock(lock: &AtomicBool) {
    lock.store(false, Ordering::Release);
}

#[derive(Copy, Clone)]
struct HidKeyboardState {
    modifiers: u8,
    pressed: [u8; 6],
}

impl HidKeyboardState {
    const fn new() -> Self {
        Self {
            modifiers: 0,
            pressed: [0; 6],
        }
    }
}

fn with_state_mut<R>(f: impl FnOnce(&mut UsbState) -> R) -> R {
    lock();
    let out = {
        // SAFETY: guarded by spin lock.
        let slot = unsafe { &mut *STATE.get() };
        if slot.is_none() {
            *slot = Some(UsbState::new());
        }
        f(slot.as_mut().expect("usb state unavailable"))
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&UsbState) -> R) -> R {
    lock();
    let out = {
        // SAFETY: guarded by spin lock.
        let slot = unsafe { &mut *STATE.get() };
        if slot.is_none() {
            *slot = Some(UsbState::new());
        }
        f(slot.as_ref().expect("usb state unavailable"))
    };
    unlock();
    out
}

fn controller_kind(dev: &PciDevice) -> &'static str {
    match dev.prog_if {
        0x00 => "uhci",
        0x10 => "ohci",
        0x20 => "ehci",
        0x30 => "xhci",
        0x80 => "usb-device",
        0xFE => "usb-device",
        _ => "usb-host",
    }
}

fn read8(base: *mut u8, offset: usize) -> u8 {
    unsafe { ptr::read_volatile(base.add(offset)) }
}

fn read16(base: *mut u8, offset: usize) -> u16 {
    unsafe { ptr::read_volatile(base.add(offset).cast::<u16>()) }
}

fn read32(base: *mut u8, offset: usize) -> u32 {
    unsafe { ptr::read_volatile(base.add(offset).cast::<u32>()) }
}

fn write32(base: *mut u8, offset: usize, value: u32) {
    unsafe {
        ptr::write_volatile(base.add(offset).cast::<u32>(), value);
    }
}

fn write64(base: *mut u8, offset: usize, value: u64) {
    unsafe {
        ptr::write_volatile(base.add(offset).cast::<u64>(), value);
    }
}

fn wait_until(base: *mut u8, offset: usize, mask: u32, set: bool) -> bool {
    for _ in 0..XHCI_WAIT_ITERS {
        let value = read32(base, offset);
        let matches = (value & mask) != 0;
        if matches == set {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn enable_pci_command(dev: &PciDevice, bits: u16) {
    let command = pci::read_u16(dev.bus, dev.device, dev.function, PCI_COMMAND_OFFSET);
    if (command & bits) != bits {
        pci::write_u16(
            dev.bus,
            dev.device,
            dev.function,
            PCI_COMMAND_OFFSET,
            command | bits,
        );
    }
}

fn map_mmio_window(
    phys_base: u64,
    size_bytes: usize,
    owner: &str,
) -> Result<(u64, *mut u8), &'static str> {
    let aligned_phys = phys_base & !(vmm::PAGE_SIZE - 1);
    let offset = (phys_base - aligned_phys) as usize;
    let total = offset
        .checked_add(size_bytes)
        .ok_or("usb: mmio window overflow")?;
    let pages = total.div_ceil(vmm::PAGE_SIZE as usize);
    let virt = vmm::map_physical_anywhere(
        aligned_phys,
        pages,
        vmm::FLAG_READ | vmm::FLAG_WRITE | vmm::FLAG_DEVICE,
        owner,
    )?;
    let base = virt
        .checked_add(offset as u64)
        .ok_or("usb: mmio virtual overflow")? as *mut u8;
    Ok((virt, base))
}

fn alloc_dma_pages(pages: usize, owner: &str) -> Result<DmaAllocation, &'static str> {
    if pages == 0 {
        return Err("usb: dma pages must be > 0");
    }

    let phys = pmm::alloc_pages(pages).ok_or("usb: dma physical allocation failed")?;
    let virt = match vmm::map_physical_anywhere(
        phys,
        pages,
        vmm::FLAG_READ | vmm::FLAG_WRITE,
        owner,
    ) {
        Ok(virt) => virt,
        Err(err) => {
            let _ = pmm::free_pages_range(phys, pages);
            return Err(err);
        }
    };

    unsafe {
        ptr::write_bytes(virt as *mut u8, 0, pages * vmm::PAGE_SIZE as usize);
    }

    Ok(DmaAllocation { phys, virt })
}

fn xhci_bios_handoff(mmio: *mut u8) -> Result<bool, &'static str> {
    let hccparams1 = read32(mmio, XHCI_CAP_HCCPARAMS1);
    let mut ext = (((hccparams1 >> 16) & 0xFFFF) as usize).saturating_mul(4);
    if ext == 0 {
        return Ok(false);
    }

    while ext != 0 && ext < 0x1000 {
        let cap_id = read8(mmio, ext);
        let next = (read16(mmio, ext) >> 8) as usize;
        if cap_id == XHCI_EXTCAP_ID_LEGACY_SUPPORT {
            let bios_owned = unsafe { mmio.add(ext + 2) };
            let os_owned = unsafe { mmio.add(ext + 3) };

            if unsafe { ptr::read_volatile(bios_owned) } == 0 {
                return Ok(false);
            }

            unsafe {
                ptr::write_volatile(os_owned, 1);
            }
            for _ in 0..XHCI_WAIT_ITERS {
                if unsafe { ptr::read_volatile(bios_owned) } == 0 {
                    return Ok(true);
                }
                core::hint::spin_loop();
            }
            return Err("usb: xhci bios handoff timed out");
        }
        if next == 0 || next.saturating_mul(4) == ext {
            break;
        }
        ext = next.saturating_mul(4);
    }

    Ok(false)
}

fn init_xhci_runtime(
    mmio_mapping_virt: u64,
    cap: *mut u8,
    op: *mut u8,
    max_slots: u8,
) -> Result<XhciRuntime, &'static str> {
    let hcsparams2 = read32(cap, XHCI_CAP_HCSPARAMS2);
    let scratchpad_count = (((hcsparams2 >> 27) & 0x1F) << 5) | ((hcsparams2 >> 21) & 0x1F);
    let page_size_bits = read32(op, XHCI_OP_PAGESIZE);
    if (page_size_bits & 0x1) == 0 {
        return Err("usb: xhci 4K page size unsupported");
    }

    let dcbaa = alloc_dma_pages(1, "usb-xhci-dcbaa")?;
    let mut scratchpad_array = None;
    let mut scratchpads = Vec::new();

    if scratchpad_count != 0 {
        let array_pages = ((scratchpad_count as usize) * core::mem::size_of::<u64>())
            .div_ceil(vmm::PAGE_SIZE as usize);
        let array = alloc_dma_pages(array_pages.max(1), "usb-xhci-scratchpad-array")?;
        let array_ptr = array.virt as *mut u64;
        for idx in 0..scratchpad_count as usize {
            let page = alloc_dma_pages(1, "usb-xhci-scratchpad")?;
            unsafe {
                ptr::write(array_ptr.add(idx), page.phys);
            }
            scratchpads.push(page);
        }
        unsafe {
            ptr::write(dcbaa.virt as *mut u64, array.phys);
        }
        scratchpad_array = Some(array);
    }

    let command_ring = alloc_dma_pages(1, "usb-xhci-command-ring")?;
    let command_trbs = command_ring.virt as *mut Trb;
    let trb_count = (vmm::PAGE_SIZE as usize) / core::mem::size_of::<Trb>();
    unsafe {
        ptr::write(
            command_trbs.add(trb_count - 1),
            Trb {
                parameter_lo: command_ring.phys as u32,
                parameter_hi: (command_ring.phys >> 32) as u32,
                status: 0,
                control: (XHCI_TRB_TYPE_LINK << 10) | (1 << 1) | 1,
            },
        );
    }

    let event_ring = alloc_dma_pages(1, "usb-xhci-event-ring")?;
    let erst = alloc_dma_pages(1, "usb-xhci-erst")?;
    unsafe {
        ptr::write(
            erst.virt as *mut EventRingSegmentTableEntry,
            EventRingSegmentTableEntry {
                ring_segment_base_lo: event_ring.phys as u32,
                ring_segment_base_hi: (event_ring.phys >> 32) as u32,
                ring_segment_size: ((vmm::PAGE_SIZE as usize) / core::mem::size_of::<Trb>()) as u32,
                reserved: 0,
            },
        );
    }

    write64(op, XHCI_OP_DCBAAP, dcbaa.phys);
    write64(op, XHCI_OP_CRCR, command_ring.phys | 1);

    let rtsoff = (read32(cap, XHCI_CAP_RTSOFF) & !0x1F) as usize;
    let ir0 = unsafe { cap.add(rtsoff + XHCI_RT_IR0) };
    write32(ir0, XHCI_IR_IMAN, 0);
    write32(ir0, XHCI_IR_IMOD, 0);
    write32(ir0, XHCI_IR_ERSTSZ, 1);
    write64(ir0, XHCI_IR_ERSTBA, erst.phys);
    write64(ir0, XHCI_IR_ERDP, event_ring.phys);

    write32(op, XHCI_OP_CONFIG, max_slots as u32);

    Ok(XhciRuntime {
        mmio_mapping_virt,
        mmio_virt: cap as u64,
        dcbaa,
        scratchpad_array,
        scratchpads,
        command_ring,
        event_ring,
        erst,
    })
}

fn probe_xhci(dev: &PciDevice, mmio_base: u64) -> Result<XhciProbeResult, &'static str> {
    enable_pci_command(dev, PCI_COMMAND_MEMORY_SPACE | PCI_COMMAND_BUS_MASTER);

    let (mapping, mmio) = map_mmio_window(mmio_base, 0x4000, "usb-xhci-probe")?;

    let result = (|| {
        let cap_length = read8(mmio, XHCI_CAP_CAPLENGTH) as usize;
        if cap_length == 0 {
            return Err("usb: xhci invalid caplength");
        }

        let version = read16(mmio, XHCI_CAP_HCIVERSION);
        let hcsparams1 = read32(mmio, XHCI_CAP_HCSPARAMS1);
        let max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;
        let max_slots = (hcsparams1 & 0xFF) as u8;
        let op = unsafe { mmio.add(cap_length) };

        let handed_off = xhci_bios_handoff(mmio)?;

        let mut command = read32(op, XHCI_OP_USBCMD);
        if (command & XHCI_USBCMD_RUN_STOP) != 0 {
            write32(op, XHCI_OP_USBCMD, command & !XHCI_USBCMD_RUN_STOP);
            if !wait_until(op, XHCI_OP_USBSTS, XHCI_USBSTS_HCHALTED, true) {
                return Err("usb: xhci halt timed out");
            }
        }

        command = read32(op, XHCI_OP_USBCMD);
        write32(op, XHCI_OP_USBCMD, command | XHCI_USBCMD_HCRST);
        if !wait_until(op, XHCI_OP_USBCMD, XHCI_USBCMD_HCRST, false) {
            return Err("usb: xhci reset timed out");
        }
        if !wait_until(op, XHCI_OP_USBSTS, XHCI_USBSTS_CNR, false) {
            return Err("usb: xhci controller-not-ready timed out");
        }

        let runtime = init_xhci_runtime(mapping, mmio, op, max_slots)?;

        command = read32(op, XHCI_OP_USBCMD);
        write32(op, XHCI_OP_USBCMD, command | XHCI_USBCMD_RUN_STOP | XHCI_USBCMD_INTE);
        if !wait_until(op, XHCI_OP_USBSTS, XHCI_USBSTS_HCHALTED, false) {
            return Err("usb: xhci run timed out");
        }

        let mut connected = 0u8;
        for port in 0..max_ports as usize {
            let portsc = read32(op, XHCI_OP_PORTREGS_BASE + port * XHCI_OP_PORTREGS_STRIDE);
            if (portsc & XHCI_PORTSC_CCS) != 0 {
                connected = connected.saturating_add(1);
            }
        }

        Ok(XhciProbeResult {
            version,
            max_ports,
            connected_ports: connected,
            max_slots,
            handed_off,
            runtime: Some(runtime),
        })
    })();

    result
}

fn choose_primary_bars(dev: &PciDevice) -> (Option<u64>, Option<u16>) {
    let mut mmio_base = None;
    let mut io_base = None;

    let mut index = 0u8;
    while index < 6 {
        let Some(bar) = pci::read_bar(dev, index) else {
            index = index.saturating_add(1);
            continue;
        };

        if bar.is_io {
            if io_base.is_none() {
                io_base = Some(bar.base as u16);
            }
        } else if mmio_base.is_none() {
            mmio_base = Some(bar.base);
        }

        if bar.is_64bit {
            index = index.saturating_add(2);
        } else {
            index = index.saturating_add(1);
        }
    }

    (mmio_base, io_base)
}

fn format_resource(bar: Option<PciBar>) -> Option<String> {
    bar.map(|bar| {
        if bar.is_io {
            format!("io@0x{:x}", bar.base)
        } else {
            format!("mmio@0x{:x}", bar.base)
        }
    })
}

fn resource_string(dev: &PciDevice) -> String {
    let mut first = None;
    let mut index = 0u8;
    while index < 6 {
        if let Some(bar) = pci::read_bar(dev, index) {
            first = format_resource(Some(bar));
            break;
        }
        index = index.saturating_add(1);
    }

    first.unwrap_or_else(|| "no-bar".to_string())
}

fn rescan_locked(state: &mut UsbState) {
    state.controllers.clear();

    let mut index = 0usize;
    for dev in pci::devices() {
        if dev.class != 0x0C || dev.subclass != 0x03 {
            continue;
        }

        let kind = controller_kind(&dev);
        let (mmio_base, io_base) = choose_primary_bars(&dev);
        let resource = resource_string(&dev);
        let mut version = None;
        let mut port_count = 0u8;
        let mut connected_ports = 0u8;
        let mut max_slots = 0u8;
        let mut runtime_ready = false;
        let mut hid_device_kind = UsbDeviceKind::Unknown;
        let mut controller_state = UsbControllerState::Discovered;
        let mut last_error = None;

        if kind == "xhci" {
            if let Some(base) = mmio_base {
                match probe_xhci(&dev, base) {
                    Ok(result) => {
                        version = Some(result.version);
                        port_count = result.max_ports;
                        connected_ports = result.connected_ports;
                        runtime_ready = result.runtime.is_some();
                        controller_state = UsbControllerState::Initialized;
                        max_slots = result.max_slots;
                        if runtime_ready && connected_ports != 0 {
                            hid_device_kind = UsbDeviceKind::Unknown;
                        }
                        if result.handed_off {
                            last_error = Some(if runtime_ready {
                                "bios-owned->os-owned handoff completed; xhci runtime ready".to_string()
                            } else {
                                "bios-owned->os-owned handoff completed".to_string()
                            });
                        }
                    }
                    Err(err) => {
                        controller_state = UsbControllerState::Faulted;
                        last_error = Some(err.to_string());
                    }
                }
            } else {
                controller_state = UsbControllerState::Faulted;
                last_error = Some("usb: xhci missing mmio bar".to_string());
            }
        }

        state.controllers.push(UsbController {
            name: format!("usb{}", index),
            kind,
            backing: format!(
                "{} pci {:02x}:{:02x}.{} {}",
                kind, dev.bus, dev.device, dev.function, resource
            ),
            bus: dev.bus,
            device: dev.device,
            function: dev.function,
            vendor_id: dev.vendor_id,
            device_id: dev.device_id,
            prog_if: dev.prog_if,
            mmio_base,
            io_base,
            version,
            port_count,
            connected_ports,
            max_slots,
            runtime_ready,
            hid_device_kind,
            state: controller_state,
            last_error,
        });
        index = index.saturating_add(1);
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

pub fn controllers() -> Vec<UsbController> {
    init();
    with_state(|state| state.controllers.clone())
}

pub fn controller_count() -> usize {
    init();
    with_state(|state| state.controllers.len())
}

pub fn host_controller_detected() -> bool {
    controller_count() != 0
}

pub fn hid_input_ready() -> bool {
    init();
    with_state(|state| {
        state.controllers.iter().any(|controller| {
            controller.kind == "xhci"
                && controller.state == UsbControllerState::Initialized
                && controller.connected_ports != 0
        })
    })
}

fn shift_active(modifiers: u8) -> bool {
    (modifiers & ((1 << 1) | (1 << 5))) != 0
}

fn ctrl_active(modifiers: u8) -> bool {
    (modifiers & ((1 << 0) | (1 << 4))) != 0
}

fn apply_arrow_modifiers(base: KeyEvent, modifiers: u8) -> KeyEvent {
    let shift = shift_active(modifiers);
    let ctrl = ctrl_active(modifiers);
    match base {
        KeyEvent::ArrowUp if shift && ctrl => KeyEvent::CtrlShiftArrowUp,
        KeyEvent::ArrowDown if shift && ctrl => KeyEvent::CtrlShiftArrowDown,
        KeyEvent::ArrowLeft if shift && ctrl => KeyEvent::CtrlShiftArrowLeft,
        KeyEvent::ArrowRight if shift && ctrl => KeyEvent::CtrlShiftArrowRight,
        KeyEvent::ArrowUp if ctrl => KeyEvent::CtrlArrowUp,
        KeyEvent::ArrowDown if ctrl => KeyEvent::CtrlArrowDown,
        KeyEvent::ArrowLeft if ctrl => KeyEvent::CtrlArrowLeft,
        KeyEvent::ArrowRight if ctrl => KeyEvent::CtrlArrowRight,
        KeyEvent::ArrowUp if shift => KeyEvent::ShiftArrowUp,
        KeyEvent::ArrowDown if shift => KeyEvent::ShiftArrowDown,
        KeyEvent::ArrowLeft if shift => KeyEvent::ShiftArrowLeft,
        KeyEvent::ArrowRight if shift => KeyEvent::ShiftArrowRight,
        _ => base,
    }
}

fn usage_to_key_event(usage: u8, modifiers: u8) -> Option<KeyEvent> {
    let shift = shift_active(modifiers);
    let ctrl = ctrl_active(modifiers);

    if ctrl {
        return match usage {
            0x04 => Some(KeyEvent::CtrlA),
            0x06 => Some(KeyEvent::CtrlC),
            0x07 => Some(KeyEvent::CtrlD),
            0x08 => Some(KeyEvent::CtrlE),
            0x0E => Some(KeyEvent::CtrlK),
            0x0F => Some(KeyEvent::CtrlL),
            0x18 => Some(KeyEvent::CtrlU),
            0x1A => Some(KeyEvent::CtrlW),
            _ => None,
        };
    }

    match usage {
        0x04..=0x1D => {
            let base = b'a' + (usage - 0x04);
            let ch = if shift { (base as char).to_ascii_uppercase() } else { base as char };
            Some(KeyEvent::Character(ch))
        }
        0x1E => Some(KeyEvent::Character(if shift { '!' } else { '1' })),
        0x1F => Some(KeyEvent::Character(if shift { '@' } else { '2' })),
        0x20 => Some(KeyEvent::Character(if shift { '#' } else { '3' })),
        0x21 => Some(KeyEvent::Character(if shift { '$' } else { '4' })),
        0x22 => Some(KeyEvent::Character(if shift { '%' } else { '5' })),
        0x23 => Some(KeyEvent::Character(if shift { '^' } else { '6' })),
        0x24 => Some(KeyEvent::Character(if shift { '&' } else { '7' })),
        0x25 => Some(KeyEvent::Character(if shift { '*' } else { '8' })),
        0x26 => Some(KeyEvent::Character(if shift { '(' } else { '9' })),
        0x27 => Some(KeyEvent::Character(if shift { ')' } else { '0' })),
        0x28 => Some(KeyEvent::Enter),
        0x29 => Some(KeyEvent::Escape),
        0x2A => Some(KeyEvent::Backspace),
        0x2B => Some(KeyEvent::Tab),
        0x2C => Some(KeyEvent::Character(' ')),
        0x2D => Some(KeyEvent::Character(if shift { '_' } else { '-' })),
        0x2E => Some(KeyEvent::Character(if shift { '+' } else { '=' })),
        0x2F => Some(KeyEvent::Character(if shift { '{' } else { '[' })),
        0x30 => Some(KeyEvent::Character(if shift { '}' } else { ']' })),
        0x31 => Some(KeyEvent::Character(if shift { '|' } else { '\\' })),
        0x33 => Some(KeyEvent::Character(if shift { ':' } else { ';' })),
        0x34 => Some(KeyEvent::Character(if shift { '"' } else { '\'' })),
        0x35 => Some(KeyEvent::Character(if shift { '~' } else { '`' })),
        0x36 => Some(KeyEvent::Character(if shift { '<' } else { ',' })),
        0x37 => Some(KeyEvent::Character(if shift { '>' } else { '.' })),
        0x38 => Some(KeyEvent::Character(if shift { '?' } else { '/' })),
        0x3A..=0x45 => Some(KeyEvent::FKey(usage - 0x39)),
        0x49 => Some(KeyEvent::Insert),
        0x4A => Some(KeyEvent::Home),
        0x4B => Some(KeyEvent::PageUp),
        0x4C => Some(KeyEvent::Delete),
        0x4D => Some(KeyEvent::End),
        0x4E => Some(KeyEvent::PageDown),
        0x4F => Some(apply_arrow_modifiers(KeyEvent::ArrowRight, modifiers)),
        0x50 => Some(apply_arrow_modifiers(KeyEvent::ArrowLeft, modifiers)),
        0x51 => Some(apply_arrow_modifiers(KeyEvent::ArrowDown, modifiers)),
        0x52 => Some(apply_arrow_modifiers(KeyEvent::ArrowUp, modifiers)),
        _ => None,
    }
}

fn enqueue_key(event: KeyEvent) {
    queue_lock(&KEY_QUEUE_LOCK);
    unsafe {
        let queue = &mut *KEY_QUEUE.get();
        let _ = queue.push_back(event);
    }
    queue_unlock(&KEY_QUEUE_LOCK);
}

fn enqueue_mouse(event: MouseEvent) {
    queue_lock(&MOUSE_QUEUE_LOCK);
    unsafe {
        let queue = &mut *MOUSE_QUEUE.get();
        let _ = queue.push_back(event);
    }
    queue_unlock(&MOUSE_QUEUE_LOCK);
}

pub fn poll_key_event() -> Option<KeyEvent> {
    queue_lock(&KEY_QUEUE_LOCK);
    let event = unsafe { (&mut *KEY_QUEUE.get()).pop_front() };
    queue_unlock(&KEY_QUEUE_LOCK);
    event
}

pub fn poll_mouse_event() -> Option<MouseEvent> {
    queue_lock(&MOUSE_QUEUE_LOCK);
    let event = unsafe { (&mut *MOUSE_QUEUE.get()).pop_front() };
    queue_unlock(&MOUSE_QUEUE_LOCK);
    event
}

pub fn feed_boot_keyboard_report(report: &[u8]) -> Result<(), &'static str> {
    if report.len() < 8 {
        return Err("usb: keyboard report too short");
    }

    let modifiers = report[0];
    let mut current = [0u8; 6];
    current.copy_from_slice(&report[2..8]);

    // SAFETY: early kernel single-core input path.
    let state = unsafe { &mut *HID_KEYBOARD_STATE.get() };
    for &usage in &current {
        if usage == 0 {
            continue;
        }
        if state.pressed.contains(&usage) {
            continue;
        }
        if let Some(event) = usage_to_key_event(usage, modifiers) {
            enqueue_key(event);
        }
    }

    state.modifiers = modifiers;
    state.pressed = current;
    Ok(())
}

pub fn feed_boot_mouse_report(report: &[u8]) -> Result<(), &'static str> {
    if report.len() < 3 {
        return Err("usb: mouse report too short");
    }

    let buttons = MouseButtons {
        left: (report[0] & 0x01) != 0,
        right: (report[0] & 0x02) != 0,
        middle: (report[0] & 0x04) != 0,
    };
    let dx = i16::from(report[1] as i8);
    let dy = -i16::from(report[2] as i8);
    if dx != 0 || dy != 0 {
        enqueue_mouse(MouseEvent::Move { dx, dy, buttons });
    }
    if report.len() >= 4 {
        let delta = report[3] as i8;
        if delta != 0 {
            enqueue_mouse(MouseEvent::Wheel { delta, buttons });
        }
    }
    Ok(())
}