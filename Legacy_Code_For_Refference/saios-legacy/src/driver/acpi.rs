//! ACPI (Advanced Configuration and Power Interface) driver.
//!
//! Provides system shutdown (S5) and reboot via ACPI reset register.
//! The driver is written defensively — every pointer read is bounds-checked
//! against the identity-mapped region (0–1 GiB) before dereference.
//!
//! # ACPI table chain
//!   RSDP → RSDT → FADT (signature "FACP") → PM1a_CNT_BLK port
//!
//! # FADT offsets used (ACPI 1.0 / 2.0 compatible)
//!   +36  FIRMWARE_CTRL  (u32)
//!   +40  DSDT            (u32)  ← physical address of DSDT
//!   +56  PM1a_EVT_BLK   (u32)
//!   +60  PM1b_EVT_BLK   (u32)
//!   +64  PM1a_CNT_BLK   (u32)  ← I/O port we write SLP command to
//!   +68  PM1b_CNT_BLK   (u32)

static PM1A_CNT: spin::Mutex<u16> = spin::Mutex::new(0);
static PM1B_CNT: spin::Mutex<u16> = spin::Mutex::new(0);
static SLP_TYP_S5: spin::Mutex<u16> = spin::Mutex::new(0);
static SMI_CMD: spin::Mutex<u16> = spin::Mutex::new(0);
static ACPI_ENABLE: spin::Mutex<u8> = spin::Mutex::new(0);

/// Maximum physical address we'll safely dereference.
/// Must match the identity-map size set up in boot.s (currently 128 GiB).
const PHYS_MAX: u64 = 128 * 1024 * 1024 * 1024;

/// Return true if `addr`..`addr+len` is fully within our identity-mapped window.
fn phys_ok(addr: u64, len: usize) -> bool {
    addr >= 0x1000 && addr.saturating_add(len as u64) < PHYS_MAX
}

/// Safe read of a u32 from a physical (= virtual in identity map) address.
/// Returns None if the address is outside our mapped window.
fn safe_read_u32(addr: u64) -> Option<u32> {
    if !phys_ok(addr, 4) {
        return None;
    }
    Some(unsafe { core::ptr::read_unaligned(addr as *const u32) })
}

/// Safe read of a u16.
fn safe_read_u16(addr: u64) -> Option<u16> {
    if !phys_ok(addr, 2) {
        return None;
    }
    Some(unsafe { core::ptr::read_unaligned(addr as *const u16) })
}

pub fn init() {
    match find_acpi_shutdown() {
        Some(a) => {
            *PM1A_CNT.lock() = a.pm1a;
            *PM1B_CNT.lock() = a.pm1b;
            *SLP_TYP_S5.lock() = a.slp;
            *SMI_CMD.lock() = a.smi_cmd;
            *ACPI_ENABLE.lock() = a.acpi_enable;
            crate::println!(
                "[acpi] S5: pm1a={:#x} pm1b={:#x} SLP_TYP={:#x} smi={:#x} en={:#x}",
                a.pm1a,
                a.pm1b,
                a.slp,
                a.smi_cmd,
                a.acpi_enable
            );
        }
        None => {
            crate::println!("[acpi] RSDP/FADT not found — using port fallbacks for shutdown");
        }
    }
}

/// Shut down the machine via ACPI S5 (soft-off).
pub fn shutdown() -> ! {
    let pm1a = *PM1A_CNT.lock();
    let pm1b = *PM1B_CNT.lock();
    let slp = *SLP_TYP_S5.lock();
    let smi = *SMI_CMD.lock();
    let en = *ACPI_ENABLE.lock();

    unsafe {
        // Enable ACPI mode first.  Until the OS writes ACPI_ENABLE to SMI_CMD
        // and SCI_EN (bit 0 of PM1a_CNT) is set, the SLP_EN write below is
        // ignored by the chipset — this is why halt did nothing on VirtualBox.
        if smi != 0 && en != 0 && pm1a != 0 && (crate::arch::port_read_u16(pm1a) & 1) == 0 {
            crate::arch::port_write_u8(smi, en);
            for _ in 0..1_000_000u64 {
                if crate::arch::port_read_u16(pm1a) & 1 != 0 {
                    break;
                }
                crate::arch::nop();
            }
        }

        // Write SLP_TYP | SLP_EN (bit 13) to PM1a (and PM1b if present).
        if pm1a != 0 {
            crate::arch::port_write_u16(pm1a, slp | (1 << 13));
            for _ in 0..1_000_000u64 {
                crate::arch::nop();
            }
        }
        if pm1b != 0 {
            crate::arch::port_write_u16(pm1b, slp | (1 << 13));
            for _ in 0..1_000_000u64 {
                crate::arch::nop();
            }
        }

        // Emulator fallbacks (used if ACPI parsing/enable failed).  These are
        // the documented power-off ports: VirtualBox 0x4004←0x3400 (SLP_TYP=5),
        // QEMU 0x604←0x2000, Bochs/old-QEMU 0xB004←0x2000.
        crate::arch::port_write_u16(0x4004, 0x3400);
        crate::arch::port_write_u16(0x0604, 0x2000);
        crate::arch::port_write_u16(0xB004, 0x2000);
    }

    loop {
        crate::arch::halt();
    }
}

/// Reboot via ACPI RESET_REG (port 0xCF9) or keyboard controller.
pub fn reboot() -> ! {
    unsafe {
        // ACPI warm reset via port 0xCF9 (bit 2 = FULL_RST, bit 1 = SYS_RST)
        crate::arch::port_write_u8(0xCF9, 0x06);
        for _ in 0..1_000_000u64 {
            crate::arch::nop();
        }
        // Keyboard controller reset pulse (bit 0 of port 0x64 output)
        loop {
            if crate::arch::port_read_u8(0x64) & 0x02 == 0 {
                break;
            }
        }
        crate::arch::port_write_u8(0x64, 0xFE);
    }
    loop {
        crate::arch::halt();
    }
}

/// Find any ACPI table by its 4-byte signature (e.g. b"APIC" for the MADT).
/// Returns the physical (= identity-mapped virtual) address of the table.
pub fn find_table(sig: &[u8; 4]) -> Option<u64> {
    let rsdp = find_rsdp()?;
    let rsdt = safe_read_u32(rsdp + 16)? as u64;
    if !phys_ok(rsdt, 36) {
        return None;
    }
    let rsdt_len = safe_read_u32(rsdt + 4)? as u64;
    if rsdt_len < 36 {
        return None;
    }
    let entries = (rsdt_len - 36) / 4;
    for i in 0..entries.min(64) {
        let table_phys = safe_read_u32(rsdt + 36 + i * 4)? as u64;
        if !phys_ok(table_phys, 8) {
            continue;
        }
        let s = unsafe { core::slice::from_raw_parts(table_phys as *const u8, 4) };
        if s == sig {
            return Some(table_phys);
        }
    }
    None
}

/// Safe read of a u8 from an identity-mapped physical address.
pub fn read_u8(addr: u64) -> Option<u8> {
    if !phys_ok(addr, 1) {
        return None;
    }
    Some(unsafe { core::ptr::read_volatile(addr as *const u8) })
}

/// Safe read of a u32 from a packed ACPI field.
pub fn read_u16(addr: u64) -> Option<u16> {
    safe_read_u16(addr)
}

/// Safe read of a u32 (public wrapper for the MADT walker).
pub fn read_u32(addr: u64) -> Option<u32> {
    safe_read_u32(addr)
}

// -- ACPI table parsing ----------------------------------------------------

struct AcpiShutdown {
    pm1a: u16,
    pm1b: u16,
    slp: u16,
    smi_cmd: u16,
    acpi_enable: u8,
}

/// Walk RSDP → RSDT → FADT to find the shutdown registers and the S5 SLP_TYP.
fn find_acpi_shutdown() -> Option<AcpiShutdown> {
    let rsdp = find_rsdp()?;

    // RSDP +16: RSDT physical address (u32)
    let rsdt = safe_read_u32(rsdp + 16)? as u64;
    if !phys_ok(rsdt, 36) {
        return None;
    }

    // RSDT header: +4 length, entries start at +36
    let rsdt_len = safe_read_u32(rsdt + 4)? as u64;
    if rsdt_len < 36 {
        return None;
    }

    let entries = (rsdt_len - 36) / 4;
    let mut fadt: u64 = 0;

    for i in 0..entries.min(64) {
        let entry_addr = rsdt + 36 + i * 4;
        let table_phys = safe_read_u32(entry_addr)? as u64;
        if !phys_ok(table_phys, 8) {
            continue;
        }
        // Check signature "FACP"
        let sig = unsafe { core::slice::from_raw_parts(table_phys as *const u8, 4) };
        if sig == b"FACP" {
            fadt = table_phys;
            break;
        }
    }

    if fadt == 0 || !phys_ok(fadt, 72) {
        return None;
    }

    // FADT fields (ACPI 1.0/2.0 legacy 32-bit block addresses).
    let smi_cmd = safe_read_u32(fadt + 48)? as u16; // +48 SMI_CMD
    let acpi_enable = (safe_read_u32(fadt + 52)? & 0xFF) as u8; // +52 ACPI_ENABLE
    let pm1a_port = safe_read_u32(fadt + 64)? as u16; // +64 PM1a_CNT_BLK
    let pm1b_port = safe_read_u32(fadt + 68)? as u16; // +68 PM1b_CNT_BLK
    if pm1a_port == 0 {
        return None;
    }

    // FADT +40: DSDT physical address (u32)
    let dsdt = safe_read_u32(fadt + 40)? as u64;

    // Parse DSDT AML to find the \_S5 SLP_TYPa value (5 is the usual default).
    let slp_typ = if phys_ok(dsdt, 128) {
        parse_s5_from_dsdt(dsdt).unwrap_or(5 << 10)
    } else {
        5 << 10
    };

    Some(AcpiShutdown {
        pm1a: pm1a_port,
        pm1b: pm1b_port,
        slp: slp_typ,
        smi_cmd,
        acpi_enable,
    })
}

/// Search for \_S5 AML name in the DSDT and extract the SLP_TYP value.
/// Returns the value pre-shifted into the SLP_TYP field (bits 12:10).
fn parse_s5_from_dsdt(dsdt: u64) -> Option<u16> {
    let length = safe_read_u32(dsdt + 4)? as u64;
    if length < 36 || !phys_ok(dsdt, length as usize) {
        return None;
    }

    let aml_start = dsdt + 36;
    let aml_len = (length - 36) as usize;
    let aml = unsafe { core::slice::from_raw_parts(aml_start as *const u8, aml_len) };

    // Find "_S5_" followed by a PackageOp, then decode the first element
    // (SLP_TYPa).  AML layout:  _S5_ 12 <PkgLength> <NumElements> <element0> ...
    // PkgLength is 1 + ((byte0 & 0xC0) >> 6) bytes; element0 may be a BytePrefix
    // (0x0A v), ZeroOp (0x00 = 0), OneOp (0x01 = 1), or a bare small integer.
    for i in 0..aml_len.saturating_sub(8) {
        if &aml[i..i + 4] == b"_S5_" && aml[i + 4] == 0x12 {
            let lead = i + 5; // first PkgLength byte
            let pkglen_bytes = 1 + ((aml[lead] & 0xC0) >> 6) as usize;
            let mut p = lead + pkglen_bytes + 1; // skip PkgLength + NumElements
            if p >= aml_len {
                return None;
            }
            let v = match aml[p] {
                0x0A => {
                    p += 1;
                    if p < aml_len { aml[p] } else { 5 }
                }
                0x00 => 0,
                0x01 => 1,
                b => b,
            };
            return Some((v as u16) << 10); // into PM1_CNT SLP_TYP field
        }
    }
    None // not found — caller uses default
}

/// Scan low memory for the RSDP signature "RSD PTR ".
/// Returns the physical address of the RSDP, or None.
fn find_rsdp() -> Option<u64> {
    // 1. First 1 KiB of EBDA (address from BDA at 0x40E)
    if phys_ok(0x40E, 2) {
        let ebda_seg = unsafe { core::ptr::read_unaligned(0x40Eu64 as *const u16) } as u64;
        let ebda = ebda_seg << 4;
        if phys_ok(ebda, 1024)
            && let Some(p) = scan_for_rsdp(ebda, 1024)
        {
            return Some(p);
        }
    }
    // 2. BIOS read-only area 0xE0000–0xFFFFF
    scan_for_rsdp(0xE_0000, 0x2_0000)
}

const RSDP_SIG: &[u8; 8] = b"RSD PTR ";

/// Linear scan for the RSDP signature in [start, start+len).
fn scan_for_rsdp(start: u64, len: usize) -> Option<u64> {
    let mut addr = start;
    let end = start + len as u64;
    while addr + 20 < end {
        if phys_ok(addr, 8) {
            let sig = unsafe { core::slice::from_raw_parts(addr as *const u8, 8) };
            if sig == RSDP_SIG {
                // Verify checksum over first 20 bytes
                if phys_ok(addr, 20) {
                    let sum: u8 = unsafe {
                        let base = addr as *const u8;
                        (0..20usize).fold(0u8, |acc, i| acc.wrapping_add(*base.add(i)))
                    };
                    if sum == 0 {
                        return Some(addr);
                    }
                }
            }
        }
        addr += 16; // RSDP is 16-byte aligned
    }
    None
}
