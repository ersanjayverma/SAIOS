use efi_main::SaiosBootInfo;
use crate::shell;

pub struct Seed {
    boot_info: *const SaiosBootInfo,
}

impl Seed {
    pub fn init(boot_info: *const SaiosBootInfo) -> Self {
        Self { boot_info }
    }

    pub fn run(&self) -> ! {
        let _ = self.boot_info;
        shell::init();
        shell::run()
    }
}
