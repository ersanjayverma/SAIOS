#![no_std]
#![no_main]

#[macro_use]
pub mod driver;
pub mod seed;
use driver::memory::BootAllocator;
use efi_main::SaiosBootInfo;
use hal::arch::paging::Table;
use hal::arch::x86_64::{gdt, idt, interrupt, paging};
use hal::println;
use seed::Seed;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    interrupt::disable();
    driver::console::init();
    gdt::init();
    idt::init();

    println!("start");
    // Set up paging: identity-map first 1 GiB, enable NX/WP.
    if paging::nx_supported() {
        paging::enable_nx();
    }
    // Convert the raw pointer and count into a temporary Rust slice
    let _entries_slice = unsafe {
        core::slice::from_raw_parts(
            (*boot_info).memorymap.entries,
            (*boot_info).memorymap.entry_count,
        )
    };
    if paging::nx_supported() {
        println!("NX supported, enabling NX bit");
        paging::enable_nx();
    }

    // Pass the correctly typed slice to your allocator
    let mut allocator = unsafe { BootAllocator::new(_entries_slice) };
    // 2. Allocate your root PML4 safely out of actual physical RAM
    let pml4_ptr: *mut Table = unsafe { allocator.allocate_page_table() };
    let pml4_phys = pml4_ptr as u64;
    println!("Allocated PML4 at physical address: {:#x}", pml4_phys);
    unsafe { paging::identity_map(pml4_phys) };
    println!("Paging structures set up");
    let cr3 = paging::read_cr3();
    println!("Paging OK CR3: {:#x}", cr3);
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
