#![no_std]
#![no_main]

#[macro_use]
pub mod driver;
pub mod seed;
use driver::memory::{alloc_page, available, init, used};
use efi_main::SaiosBootInfo;
use hal::arch::paging::Table;
use hal::arch::x86_64::{gdt, idt, interrupt};
use hal::println;
use seed::Seed;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    interrupt::disable();
    driver::console::init();
    gdt::init();
    idt::init();

    println!("start");
    // Convert the raw pointer and count into a temporary Rust slice
    let _entries_slice = unsafe {
        core::slice::from_raw_parts(
            (*boot_info).memorymap.entries,
            (*boot_info).memorymap.entry_count,
        )
    };
    // Initialize PMM with the boot memory map and take one page for PML4.
    unsafe { init(_entries_slice) };
    let pml4_phys = unsafe { alloc_page() }.expect("PMM: no free pages for PML4");
    let pml4_ptr = pml4_phys as *mut Table;
    unsafe { (*pml4_ptr).clear() };
    println!("Allocated PML4 at physical address: {:#x}", pml4_phys);
    println!("PMM used: {} bytes", unsafe { used() });
    println!("PMM available: {} bytes", unsafe { available() });
   
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
