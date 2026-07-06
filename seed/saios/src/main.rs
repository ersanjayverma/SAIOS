//! SAIOS kernel main binary.
//!
//! This is the kernel entry point. It initializes the GDT, IDT, physical and
//! virtual memory managers, heap, console, KSF services, ACPI and finally
//! hands control to the seed runtime.

#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
pub mod driver;
pub mod console;
pub mod diskpart;
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
pub mod seed;
pub mod shell;
pub mod sif;
pub mod snom;
pub mod som;
pub mod taskman;
pub mod timer;
pub mod vfs;
pub mod vmm;
use core::mem::size_of;
use efi_main::SaiosBootInfo;
use hal::arch::x86_64::{gdt, idt, interrupt, paging};
use seed::Seed;

unsafe extern "C" {
    static _kernel_start: u8;
    static _kernel_end: u8;
}

use crate::kernel::constants::{
    KERNEL_PHYS_BASE, PTE_ADDR_MASK,
};

/// Kernel heap module.

fn detect_nx_page_protection_policy() -> bool {
    let features = hal::arch::x86_64::cpuid::features();
    let mut efer_nxe = false;
    if features.nx && features.msr {
        let efer = hal::arch::x86_64::msr::rdmsr(0xC000_0080);
        efer_nxe = (efer & (1 << 11)) != 0;
    }
    features.nx && efer_nxe
}

type DescriptorTablePtr = hal::arch::x86_64::cpu::DescriptorTablePtr;

fn read_rsp() -> u64 {
    hal::arch::x86_64::cpu::read_rsp()
}

fn read_rip() -> u64 {
    hal::arch::x86_64::cpu::read_rip()
}

fn read_idt_ptr() -> DescriptorTablePtr {
    hal::arch::x86_64::cpu::read_idt_ptr()
}

fn read_gdt_ptr() -> DescriptorTablePtr {
    hal::arch::x86_64::cpu::read_gdt_ptr()
}

#[inline(never)]
fn late_cr3_smoke_test() {
    // Minimal post-switch probe: instruction stream, global data, and stack.
    let mut stack_word: u64 = 0x5A5A_A5A5_1122_3344;
    let marker_ptr = &GLOBAL_ALLOCATOR as *const _ as *const u8;
    unsafe {
        let marker = core::ptr::read_volatile(marker_ptr);
        stack_word ^= marker as u64;
        core::ptr::write_volatile(&mut stack_word as *mut u64, stack_word.rotate_left(7));
        core::hint::black_box(stack_word);
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: heap::KernelHeapAllocator = heap::KernelHeapAllocator;

/// # Safety
///
/// This is the kernel entry point invoked by the bootloader. `boot_info` must
/// be a valid, properly aligned pointer to a `SaiosBootInfo` structure that
/// remains valid for the lifetime of the kernel. Must be called exactly once
/// with interrupts in a defined state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn saios_kernel_main(boot_info: *const SaiosBootInfo) -> ! {
    hal::arch::x86_64::seed_support::ensure_linked();
    let boot_info = unsafe { &*boot_info };
    let framebuffer_info = boot_info.framebuffer;

    // Bring up UART immediately so post-ExitBootServices progress is always visible.
    hal::arch::x86_64::console::init_serial();
    hal::arch::x86_64::console::set_output_enabled(true);
    interrupt::disable();

    gdt::init();
    idt::init();
    if let Err(e) = hal::arch::x86_64::syscall::init() {
        hal::arch::x86_64::console::_print(format_args!(
            "kernel: syscall init failed: {}\n",
            e
        ));
    }
    kernel::fault::init();

    // Convert the raw pointer and count into a temporary Rust slice
    let _entries_slice = unsafe {
        core::slice::from_raw_parts(boot_info.memorymap.entries, boot_info.memorymap.entry_count)
    };
    pmm::init(_entries_slice);

    let kernel_vma_start = unsafe { &_kernel_start as *const u8 as u64 };
    let kernel_vma_end = unsafe { &_kernel_end as *const u8 as u64 };
    // Physical kernel image starts at the boot trampoline (KERNEL_PHYS_BASE)
    // and ends where the higher-half sections end (kernel_vma_end - offset).
    let kernel_start = KERNEL_PHYS_BASE;
    let kernel_end = kernel_vma_end.saturating_sub(vmm::KERNEL_IMAGE_MIRROR_BASE - KERNEL_PHYS_BASE);
    let boot_info_ptr = boot_info as *const SaiosBootInfo as u64;
    let boot_info_size = size_of::<SaiosBootInfo>();

    let nx_enabled = detect_nx_page_protection_policy();
    vmm::set_nx_page_protection_enabled(nx_enabled);
    // Track the kernel image using its higher-half VMA range so user-space
    // ELF overlap checks compare against where the kernel actually executes.
    vmm::set_kernel_image_range(kernel_vma_start, kernel_vma_end);

    let (_prepared_kernel_pml4, active_cr3, vmm_bootstrap_ok) =
        match vmm::bootstrap_kernel_page_tables(
            framebuffer_info.base,
            framebuffer_info.size,
            boot_info_ptr,
            boot_info_size,
            kernel_start,
            kernel_end,
        ) {
            Ok(pml4) => {
                let current_cr3 = paging::read_cr3() & PTE_ADDR_MASK;
                hal::arch::x86_64::console::_print(format_args!(
                    "kernel: VMM bootstrap succeeded (cr3={:#x})\n",
                    current_cr3
                ));
                (Some(pml4), current_cr3, false)
            }
            Err(e) => {
                // Fallback path for firmware that cannot tolerate early CR3/bootstrap assumptions.
                let current_cr3 = paging::read_cr3() & PTE_ADDR_MASK;
                hal::arch::x86_64::console::_print(format_args!(
                    "kernel: VMM bootstrap failed: {} (fallback cr3={:#x})\n",
                    e, current_cr3
                ));
                (None, current_cr3, false)
            }
        };

    if let Err(e) = vmm::init(active_cr3) {
        hal::arch::x86_64::console::_print(format_args!("kernel: VMM init failed: {}\n", e));
        panic!("VMM: failed to initialize kernel virtual memory manager");
    }



    heap::init();

    kernel::timeline::init();
    kernel::timeline::mark("Boot");
    kernel::timeline::mark("Memory");

    // Higher-half bring-up can still have edge cases in the dynamic mapping
    // path. If mapped attach did not become visible, retry with direct GOP
    // pointer path (works while low-memory identity window is active).
    if !console::framebuffer_attached() {
        console::attach_framebuffer_direct(framebuffer_info);
    }

    driver::console::init();
    console::set_serial_logging(true);
    kernel::timeline::mark("Heap");

    let mut fb_ready = console::promote_framebuffer_renderer();
    if !fb_ready {
        console::attach_framebuffer_direct(framebuffer_info);
        fb_ready = console::promote_framebuffer_renderer();
    }
    if fb_ready {
        console::println!("SAIOS kernel framebuffer online");
        console::println!("Starting kernel services...");
    }

    ksf::bootstrap().expect("KSF bootstrap failed");
    kernel::timeline::mark("Services");

    // Initialize ACPI subsystem
    if boot_info.acpi.rsdp != 0 && heap::dynamic_mappings_available() {
        match kernel::acpi::init(boot_info.acpi.rsdp) {
            Ok(()) => {
                kernel::timeline::mark("ACPI");
            }
            Err(_e) => {}
        }
    } else if boot_info.acpi.rsdp != 0 {
        console::println!("kernel: ACPI deferred in fallback identity mode");
    }

    interrupt::enable();

    if cfg!(debug_assertions) {
        kernel::testing::boot_self_test();
    }

    let seed = Seed::init(boot_info as *const SaiosBootInfo);
    kernel::timeline::mark("Ready");

    seed.run()
}

/// Kernel panic handler.
///
/// Disables interrupts, prints panic information to the emergency serial path
/// and halts the CPU forever.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    interrupt::disable();
    console::panic_println("PANIC");
    // Print panic info directly to emergency serial path.
    hal::arch::x86_64::console::_print_force(format_args!("{}\n", info));
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}
