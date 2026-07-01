use efi_main::SaiosBootInfo;
use crate::console;
use crate::scheduler;
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

        scheduler::spawn(thread_a);
        scheduler::spawn(thread_b);

        shell::run()
    }
}

fn thread_a() {
    loop {
        console::println!("A");
        scheduler::yield_now();
    }
}

fn thread_b() {
    loop {
        console::println!("B");
        scheduler::yield_now();
    }
}
