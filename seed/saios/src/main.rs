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
use core::{arch::global_asm, mem::size_of};
use efi_main::SaiosBootInfo;
use hal::arch::x86_64::{gdt, idt, interrupt, paging};
use seed::Seed;

unsafe extern "C" {
    static _kernel_start: u8;
    static _kernel_end: u8;
}

global_asm!(
    ".section .text.boot, \"ax\"",
    ".global _start",
    "_start:",
    "cli",
    "cld",
    "mov dx, 0x3f8",
    "mov al, 'K'",
    "out dx, al",
    "and rsp, -16",
    "call saios_kernel_main",
    "2:",
    "hlt",
    "jmp 2b",
);

const EARLY_CR3_SWITCH_ENABLED: bool = false;
const FALLBACK_IDENTITY_HEAP_MAX_PHYS: u64 = 0x0400_0000;

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
    let boot_info = unsafe { &*boot_info };
    let framebuffer_info = boot_info.framebuffer;

    // Bring up UART immediately so post-ExitBootServices progress is always visible.
    hal::arch::x86_64::console::init_serial();
    hal::arch::x86_64::console::set_output_enabled(true);
    interrupt::disable();

    gdt::init();
    idt::init();
    kernel::fault::init();

    // Convert the raw pointer and count into a temporary Rust slice
    let _entries_slice = unsafe {
        core::slice::from_raw_parts(boot_info.memorymap.entries, boot_info.memorymap.entry_count)
    };
    pmm::init(_entries_slice);

    let kernel_start = unsafe { &_kernel_start as *const u8 as u64 };
    let kernel_end = unsafe { &_kernel_end as *const u8 as u64 };
    let boot_info_ptr = boot_info as *const SaiosBootInfo as u64;
    let boot_info_size = size_of::<SaiosBootInfo>();


    let (active_cr3, vmm_bootstrap_ok) = match vmm::bootstrap_kernel_page_tables(
        framebuffer_info.base,
        framebuffer_info.size,
        boot_info_ptr,
        boot_info_size,
        kernel_start,
        kernel_end,
    ) {
        Ok(pml4) => {
            if EARLY_CR3_SWITCH_ENABLED {
                if let Err(e) = vmm::activate_kernel_page_tables(pml4) {
                    let current_cr3 = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;
                    hal::arch::x86_64::console::_print(format_args!(
                        "kernel: CR3 switch failed: {} (fallback cr3={:#x})\n",
                        e, current_cr3
                    ));
                    (current_cr3, false)
                } else {
                    (pml4, true)
                }
            } else {
                let current_cr3 = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;
                (current_cr3, false)
            }
        }
        Err(e) => {
            // Fallback path for firmware that cannot tolerate early CR3/bootstrap assumptions.
            let current_cr3 = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;
            hal::arch::x86_64::console::_print(format_args!(
                "kernel: VMM bootstrap failed: {} (fallback cr3={:#x})\n",
                e, current_cr3
            ));
            (current_cr3, false)
        }
    };

    if let Err(e) = vmm::init(active_cr3) {
        hal::arch::x86_64::console::_print(format_args!("kernel: VMM init failed: {}\n", e));
        panic!("VMM: failed to initialize kernel virtual memory manager");
    }

    if !vmm_bootstrap_ok {
        heap::configure_identity_mode(Some(FALLBACK_IDENTITY_HEAP_MAX_PHYS));
    } else {
        heap::configure_identity_mode(None);
    }

    heap::init();

    kernel::timeline::init();
    kernel::timeline::mark("Boot");
    kernel::timeline::mark("Memory");

    if vmm_bootstrap_ok {
        console::attach_framebuffer(framebuffer_info);
    } else {
        console::attach_framebuffer_direct(framebuffer_info);
    }

    driver::console::init();
    console::set_serial_logging(true);
    kernel::timeline::mark("Heap");

    let fb_ready = console::promote_framebuffer_renderer();
    if fb_ready {
        console::println!("SAIOS kernel framebuffer online");
        console::println!("Starting kernel services...");
    }

    ksf::bootstrap().expect("KSF bootstrap failed");
    kernel::timeline::mark("Services");

    // Initialize ACPI subsystem
    if !vmm_bootstrap_ok {
        hal::arch::x86_64::console::_print(format_args!(
            "kernel: ACPI init skipped (VMM fallback mode)\n"
        ));
    } else if boot_info.acpi.rsdp != 0 {
        match kernel::acpi::init(boot_info.acpi.rsdp) {
            Ok(()) => {
                if let Some(acpi_mgr) = kernel::acpi::get_manager()
                    && let Ok((oem_id, revision)) = acpi_mgr.oem_info()
                {
                    hal::arch::x86_64::console::_print(format_args!(
                        "kernel: ACPI v{} initialized, OEM={}, processors={}\n",
                        revision,
                        oem_id,
                        acpi_mgr.processor_count()
                    ));
                    kernel::timeline::mark("ACPI");
                }
            }
            Err(e) => {
                hal::arch::x86_64::console::_print(format_args!(
                    "kernel: ACPI init failed: {}\n",
                    e
                ));
            }
        }
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
