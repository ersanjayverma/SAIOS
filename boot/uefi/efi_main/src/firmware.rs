use uefi::prelude::*;
use uefi::println;

pub fn print(st: &SystemTable<Boot>) {
    println!("Firmware Vendor: {}", st.firmware_vendor());
    println!("Firmware Revision: {}", st.firmware_revision());
}