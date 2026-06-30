use efi_main::SaiosBootInfo;
use hal::println;

pub struct Seed {
    boot_info: *const SaiosBootInfo,
}

impl Seed {
    pub fn init(boot_info: *const SaiosBootInfo) -> Self {
        Self { boot_info }
    }

    pub fn run(&self) -> ! {
        println!("SAIOS kernel started");
        // Verify the boot_info pointer is valid by reading magic.
        // SAFETY: pointer is valid per the boot contract.
        let bi = unsafe { &*self.boot_info };
        // Use simple printing — complex format_args! pulls in ~74 KB
        // of core::fmt code that may not work yet.
        if bi.magic == efi_main::SAIOS_BOOT_MAGIC {
            println!("Boot magic OK");
        } else {
            println!("Boot magic BAD");
        }

        println!("Hello from SAIOS kernel!");

        loop {
            hal::arch::x86_64::cpu::hlt();
        }
    }
}
