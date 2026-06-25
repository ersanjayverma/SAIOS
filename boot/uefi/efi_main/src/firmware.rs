use uefi::system;
use alloc::string::{String, ToString};
use uefi::table::Revision;
pub struct FirmwareInfo {
    pub vendor: String,
    pub firmware_revision: u32,
    pub uefi_revision: Revision,
}
pub fn initialize() -> uefi::Result<FirmwareInfo> {
    let vendor = system::firmware_vendor().to_string();

    let firmware_revision = system::firmware_revision();

    let uefi_revision = system::uefi_revision();

    Ok(FirmwareInfo {
        vendor,
        firmware_revision,
        uefi_revision,
    })
}