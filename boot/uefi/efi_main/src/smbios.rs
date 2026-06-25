#[repr(C)]
pub struct SmbiosInfo {
    pub entry_point: u64,
    pub version_major: u8,
    pub version_minor: u8,
    pub version_revision: u8,

    pub table_address: u64,
    pub table_length: u32,

    pub is_64bit: bool,
}
#[repr(C, packed)]
pub struct SmbiosEntryPoint32 {
    pub anchor: [u8; 4], // "_SM_"
    pub checksum: u8,
    pub length: u8,
    pub major: u8,
    pub minor: u8,
    pub max_structure_size: u16,
    pub entry_point_revision: u8,
    pub formatted_area: [u8; 5],
    pub intermediate_anchor: [u8; 5], // "_DMI_"
    pub intermediate_checksum: u8,
    pub table_length: u16,
    pub table_address: u32,
    pub structure_count: u16,
    pub bcd_revision: u8,
}
#[repr(C, packed)]
pub struct SmbiosEntryPoint64 {
    pub anchor: [u8; 5], // "_SM3_"
    pub checksum: u8,
    pub length: u8,
    pub major: u8,
    pub minor: u8,
    pub docrev: u8,
    pub revision: u8,
    pub reserved: u8,
    pub table_max_size: u32,
    pub table_address: u64,
}
pub fn initialize() -> uefi::Result<SmbiosInfo> {
    Ok(SmbiosInfo {
        entry_point: 0,
        version_major: 0,
        version_minor: 0,
        version_revision: 0,
        table_address: 0,
        table_length: 0,
        is_64bit: false,
    })
}
