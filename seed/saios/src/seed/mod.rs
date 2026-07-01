use efi_main::SaiosBootInfo;
use crate::console;

pub struct Seed {
    boot_info: *const SaiosBootInfo,
}

impl Seed {
    pub fn init(boot_info: *const SaiosBootInfo) -> Self {
        Self { boot_info }
    }

    pub fn run(&self) -> ! {
        let _ = self.boot_info;

        console::clear();
        console::println!("SAIOS");
        console::println!();
        console::prompt();

        loop {
            let _ = console::poll_input();
            hal::arch::x86_64::cpu::pause();
        }
    }
}
