#![no_std]
#![no_main]

pub mod arch;
pub mod boot;
pub mod diagnostics;
pub mod drivers;
pub mod fs;
pub mod graphics;
pub mod ipc;
pub mod kernel;
pub mod log;
pub mod memory;
pub mod net;
pub mod process;
pub mod rrod;
pub mod scheduler;

use efi_main::SaiosBootInfo;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }

    kernel::init(boot_info);
    kernel::run()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let context = rrod::capture::from_panic(info);
    rrod::trigger(context)
}
