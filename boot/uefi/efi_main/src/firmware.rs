extern crate alloc;
use uefi::println;
use uefi::system;
use uefi::table::Revision;
#[repr(C)]
#[derive(Debug, Clone,Copy, PartialEq, Eq)]
pub struct FirmwareInfo {
    pub vendor: [u8; 32],
    pub firmware_revision: u32,
    pub uefi_revision: Revision,
}
pub fn initialize() -> uefi::Result<FirmwareInfo> {
    let vendor_src = system::firmware_vendor().as_bytes(); // Convert &str to &[u8]

    // 1. Create a blank 32-byte array initialized to zeros
    let mut vendor = [0u8; 32];

    // 2. Determine how many bytes can safely fit (max 32)
    let len = core::cmp::min(vendor_src.len(), 32);
    vendor[..len].copy_from_slice(&vendor_src[..len]);

    let firmware_revision = system::firmware_revision();

    let uefi_revision = system::uefi_revision();

    println!("Vendor   : {}", core::str::from_utf8(&vendor) .unwrap_or("Invalid UTF-8"));
    println!("Firmware : {}", firmware_revision);
    println!("UEFI     : {}", uefi_revision);
    Ok(FirmwareInfo {
        vendor,
        firmware_revision,
        uefi_revision,
    })
}
