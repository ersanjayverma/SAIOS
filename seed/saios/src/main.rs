#![no_std]
#![no_main]
pub mod seed;
use efi_main::SaiosBootInfo;

#[unsafe(no_mangle)]
/// # Safety
///
/// `boot_info` must be a valid pointer supplied by the SAIOS bootloader entry
/// contract and remain valid throughout early kernel initialization.
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
   seed::init(boot_info);
    seed::run()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    efi_main::panic_handler(info)
}
