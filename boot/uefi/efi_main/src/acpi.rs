#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct AcpiInfo {
    pub rsdp: u64,
    pub revision: u8,

    pub rsdt: u64,
    pub xsdt: u64,

    pub oem_id: [u8; 6],
}
#[repr(C, packed)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct RsdpDescriptor {
    pub signature: [u8; 8], // "RSD PTR "
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,

    // ACPI 2+
    pub length: u32,
    pub xsdt_address: u64,
    pub extended_checksum: u8,
    pub reserved: [u8; 3],
}
pub fn initialize() -> uefi::Result<AcpiInfo> {
    let rsdp = self::find_rsdp().expect("RSDP not found");
    let _revision = rsdp.revision;
    let _rsdt = rsdp.rsdt_address as u64;
    let _xsdt = rsdp.xsdt_address as u64;
    let _oem_id = rsdp.oem_id;

    Ok(AcpiInfo {
        rsdp: rsdp as *const RsdpDescriptor as u64,
        revision: rsdp.revision,
        rsdt: rsdp.rsdt_address as u64,
        xsdt: rsdp.xsdt_address,
        oem_id: rsdp.oem_id,
    })
}
pub fn find_rsdp() -> Option<&'static RsdpDescriptor> {
    // 1. Search the Extended BIOS Data Area (EBDA)
    if let Some(rsdp) = search_ebda() {
        return Some(rsdp);
    }

    // 2. Fall back to searching the main BIOS region (0xE0000 - 0xFFFFF)
    search_region(0x000E_0000, 0x000F_FFFF)
}

fn search_ebda() -> Option<&'static RsdpDescriptor> {
    // The 16-bit segment address of EBDA is located at 0x40E
    let ebda_ptr = 0x40E as *const u16;
    let ebda_segment = unsafe { core::ptr::read_volatile(ebda_ptr) } as usize;

    // Convert segment address to absolute physical address
    let ebda_base = ebda_segment << 4;

    if ebda_base > 0 {
        // Scan the first 1 KiB of EBDA
        return search_region(ebda_base, ebda_base + 1024 - 1);
    }
    None
}

fn search_region(start: usize, end: usize) -> Option<&'static RsdpDescriptor> {
    // RSDP is guaranteed to be aligned to a 16-byte boundary
    let scan_start = (start + 15) & !15;

    for addr in (scan_start..=end).step_by(16) {
        let rsdp_ptr = addr as *const RsdpDescriptor;
        unsafe {
            // Check signature "RSD PTR "
            if (*rsdp_ptr).signature == *b"RSD PTR " {
                if validate_checksum(rsdp_ptr) {
                    return Some(&*rsdp_ptr);
                }
            }
        }
    }
    None
}

unsafe fn validate_checksum(ptr: *const RsdpDescriptor) -> bool {
    // ACPI 1.0 structures are validated by checking the first 20 bytes
    unsafe {
        let bytes = core::slice::from_raw_parts(ptr as *const u8, 20);
        let sum: u8 = bytes.iter().fold(0, |acc, &x| acc.wrapping_add(x));
        sum == 0
    }
}
