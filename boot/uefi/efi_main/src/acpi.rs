use uefi::system;
use uefi::table::cfg::ConfigTableEntry;

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
    let mut candidate: Option<&'static RsdpDescriptor> = None;

    system::with_config_table(|entries| {
        for entry in entries {
            if entry.guid == ConfigTableEntry::ACPI2_GUID || entry.guid == ConfigTableEntry::ACPI_GUID {
                if let Some(rsdp) = rsdp_from_config_entry(entry) {
                    candidate = Some(rsdp);
                    if entry.guid == ConfigTableEntry::ACPI2_GUID {
                        break;
                    }
                }
            }
        }
    });

    candidate
}

fn rsdp_from_config_entry(entry: &ConfigTableEntry) -> Option<&'static RsdpDescriptor> {
    let ptr = entry.address.cast::<RsdpDescriptor>();
    if ptr.is_null() {
        return None;
    }

    let rsdp = unsafe { &*ptr };
    if rsdp.signature != *b"RSD PTR " {
        return None;
    }

    if !validate_checksum(ptr) {
        return None;
    }

    if rsdp.revision >= 2 {
        let length = rsdp.length as usize;
        if length < core::mem::size_of::<RsdpDescriptor>() {
            return None;
        }
        if !validate_extended_checksum(ptr, length) {
            return None;
        }
    }

    Some(rsdp)
}

fn validate_checksum(ptr: *const RsdpDescriptor) -> bool {
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, 20) };
    let sum: u8 = bytes.iter().fold(0, |acc, &x| acc.wrapping_add(x));
    sum == 0
}

fn validate_extended_checksum(ptr: *const RsdpDescriptor, length: usize) -> bool {
    let bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, length) };
    let sum: u8 = bytes.iter().fold(0, |acc, &x| acc.wrapping_add(x));
    sum == 0
}
