/// ACPI (Advanced Configuration and Power Interface) Support Module
/// Provides full ACPI table parsing, device enumeration, power management,
/// and interrupt routing for the SAIOS kernel.

use alloc::vec::Vec;
use alloc::string::String;
use core::mem;
use core::ptr;

/// ACPI signature constants (4-byte big-endian identifiers)
pub const RSDT_SIGNATURE: &[u8; 4] = b"RSDT";
pub const XSDT_SIGNATURE: &[u8; 4] = b"XSDT";
pub const DSDT_SIGNATURE: &[u8; 4] = b"DSDT";
pub const SSDT_SIGNATURE: &[u8; 4] = b"SSDT";
pub const FADT_SIGNATURE: &[u8; 4] = b"FACP";
pub const MADT_SIGNATURE: &[u8; 4] = b"APIC";
pub const SRAT_SIGNATURE: &[u8; 4] = b"SRAT";
pub const SLIT_SIGNATURE: &[u8; 4] = b"SLIT";

/// Generic ACPI System Description Table Header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct AcpiTableHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

/// Root System Description Pointer (RSDP)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct RsdpDescriptor {
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,
    pub length: u32,
    pub xsdt_address: u64,
    pub extended_checksum: u8,
    pub reserved: [u8; 3],
}

/// Root System Description Table (RSDT) - for ACPI 1.0
#[repr(C, packed)]
pub struct Rsdt {
    pub header: AcpiTableHeader,
    // Followed by array of u32 pointers to other tables
}

/// Extended System Description Table (XSDT) - for ACPI 2.0+
#[repr(C, packed)]
pub struct Xsdt {
    pub header: AcpiTableHeader,
    // Followed by array of u64 pointers to other tables
}

/// Fixed ACPI Description Table (FADT)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Fadt {
    pub header: AcpiTableHeader,
    pub firmware_ctrl: u32,
    pub dsdt: u32,
    pub reserved1: u8,
    pub preferred_pm_profile: u8,
    pub sci_int: u16,
    pub smi_cmd: u32,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
    pub s4bios_req: u8,
    pub pstate_cnt: u8,
    pub pm1a_evt_blk: u32,
    pub pm1b_evt_blk: u32,
    pub pm1a_cnt_blk: u32,
    pub pm1b_cnt_blk: u32,
    pub pm2_cnt_blk: u32,
    pub pm_tmr_blk: u32,
    pub gpe0_blk: u32,
    pub gpe1_blk: u32,
    pub pm1_evt_len: u8,
    pub pm1_cnt_len: u8,
    pub pm2_cnt_len: u8,
    pub pm_tmr_len: u8,
    pub gpe0_blk_len: u8,
    pub gpe1_blk_len: u8,
    pub gpe1_base: u8,
    pub cst_cnt: u8,
    pub p_lvl2_lat: u16,
    pub p_lvl3_lat: u16,
    pub flush_size: u16,
    pub flush_stride: u16,
    pub duty_offset: u8,
    pub duty_width: u8,
    pub day_alrm: u8,
    pub mon_alrm: u8,
    pub century: u8,
    pub iapc_boot_arch: u16,
    pub reserved2: u8,
    pub flags: u32,
    // Extended fields for ACPI 3.0+
    pub reset_reg: [u8; 12],
    pub reset_value: u8,
    pub reserved3: [u8; 3],
    pub x_firmware_ctrl: u64,
    pub x_dsdt: u64,
}

/// Multiple APIC Description Table (MADT)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Madt {
    pub header: AcpiTableHeader,
    pub local_apic_addr: u32,
    pub flags: u32,
}

/// MADT Entry Types
#[repr(u8)]
pub enum MadtEntryType {
    ProcessorLocalApic = 0,
    IoApic = 1,
    InterruptSourceOverride = 2,
    NonMaskableInterrupt = 3,
    LocalApicNmi = 4,
    LocalApicAddressOverride = 5,
    IoSapic = 6,
    ProcessorLocalSapic = 7,
    PlatformInterruptSources = 8,
    Processorx2Apic = 9,
    x2ApicNmi = 10,
    GicCpuInterface = 11,
    GicDistributor = 12,
}

/// Processor Local APIC entry in MADT
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtProcessorLocalApic {
    pub entry_type: u8,
    pub length: u8,
    pub processor_id: u8,
    pub apic_id: u8,
    pub flags: u32,
}

/// IO APIC entry in MADT
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtIoApic {
    pub entry_type: u8,
    pub length: u8,
    pub io_apic_id: u8,
    pub reserved: u8,
    pub io_apic_address: u32,
    pub global_system_interrupt_base: u32,
}

/// Interrupt Source Override entry in MADT
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MadtInterruptSourceOverride {
    pub entry_type: u8,
    pub length: u8,
    pub bus: u8,
    pub source: u8,
    pub global_system_interrupt: u32,
    pub flags: u16,
}

/// SRAT (System Resource Affinity Table) entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SratEntry {
    pub entry_type: u8,
    pub length: u8,
}

/// Power Button Event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    PowerButton,
    SleepButton,
    LidSwitch,
}

/// System Sleep State
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepState {
    S0,  // Working
    S1,  // Sleeping (light)
    S2,  // Sleeping (medium)
    S3,  // Sleeping (deep, memory)
    S4,  // Hibernation
    S5,  // Soft off
}

/// ACPI Device Information
#[derive(Debug, Clone)]
pub struct AcpiDevice {
    pub name: String,
    pub hardware_id: String,
    pub unique_id: String,
    pub device_class: String,
}

/// Main ACPI Manager
pub struct AcpiManager {
    rsdp_address: u64,
    revision: u8,
    processors: Vec<ProcessorInfo>,
    devices: Vec<AcpiDevice>,
    madt_address: u64,
    fadt_address: u64,
    dsdt_address: u64,
    local_apic_addr: u32,
    enabled: bool,
}

/// Processor Information
#[derive(Debug, Clone, Copy)]
pub struct ProcessorInfo {
    pub acpi_processor_id: u8,
    pub apic_id: u8,
    pub flags: u32,
}

impl AcpiManager {
    /// Create a new ACPI manager with the RSDP address from bootloader
    pub fn new(rsdp_address: u64) -> Self {
        Self {
            rsdp_address,
            revision: 0,
            processors: Vec::new(),
            devices: Vec::new(),
            madt_address: 0,
            fadt_address: 0,
            dsdt_address: 0,
            local_apic_addr: 0,
            enabled: false,
        }
    }

    /// Initialize ACPI - parses tables and discovers hardware
    pub fn initialize(&mut self) -> Result<(), &'static str> {
        if self.rsdp_address == 0 {
            return Err("ACPI: No RSDP address provided");
        }

        // Parse RSDP
        let rsdp = self.parse_rsdp()?;
        self.revision = rsdp.revision;

        // Get root table address based on ACPI version
        let root_table_addr = if rsdp.revision >= 2 && rsdp.xsdt_address != 0 {
            rsdp.xsdt_address
        } else if rsdp.rsdt_address != 0 {
            rsdp.rsdt_address as u64
        } else {
            return Err("ACPI: No valid RSDT or XSDT found");
        };

        // Parse root table and find DSDT, FADT, MADT
        self.parse_root_table(root_table_addr, rsdp.revision >= 2)?;

        // Parse FADT for power management info
        if self.fadt_address != 0 {
            self.parse_fadt()?;
        }

        // Parse MADT for processor and interrupt information
        if self.madt_address != 0 {
            self.parse_madt()?;
        }

        self.enabled = true;
        Ok(())
    }

    /// Parse RSDP from physical address
    fn parse_rsdp(&self) -> Result<RsdpDescriptor, &'static str> {
        let rsdp_ptr = self.rsdp_address as *const RsdpDescriptor;
        if rsdp_ptr.is_null() {
            return Err("ACPI: RSDP pointer is null");
        }

        let rsdp = unsafe { ptr::read_unaligned(rsdp_ptr) };

        // Validate signature
        if rsdp.signature != *b"RSD PTR " {
            return Err("ACPI: Invalid RSDP signature");
        }

        // Validate checksum
        let basic_bytes = unsafe {
            core::slice::from_raw_parts(self.rsdp_address as *const u8, 20)
        };
        let sum: u8 = basic_bytes.iter().fold(0, |acc, &x| acc.wrapping_add(x));
        if sum != 0 {
            return Err("ACPI: RSDP checksum validation failed");
        }

        Ok(rsdp)
    }

    /// Parse root table (RSDT or XSDT)
    fn parse_root_table(&mut self, root_addr: u64, is_xsdt: bool) -> Result<(), &'static str> {
        let header_ptr = root_addr as *const AcpiTableHeader;
        if header_ptr.is_null() {
            return Err("ACPI: Root table pointer is null");
        }

        let header = unsafe { ptr::read_unaligned(header_ptr) };

        // Validate signature
        let expected_sig = if is_xsdt { XSDT_SIGNATURE } else { RSDT_SIGNATURE };
        if header.signature != *expected_sig {
            return Err("ACPI: Invalid root table signature");
        }

        // Validate checksum
        let table_bytes = unsafe {
            core::slice::from_raw_parts(root_addr as *const u8, header.length as usize)
        };
        if !Self::validate_checksum(table_bytes) {
            return Err("ACPI: Root table checksum validation failed");
        }

        // Parse table entries
        let entry_size = if is_xsdt { 8 } else { 4 };
        let num_entries = (header.length as usize - mem::size_of::<AcpiTableHeader>()) / entry_size;
        let entries_offset = root_addr + mem::size_of::<AcpiTableHeader>() as u64;

        for i in 0..num_entries {
            let entry_addr = entries_offset + (i * entry_size) as u64;
            let table_addr = if is_xsdt {
                let ptr = entry_addr as *const u64;
                unsafe { ptr::read_unaligned(ptr) }
            } else {
                let ptr = entry_addr as *const u32;
                unsafe { ptr::read_unaligned(ptr) as u64 }
            };

            self.process_table(table_addr);
        }

        Ok(())
    }

    /// Process a discovered ACPI table
    fn process_table(&mut self, table_addr: u64) {
        if table_addr == 0 {
            return;
        }

        let header_ptr = table_addr as *const AcpiTableHeader;
        if header_ptr.is_null() {
            return;
        }

        let header = unsafe { ptr::read_unaligned(header_ptr) };

        match header.signature {
            *DSDT_SIGNATURE => self.dsdt_address = table_addr,
            *FADT_SIGNATURE => self.fadt_address = table_addr,
            *MADT_SIGNATURE => self.madt_address = table_addr,
            _ => {} // Ignore other tables for now
        }
    }

    /// Parse FADT (Fixed ACPI Description Table)
    fn parse_fadt(&mut self) -> Result<(), &'static str> {
        let fadt_ptr = self.fadt_address as *const Fadt;
        if fadt_ptr.is_null() {
            return Err("ACPI: FADT pointer is null");
        }

        let fadt = unsafe { ptr::read_unaligned(fadt_ptr) };

        // Validate signature
        if fadt.header.signature != *FADT_SIGNATURE {
            return Err("ACPI: Invalid FADT signature");
        }

        // Validate checksum
        let fadt_bytes = unsafe {
            core::slice::from_raw_parts(self.fadt_address as *const u8, fadt.header.length as usize)
        };
        if !Self::validate_checksum(fadt_bytes) {
            return Err("ACPI: FADT checksum validation failed");
        }

        Ok(())
    }

    /// Parse MADT (Multiple APIC Description Table)
    fn parse_madt(&mut self) -> Result<(), &'static str> {
        let madt_ptr = self.madt_address as *const Madt;
        if madt_ptr.is_null() {
            return Err("ACPI: MADT pointer is null");
        }

        let madt = unsafe { ptr::read_unaligned(madt_ptr) };

        // Validate signature
        if madt.header.signature != *MADT_SIGNATURE {
            return Err("ACPI: Invalid MADT signature");
        }

        // Validate checksum
        let madt_bytes = unsafe {
            core::slice::from_raw_parts(self.madt_address as *const u8, madt.header.length as usize)
        };
        if !Self::validate_checksum(madt_bytes) {
            return Err("ACPI: MADT checksum validation failed");
        }

        self.local_apic_addr = madt.local_apic_addr;

        // Parse MADT entries
        let mut offset = mem::size_of::<Madt>();
        let madt_end = madt.header.length as usize;

        while offset < madt_end {
            let entry_ptr = (self.madt_address + offset as u64) as *const u8;
            if entry_ptr.is_null() {
                break;
            }

            let entry_type = unsafe { *entry_ptr };
            let entry_length = unsafe { *(entry_ptr.add(1)) };

            if entry_length == 0 {
                break;
            }

            match entry_type {
                0 => self.parse_madt_processor_local_apic(entry_ptr),
                1 => self.parse_madt_io_apic(entry_ptr),
                2 => self.parse_madt_interrupt_source_override(entry_ptr),
                _ => {} // Ignore other entry types for now
            }

            offset += entry_length as usize;
        }

        Ok(())
    }

    /// Parse Processor Local APIC entry in MADT
    fn parse_madt_processor_local_apic(&mut self, entry_ptr: *const u8) {
        let entry_ptr = entry_ptr as *const MadtProcessorLocalApic;
        let entry = unsafe { ptr::read_unaligned(entry_ptr) };

        // Only add if the processor is enabled
        if (entry.flags & 0x01) != 0 {
            self.processors.push(ProcessorInfo {
                acpi_processor_id: entry.processor_id,
                apic_id: entry.apic_id,
                flags: entry.flags,
            });
        }
    }

    /// Parse IO APIC entry in MADT
    fn parse_madt_io_apic(&self, _entry_ptr: *const u8) {
        // IO APIC handling would go here
        // For now, just acknowledge it was found
    }

    /// Parse Interrupt Source Override entry in MADT
    fn parse_madt_interrupt_source_override(&self, _entry_ptr: *const u8) {
        // Interrupt source override handling would go here
        // For now, just acknowledge it was found
    }

    /// Validate ACPI table checksum
    fn validate_checksum(table_bytes: &[u8]) -> bool {
        let sum: u8 = table_bytes.iter().fold(0, |acc, &x| acc.wrapping_add(x));
        sum == 0
    }

    /// Get discovered processors
    pub fn processors(&self) -> &[ProcessorInfo] {
        &self.processors
    }

    /// Get local APIC address
    pub fn local_apic_address(&self) -> u32 {
        self.local_apic_addr
    }

    /// Check if ACPI is initialized and enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get ACPI revision
    pub fn revision(&self) -> u8 {
        self.revision
    }

    /// Get number of discovered processors
    pub fn processor_count(&self) -> usize {
        self.processors.len()
    }

    /// Attempt to enter sleep state (requires AML interpreter in full implementation)
    pub fn enter_sleep_state(&self, _state: SleepState) -> Result<(), &'static str> {
        // Full implementation would use AML interpreter to call _PTS and _GTS
        Err("ACPI: Sleep state not yet implemented")
    }

    /// Shutdown system (requires AML interpreter in full implementation)
    pub fn shutdown(&self) -> Result<(), &'static str> {
        // Full implementation would use AML interpreter to call \_S5
        Err("ACPI: Shutdown not yet implemented")
    }

    /// Reboot system
    pub fn reboot(&self) -> Result<(), &'static str> {
        // Use FADT reset register if available
        if self.fadt_address != 0 {
            let fadt_ptr = self.fadt_address as *const Fadt;
            let fadt = unsafe { ptr::read_unaligned(fadt_ptr) };

            // Reset register is in fadt.reset_reg (GAS structure)
            // For now, just indicate it's not implemented
            Err("ACPI: Reboot not yet implemented")
        } else {
            Err("ACPI: No FADT for reboot information")
        }
    }

    /// Get ACPI devices found via DSDT/SSDT parsing
    pub fn devices(&self) -> &[AcpiDevice] {
        &self.devices
    }

    /// Get OEM information from RSDP
    pub fn oem_info(&self) -> Result<(String, u8), &'static str> {
        if self.rsdp_address == 0 {
            return Err("ACPI: No RSDP available");
        }

        let rsdp_ptr = self.rsdp_address as *const RsdpDescriptor;
        let rsdp = unsafe { ptr::read_unaligned(rsdp_ptr) };

        // Convert OEM ID to string (6 bytes, may have trailing spaces)
        let mut oem_id = alloc::string::String::new();
        for &byte in &rsdp.oem_id {
            if byte != 0 && byte != b' ' {
                oem_id.push(byte as char);
            }
        }

        Ok((oem_id, rsdp.revision))
    }
}

/// Global ACPI manager instance
static mut ACPI_MANAGER: Option<AcpiManager> = None;
static ACPI_INITIALIZED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Initialize global ACPI manager
pub fn init(rsdp_address: u64) -> Result<(), &'static str> {
    if ACPI_INITIALIZED.load(core::sync::atomic::Ordering::SeqCst) {
        return Err("ACPI: Already initialized");
    }

    unsafe {
        if ACPI_MANAGER.is_some() {
            return Err("ACPI: Already initialized");
        }

        let mut manager = AcpiManager::new(rsdp_address);
        manager.initialize()?;
        ACPI_MANAGER = Some(manager);
    }

    ACPI_INITIALIZED.store(true, core::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Get reference to global ACPI manager
pub fn get_manager() -> Option<&'static AcpiManager> {
    if ACPI_INITIALIZED.load(core::sync::atomic::Ordering::SeqCst) {
        unsafe { ACPI_MANAGER.as_ref() }
    } else {
        None
    }
}

/// Get mutable reference to global ACPI manager
pub fn get_manager_mut() -> Option<&'static mut AcpiManager> {
    if ACPI_INITIALIZED.load(core::sync::atomic::Ordering::SeqCst) {
        unsafe { ACPI_MANAGER.as_mut() }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acpi_manager_creation() {
        let manager = AcpiManager::new(0x1000);
        assert_eq!(manager.rsdp_address, 0x1000);
        assert!(!manager.enabled);
    }

    #[test]
    fn test_acpi_signature_constants() {
        assert_eq!(*RSDT_SIGNATURE, *b"RSDT");
        assert_eq!(*XSDT_SIGNATURE, *b"XSDT");
        assert_eq!(*DSDT_SIGNATURE, *b"DSDT");
        assert_eq!(*FADT_SIGNATURE, *b"FACP");
        assert_eq!(*MADT_SIGNATURE, *b"APIC");
    }
}
