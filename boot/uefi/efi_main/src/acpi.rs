#[repr(C)]
pub struct AcpiInfo {
    pub rsdp: u64,
    pub revision: u8,

    pub rsdt: u64,
    pub xsdt: u64,

    pub oem_id: [u8; 6],
}
#[repr(C, packed)]
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
    None
}
