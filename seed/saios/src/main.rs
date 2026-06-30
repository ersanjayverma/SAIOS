#![no_std]
#![no_main]

#[macro_use]
pub mod driver;
pub mod seed;
use hal::println;
use efi_main::SaiosBootInfo;
use hal::arch::x86_64::{gdt, idt, interrupt, paging};
use seed::Seed;

const INITIAL_MAP_SIZE: u64 = 1024 * 1024 * 1024; // 1 GiB
const KERNEL_PHYSICAL_OFFSET: u64 = 0; // identity-mapped: virt == phys

#[unsafe(no_mangle)]
pub unsafe extern "win64" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    interrupt::disable();
    driver::console::init();
    gdt::init();
    idt::init();
  
    println!("start");
    // Set up paging: identity-map first 1 GiB, enable NX/WP.
    if paging::nx_supported() {
        paging::enable_nx();
    }
   
    println!("Calling identity_map...");
    let pml4_phys = unsafe { paging::identity_map(INITIAL_MAP_SIZE, KERNEL_PHYSICAL_OFFSET) };
    println!("identity_map returned: pml4_phys = {:#x}", pml4_phys);
    if pml4_phys == 0 {
        println!("ERROR: identity_map returned 0");
        loop {
            hal::arch::x86_64::cpu::hlt();
        }
    }
    println!("Paging initialized");
    unsafe { paging::load_cr3(pml4_phys); }
    paging::enable_write_protect();

    println!("Paging OK");

    let seed = Seed::init(boot_info);
    seed.run()
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    interrupt::disable();
    println!("PANIC");
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}
