extern crate alloc;
use alloc::string::String;
use uefi::println;
use uefi::system;
use uefi::table::Revision;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareInfo {
    pub vendor: [u8; 32],
    pub firmware_revision: u32,
    pub uefi_revision: Revision,
}

pub fn initialize() -> uefi::Result<FirmwareInfo> {
    // `firmware_vendor()` returns a `&CStr16` (UCS-2 string).
    // `CStr16` implements `Display` and `From<&CStr16> for String`,
    // both of which properly convert to UTF-8.
    let vendor_cstr16 = system::firmware_vendor();

    // Convert to a Rust String (UTF-8), then copy into the fixed-size
    // array for the boot-info struct.
    let vendor_string: String = vendor_cstr16.into();
    let vendor_bytes = vendor_string.as_bytes();

    let mut vendor = [0u8; 32];
    let len = core::cmp::min(vendor_bytes.len(), 32);
    vendor[..len].copy_from_slice(&vendor_bytes[..len]);

    let firmware_revision = system::firmware_revision();
    let uefi_revision = system::uefi_revision();

    // Print using the Display impl — proper UTF-8 conversion.
    println!("Vendor   : {}", vendor_cstr16);
    println!("Firmware : {}", firmware_revision);
    println!("UEFI     : {}", uefi_revision);

    Ok(FirmwareInfo {
        vendor,
        firmware_revision,
        uefi_revision,
    })
}
