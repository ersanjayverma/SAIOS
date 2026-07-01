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
        console::println!("SAIOS");
        console::println!("Kernel started");
        console::println!("Framebuffer console OK");

        console::println!("SAIOS kernel started");
        // Verify the boot_info pointer is valid by reading magic.
        // SAFETY: pointer is valid per the boot contract.
        let bi = unsafe { &*self.boot_info };
        // Use simple printing — complex format_args! pulls in ~74 KB
        // of core::fmt code that may not work yet.
        if bi.magic == efi_main::SAIOS_BOOT_MAGIC {
            console::println!("Boot magic OK");
        } else {
            console::println!("Boot magic BAD");
        }

        console::println!("Hello from SAIOS kernel!");

        loop {
            hal::arch::x86_64::cpu::hlt();
        }
    }
}
