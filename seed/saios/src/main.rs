#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
pub mod driver;
pub mod console;
pub mod graphics;
pub mod heap;
pub mod kernel;
pub mod kernel_arch;
pub mod ksf;
pub mod memory;
pub mod object_manager;
pub mod pci;
pub mod pmm;
pub mod provider;
pub mod saifs;
pub mod scheduler;
pub mod shell;
pub mod sif;
pub mod som;
pub mod snom;
pub mod seed;
pub mod timer;
pub mod vfs;
use efi_main::SaiosBootInfo;
use hal::arch::paging::{self, Table};
use hal::arch::x86_64::{gdt, idt, interrupt};
use seed::Seed;

#[global_allocator]
static GLOBAL_ALLOCATOR: heap::KernelHeapAllocator = heap::KernelHeapAllocator;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    interrupt::disable();
    kernel::timeline::init();
    kernel::timeline::mark("Boot");
    let boot_info = unsafe { &*boot_info };
    console::attach_framebuffer(boot_info.framebuffer);
    driver::console::init();
    gdt::init();
    idt::init();

    // Convert the raw pointer and count into a temporary Rust slice
    let _entries_slice = unsafe {
        core::slice::from_raw_parts(boot_info.memorymap.entries, boot_info.memorymap.entry_count)
    };
    // Initialize PMM with the boot memory map and take one page for PML4.
    pmm::init(_entries_slice);
    kernel::timeline::mark("Memory");
    let pml4_phys = pmm::alloc_page().expect("PMM: no free pages for PML4");
    let pml4_ptr = pml4_phys as *mut Table;
    unsafe { (*pml4_ptr).clear() };
    // Recursive mapping: last PML4 slot points to the PML4 itself.
    unsafe {
        (*pml4_ptr).entries[511].set_page(pml4_phys, paging::FLAG_WRITABLE);
    }

    heap::init();
    kernel::timeline::mark("Heap");
    let _ = console::promote_framebuffer_renderer();
    ksf::bootstrap().expect("KSF bootstrap failed");
    kernel::timeline::mark("Services");
    interrupt::enable();
    if cfg!(debug_assertions) {
        kernel::testing::boot_self_test();
    }
   
    let seed = Seed::init(boot_info as *const SaiosBootInfo);
    kernel::timeline::mark("Ready");
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
