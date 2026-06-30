#![no_std]
#![no_main]

pub mod driver;
pub mod seed;
use efi_main::SaiosBootInfo;
use seed::Seed;
#[unsafe(no_mangle)]
/// # Safety
///
/// `boot_info` must be a valid pointer supplied by the SAIOS bootloader entry
/// contract and remain valid throughout early kernel initialization.
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    let seed = Seed::init(boot_info);
    println!("SAIOS kernel started");
    println!("Serial initialized");
    seed.run()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    match info.location() {
        Some(loc) => {
            println!("Panic at {}:{}", loc.file(), loc.line());
        }
        None => {
            println!("Panic");
        }
    }
    loop {}
}
