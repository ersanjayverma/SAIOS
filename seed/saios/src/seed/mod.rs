use crate::scheduler;
use efi_main::SaiosBootInfo;

pub struct Seed {
    boot_info: *const SaiosBootInfo,
}

impl Seed {
    pub fn init(boot_info: *const SaiosBootInfo) -> Self {
        Self { boot_info }
    }

    pub fn run(&self) -> ! {
        let _ = self.boot_info;

        idle_loop()
    }
}

fn idle_loop() -> ! {
    loop {
        scheduler::yield_now();
        hal::arch::x86_64::cpu::hlt();
    }
}
