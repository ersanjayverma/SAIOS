#![no_std]
#![no_main]

pub mod arch;
pub mod boot;
pub mod console;
pub mod diagnostics;
pub mod drivers;
pub mod fs;
pub mod graphics;
pub mod ipc;
pub mod log;
pub mod memory;
pub mod net;
pub mod process;
pub mod rrod;
pub mod scheduler;
pub mod seed;
pub mod timer;

use efi_main::SaiosBootInfo;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    arch::disable_interrupts();

    seed::init(boot_info);
    seed::run()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    console::init_serial();
    console::panic_prelude(info);
    let context = rrod::capture::from_panic(info);
    rrod::trigger(context)
}
