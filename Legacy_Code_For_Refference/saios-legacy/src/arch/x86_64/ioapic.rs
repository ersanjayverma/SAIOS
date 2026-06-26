//! I/O APIC driver — F-INT-02 fix.
//!
//! Detects IOAPIC via ACPI MADT, programs redirection table entries for ISA
//! IRQs, and provides an interrupt routing abstraction over legacy PIC.
//! Falls back to PIC if no IOAPIC is found.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// IOAPIC base address (MMIO).
static IOAPIC_BASE: AtomicU64 = AtomicU64::new(0);
/// GSI (Global System Interrupt) base for this IOAPIC.
static IOAPIC_GSI_BASE: AtomicU64 = AtomicU64::new(0);
/// Maximum number of redirection entries (0-based).
static IOAPIC_MAX_REDIR: AtomicU64 = AtomicU64::new(0);
/// Whether IOAPIC is active (PIC disabled).
static IOAPIC_ACTIVE: AtomicBool = AtomicBool::new(false);

/// IDT vector offset for hardware IRQs (must match PIC offset in interrupt/mod.rs).
const IRQ_VECTOR_BASE: u8 = 0x20;

// IOAPIC register indices
const IOAPIC_REG_ID: u32 = 0x00;
const IOAPIC_REG_VER: u32 = 0x01;
const IOAPIC_REG_REDTBL_BASE: u32 = 0x10;

// Redirection entry flags
const REDIR_DELIVERY_FIXED: u64 = 0;
const REDIR_DESTMODE_PHYSICAL: u64 = 0;
const REDIR_POLARITY_HIGH: u64 = 0;
const REDIR_TRIGGER_EDGE: u64 = 0;
const REDIR_MASKED: u64 = 1 << 16;

/// Read a 32-bit IOAPIC register.
unsafe fn ioapic_read(base: u64, reg: u32) -> u32 {
    let regsel = base as *mut u32;
    let window = (base + 0x10) as *mut u32;
    unsafe {
        core::ptr::write_volatile(regsel, reg);
        core::ptr::read_volatile(window)
    }
}

/// Write a 32-bit IOAPIC register.
unsafe fn ioapic_write(base: u64, reg: u32, value: u32) {
    let regsel = base as *mut u32;
    let window = (base + 0x10) as *mut u32;
    unsafe {
        core::ptr::write_volatile(regsel, reg);
        core::ptr::write_volatile(window, value);
    }
}

/// Write a 64-bit redirection table entry (low + high dwords).
unsafe fn write_redir(base: u64, irq: u8, entry: u64) {
    let reg_low = IOAPIC_REG_REDTBL_BASE + (irq as u32) * 2;
    let reg_high = reg_low + 1;
    unsafe {
        ioapic_write(base, reg_low, entry as u32);
        ioapic_write(base, reg_high, (entry >> 32) as u32);
    }
}

/// Parse MADT for IOAPIC entries (type 1).
/// Returns (base_address, gsi_base) if found.
pub fn detect_from_madt() -> Option<(u64, u32)> {
    let madt = crate::driver::acpi::find_table(b"APIC")?;
    let len = crate::driver::acpi::read_u32(madt + 4)? as u64;
    let end = madt + len;
    let mut p = madt + 44; // entries start at offset 44

    while p + 2 <= end {
        let etype = crate::driver::acpi::read_u8(p).unwrap_or(0xFF);
        let elen = crate::driver::acpi::read_u8(p + 1).unwrap_or(0) as u64;
        if elen < 2 {
            break;
        }
        // MADT entry type 1: I/O APIC
        if etype == 1 && elen >= 12 {
            let ioapic_addr = crate::driver::acpi::read_u32(p + 4).unwrap_or(0) as u64;
            let gsi_base = crate::driver::acpi::read_u32(p + 8).unwrap_or(0);
            if ioapic_addr != 0 {
                return Some((ioapic_addr, gsi_base));
            }
        }
        p += elen;
    }
    None
}

/// Initialize the IOAPIC: detect, read version, program ISA IRQ routing.
/// Falls back to legacy PIC if no IOAPIC is found.
pub fn init() {
    let Some((base, gsi_base)) = detect_from_madt() else {
        crate::serial_println!("[ioapic] not found in MADT — using legacy PIC");
        return;
    };

    // Identity-map the IOAPIC MMIO page (already mapped if in first 4GB by
    // the bootloader's identity mapping, which SAIOS preserves for MMIO).
    IOAPIC_BASE.store(base, Ordering::Release);
    IOAPIC_GSI_BASE.store(gsi_base as u64, Ordering::Release);

    let (id, max_redir) = unsafe {
        let ver = ioapic_read(base, IOAPIC_REG_VER);
        let id = ioapic_read(base, IOAPIC_REG_ID) >> 24;
        let max_redir = ((ver >> 16) & 0xFF) as u8;
        (id, max_redir)
    };

    IOAPIC_MAX_REDIR.store(max_redir as u64, Ordering::Release);

    crate::serial_println!(
        "[ioapic] detected base={:#x} gsi_base={} id={} max_redir={}",
        base,
        gsi_base,
        id,
        max_redir
    );

    // Program ISA IRQs (0-15) to fixed delivery, edge-triggered, active-high,
    // targeting BSP (LAPIC ID 0 physical destination).
    for irq in 0..=15u8.min(max_redir) {
        let vector = IRQ_VECTOR_BASE + irq;
        // Start masked; unmask individually as drivers register.
        let entry: u64 = (vector as u64)
            | REDIR_DELIVERY_FIXED
            | REDIR_DESTMODE_PHYSICAL
            | REDIR_POLARITY_HIGH
            | REDIR_TRIGGER_EDGE
            | REDIR_MASKED;
        unsafe { write_redir(base, irq, entry) };
    }

    // Disable legacy 8259 PIC by masking all IRQs (IOAPIC takes over).
    // Must be done BEFORE unmasking IOAPIC entries and marking active, so there
    // is no window where both PIC and IOAPIC deliver the same IRQ.
    disable_legacy_pic();

    // Mark IOAPIC active BEFORE unmasking — the eoi() path checks this flag to
    // decide whether to write LAPIC EOI or PIC EOI.  If an IRQ fires immediately
    // after unmask, the handler must already know to use LAPIC EOI.
    IOAPIC_ACTIVE.store(true, Ordering::Release);

    // Unmask only ISA IRQs with installed IDT handlers. IRQ2 is a legacy PIC
    // cascade line, not an IOAPIC device interrupt in this routing mode.
    unmask_irq(0); // PIT timer
    unmask_irq(1); // Keyboard
    unmask_irq(12); // PS/2 Mouse

    crate::serial_println!("[ioapic] active — legacy PIC disabled");
}

/// Unmask a specific IRQ in the IOAPIC redirection table.
pub fn unmask_irq(irq: u8) {
    let base = IOAPIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    let max = IOAPIC_MAX_REDIR.load(Ordering::Acquire) as u8;
    if irq > max {
        return;
    }
    unsafe {
        let reg_low = IOAPIC_REG_REDTBL_BASE + (irq as u32) * 2;
        let mut low = ioapic_read(base, reg_low);
        low &= !(1 << 16); // clear mask bit
        ioapic_write(base, reg_low, low);
    }
}

/// Mask a specific IRQ in the IOAPIC redirection table.
pub fn mask_irq(irq: u8) {
    let base = IOAPIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    let max = IOAPIC_MAX_REDIR.load(Ordering::Acquire) as u8;
    if irq > max {
        return;
    }
    unsafe {
        let reg_low = IOAPIC_REG_REDTBL_BASE + (irq as u32) * 2;
        let mut low = ioapic_read(base, reg_low);
        low |= 1 << 16; // set mask bit
        ioapic_write(base, reg_low, low);
    }
}

/// Disable the legacy 8259 PIC by masking all IRQ lines.
fn disable_legacy_pic() {
    unsafe {
        // Mask all IRQs on both PICs.
        crate::arch::port_write_u8(0x21, 0xFF); // PIC1 data
        crate::arch::port_write_u8(0xA1, 0xFF); // PIC2 data
    }
}

/// Returns true if IOAPIC is the active interrupt controller.
pub fn is_active() -> bool {
    IOAPIC_ACTIVE.load(Ordering::Acquire)
}

/// Route an IRQ to a specific LAPIC (for SMP IRQ distribution).
pub fn route_irq_to_cpu(irq: u8, lapic_id: u8) {
    let base = IOAPIC_BASE.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    let max = IOAPIC_MAX_REDIR.load(Ordering::Acquire) as u8;
    if irq > max {
        return;
    }
    unsafe {
        let reg_high = IOAPIC_REG_REDTBL_BASE + (irq as u32) * 2 + 1;
        let mut high = ioapic_read(base, reg_high);
        high = (high & 0x00FF_FFFF) | ((lapic_id as u32) << 24);
        ioapic_write(base, reg_high, high);
    }
}
