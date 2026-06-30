use crate::println;
use efi_main::SaiosBootInfo;
pub struct Seed {
    pub boot_info: *const SaiosBootInfo,
}
impl Seed {
    pub fn init(boot_info: *const SaiosBootInfo) -> Self {
        Self { boot_info }
    }
    pub fn run(&self) -> ! {
        // Print a message to the serial console
        println!("Hello from SAIOS kernel!");
        loop {
            core::hint::spin_loop();
        }
    }
}
