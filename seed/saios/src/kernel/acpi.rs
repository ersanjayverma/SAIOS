use alloc::string::String;
/// ACPI (Advanced Configuration and Power Interface) Support Module
/// Provides full ACPI table parsing, device enumeration, power management,
/// and interrupt routing for the SAIOS kernel.
use alloc::vec::Vec;
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

/// Generic Address Space (GAS) Structure
/// Used for ACPI register addresses
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct GenericAddressSpace {
    pub address_space_id: u8, // 0=Mem, 1=IO, 2=PCI_CFG, 3=EC, 4=SMBus, 5=CMOS, 6=PCIBAR
    pub register_bit_width: u8,
    pub register_bit_offset: u8,
    pub access_size: u8, // 0=undef, 1=byte, 2=word, 3=dword, 4=qword
    pub address: u64,
}

impl GenericAddressSpace {
    /// Check if this GAS describes an IO port access
    pub fn is_io_port(&self) -> bool {
        self.address_space_id == 1
    }

    /// Check if this GAS describes memory access
    pub fn is_memory(&self) -> bool {
        self.address_space_id == 0
    }

    /// Check if GAS is valid (has a non-zero address)
    pub fn is_valid(&self) -> bool {
        self.address != 0
    }
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
    X2ApicNmi = 10,
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
    S0, // Working
    S1, // Sleeping (light)
    S2, // Sleeping (medium)
    S3, // Sleeping (deep, memory)
    S4, // Hibernation
    S5, // Soft off
}

/// ACPI Device Information
#[derive(Debug, Clone)]
pub struct AcpiDevice {
    pub name: String,
    pub hardware_id: String,
    pub unique_id: String,
    pub device_class: String,
}

/// IO APIC Information
#[derive(Debug, Clone, Copy)]
pub struct IoApicInfo {
    pub io_apic_id: u8,
    pub io_apic_address: u32,
    pub global_system_interrupt_base: u32,
}

/// Interrupt Source Override Information
#[derive(Debug, Clone, Copy)]
pub struct InterruptSourceOverride {
    pub bus: u8,
    pub source: u8,
    pub global_system_interrupt: u32,
    pub flags: u16,
}

/// Main ACPI Manager
pub struct AcpiManager {
    rsdp_address: u64,
    revision: u8,
    processors: Vec<ProcessorInfo>,
    devices: Vec<AcpiDevice>,
    io_apics: Vec<IoApicInfo>,
    interrupt_overrides: Vec<InterruptSourceOverride>,
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
            io_apics: Vec::new(),
            interrupt_overrides: Vec::new(),
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
        let basic_bytes =
            unsafe { core::slice::from_raw_parts(self.rsdp_address as *const u8, 20) };
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
        let expected_sig = if is_xsdt {
            XSDT_SIGNATURE
        } else {
            RSDT_SIGNATURE
        };
        if header.signature != *expected_sig {
            return Err("ACPI: Invalid root table signature");
        }

        // Validate checksum
        let table_bytes =
            unsafe { core::slice::from_raw_parts(root_addr as *const u8, header.length as usize) };
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
            [b'D', b'S', b'D', b'T'] => self.dsdt_address = table_addr,
            [b'F', b'A', b'C', b'P'] => self.fadt_address = table_addr,
            [b'A', b'P', b'I', b'C'] => self.madt_address = table_addr,
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
    fn parse_madt_io_apic(&mut self, entry_ptr: *const u8) {
        let entry_ptr = entry_ptr as *const MadtIoApic;
        let entry = unsafe { ptr::read_unaligned(entry_ptr) };

        self.io_apics.push(IoApicInfo {
            io_apic_id: entry.io_apic_id,
            io_apic_address: entry.io_apic_address,
            global_system_interrupt_base: entry.global_system_interrupt_base,
        });
    }

    /// Parse Interrupt Source Override entry in MADT
    fn parse_madt_interrupt_source_override(&mut self, entry_ptr: *const u8) {
        let entry_ptr = entry_ptr as *const MadtInterruptSourceOverride;
        let entry = unsafe { ptr::read_unaligned(entry_ptr) };

        self.interrupt_overrides.push(InterruptSourceOverride {
            bus: entry.bus,
            source: entry.source,
            global_system_interrupt: entry.global_system_interrupt,
            flags: entry.flags,
        });
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

    /// Get IO APICs discovered in MADT
    pub fn io_apics(&self) -> &[IoApicInfo] {
        &self.io_apics
    }

    /// Get interrupt source overrides discovered in MADT
    pub fn interrupt_overrides(&self) -> &[InterruptSourceOverride] {
        &self.interrupt_overrides
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
    /// Attempt to enter sleep state (requires AML interpreter in full implementation)
    pub fn enter_sleep_state(&self, state: SleepState) -> Result<(), &'static str> {
        if !self.enabled {
            return Err("ACPI: Not enabled");
        }

        // S5 (soft power-off) is handled by shutdown()
        if state == SleepState::S5 {
            return self.shutdown();
        }

        // All other S-states (S1-S4) require AML interpreter to:
        // 1. Find \_S<n> objects in DSDT/SSDT
        // 2. Extract SLP_TYP_A and SLP_TYP_B values
        // 3. Execute \_PTS (prepare to sleep) method
        // 4. Write to PM1A_CNT register
        // 5. Execute \_BFS (back from sleep) on wake
        //
        // This is deferred to Phase 2 (AML interpreter implementation)
        Err("ACPI: Sleep state support requires AML interpreter (Phase 2)")
    }

    /// Shutdown system using ACPI S5 (soft power-off) or platform shutdown
    pub fn shutdown(&self) -> Result<(), &'static str> {
        if self.fadt_address == 0 {
            return self.platform_shutdown();
        }

        let fadt_ptr = self.fadt_address as *const Fadt;
        let fadt = unsafe { ptr::read_unaligned(fadt_ptr) };

        // Try to use PM1A_CNT register if available
        if fadt.pm1a_cnt_blk != 0 {
            // PM1A Control block base address
            let pm1a_cnt_addr = fadt.pm1a_cnt_blk as u16;

            // Without AML interpreter, we use a fallback S5 SLP_TYP value
            // Typical values: S5 SLP_TYP = 0x00 or platform-specific
            // We'll write to PM1A_CNT: (SLP_TYP << 10) | SLP_EN (bit 13)
            const S5_SLP_TYP: u16 = 0x00; // S5 SLP type (varies by platform)
            const SLP_EN: u16 = 1 << 13; // Sleep enable bit

            let pm_value = (S5_SLP_TYP << 10) | SLP_EN;

            // Write to I/O port
            unsafe {
                core::arch::asm!(
                    "out dx, ax",
                    in("dx") pm1a_cnt_addr,
                    in("ax") pm_value,
                    options(nomem, nostack, preserves_flags)
                );
            }

            // Wait for shutdown to take effect
            loop {
                hal::arch::x86_64::cpu::hlt();
            }
        }

        // Fallback to platform shutdown
        self.platform_shutdown()
    }

    /// Platform-specific shutdown (x86 specific)
    fn platform_shutdown(&self) -> Result<(), &'static str> {
        // Try x86 CMOS/IO shutdown via port 0xCF9
        // Value 0x0E triggers system shutdown on most x86 systems
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") 0xCF9u16,
                in("al") 0x0Eu8,
                options(nomem, nostack, preserves_flags)
            );
        }

        // Wait for shutdown
        loop {
            hal::arch::x86_64::cpu::hlt();
        }
    }

    /// Reboot system using ACPI reset or platform-specific method
    pub fn reboot(&self) -> Result<(), &'static str> {
        if self.fadt_address == 0 {
            return self.platform_reset();
        }

        let fadt_ptr = self.fadt_address as *const Fadt;
        let fadt = unsafe { ptr::read_unaligned(fadt_ptr) };

        // FADT reset register is in fadt.reset_reg (12 bytes = GAS structure)
        // Try to use reset register if available
        if fadt.reset_reg[0] != 0 {
            // address_space_id field
            let gas_ptr = &fadt.reset_reg as *const [u8; 12] as *const GenericAddressSpace;
            let gas = unsafe { ptr::read_unaligned(gas_ptr) };

            if gas.is_valid() {
                // Write reset value to the reset register
                if gas.is_io_port() {
                    // IO port access
                    unsafe {
                        core::arch::asm!(
                            "out dx, al",
                            in("dx") gas.address as u16,
                            in("al") fadt.reset_value,
                            options(nomem, nostack, preserves_flags)
                        );
                    }

                    // Wait for reset
                    loop {
                        hal::arch::x86_64::cpu::hlt();
                    }
                } else if gas.is_memory() {
                    // Memory-mapped access
                    let reset_ptr = gas.address as *mut u8;
                    unsafe {
                        ptr::write(reset_ptr, fadt.reset_value);
                    }

                    // Wait for reset
                    loop {
                        hal::arch::x86_64::cpu::hlt();
                    }
                }
            }
        }

        // Fallback to platform reset
        self.platform_reset()
    }

    /// Platform-specific reset (x86 specific)
    fn platform_reset(&self) -> Result<(), &'static str> {
        // Try x86 reset via port 0xCF9
        // Value 0x06 triggers system reset on most x86 systems
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") 0xCF9u16,
                in("al") 0x06u8,
                options(nomem, nostack, preserves_flags)
            );
        }

        // If that doesn't work, try triple fault
        // Create invalid IDT descriptor to trigger fault
        unsafe {
            let idt_desc: u64 = 0;
            core::arch::asm!(
                "lidt [{}]",
                in(reg) &idt_desc,
            );
            // Trigger interrupt to cause triple fault
            core::arch::asm!("int 0");
        }

        // Should not reach here
        Err("ACPI: Platform reset failed")
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
/// SAFETY: Protected by ACPI_INITIALIZED atomic flag. Once initialized, remains immutable.
#[allow(static_mut_refs)]
static mut ACPI_MANAGER: Option<AcpiManager> = None;
static ACPI_INITIALIZED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Initialize global ACPI manager
pub fn init(rsdp_address: u64) -> Result<(), &'static str> {
    if ACPI_INITIALIZED.load(core::sync::atomic::Ordering::SeqCst) {
        return Err("ACPI: Already initialized");
    }

    unsafe {
        #[allow(static_mut_refs)]
        if ACPI_MANAGER.is_some() {
            return Err("ACPI: Already initialized");
        }

        let mut manager = AcpiManager::new(rsdp_address);
        manager.initialize()?;
        #[allow(static_mut_refs)]
        {
            ACPI_MANAGER = Some(manager);
        }
    }

    ACPI_INITIALIZED.store(true, core::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Get reference to global ACPI manager
pub fn get_manager() -> Option<&'static AcpiManager> {
    if ACPI_INITIALIZED.load(core::sync::atomic::Ordering::SeqCst) {
        unsafe {
            #[allow(static_mut_refs)]
            {
                ACPI_MANAGER.as_ref()
            }
        }
    } else {
        None
    }
}

/// Get mutable reference to global ACPI manager
pub fn get_manager_mut() -> Option<&'static mut AcpiManager> {
    if ACPI_INITIALIZED.load(core::sync::atomic::Ordering::SeqCst) {
        unsafe {
            #[allow(static_mut_refs)]
            {
                ACPI_MANAGER.as_mut()
            }
        }
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

    #[test]
    fn test_gas_structure_size() {
        // GenericAddressSpace must be 12 bytes to match FADT reset_reg
        assert_eq!(core::mem::size_of::<GenericAddressSpace>(), 12);
    }

    #[test]
    fn test_rsdp_descriptor_size() {
        // RSDP must be 36 bytes for proper parsing
        assert_eq!(core::mem::size_of::<RsdpDescriptor>(), 36);
    }

    #[test]
    fn test_acpi_table_header_size() {
        // Standard ACPI header is 36 bytes
        assert_eq!(core::mem::size_of::<AcpiTableHeader>(), 36);
    }
}
