#![no_std]
#![no_main]

#[macro_use]
pub mod driver;
pub mod console;
pub mod seed;
use driver::memory::{alloc_page, available, init, used};
use efi_main::SaiosBootInfo;
use hal::arch::paging::{self, Table};
use hal::arch::x86_64::{gdt, idt, interrupt};
use seed::Seed;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    interrupt::disable();
    unsafe { console::attach_framebuffer((*boot_info).framebuffer) };
    driver::console::init();
    gdt::init();
    idt::init();

    console::println!("start");
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
    // Recursive mapping: last PML4 slot points to the PML4 itself.
    unsafe {
        (*pml4_ptr).entries[511].set_page(pml4_phys, paging::FLAG_WRITABLE);
    }
    console::println!("Allocated PML4 at physical address: {:#x}", pml4_phys);
    console::println!("PMM used: {} bytes", unsafe { used() });
    console::println!("PMM available: {} bytes", unsafe { available() });
    console::println!("Current CR3: {:#x}", paging::read_cr3());
   
    let seed = Seed::init(boot_info);
    seed.run()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    interrupt::disable();
    console::panic_println("PANIC");
    // Print panic info directly to emergency serial path.
    hal::arch::x86_64::console::_print(format_args!("{}\n", info));
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}
