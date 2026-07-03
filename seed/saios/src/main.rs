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
use efi_main::SaiosBootInfo;
use graphics::display::FramebufferDisplay;
use hal::arch::paging;
use hal::arch::x86_64::{gdt, idt, interrupt};
use seed::Seed;

const KERNEL_SERIAL_LOGGING_ENABLED: bool = false;

const STAGE_KERNEL_ENTRY: u32 = 0x0030_0000;
const STAGE_MEMORY_READY: u32 = 0x0030_1800;
const STAGE_HEAP_READY: u32 = 0x0030_3030;
const STAGE_FB_ATTACHED: u32 = 0x0000_3040;
const STAGE_KSF_READY: u32 = 0x0000_2040;
const STAGE_BOOT_READY: u32 = 0x0000_0030;

#[global_allocator]
static GLOBAL_ALLOCATOR: heap::KernelHeapAllocator = heap::KernelHeapAllocator;

fn mark_boot_stage(framebuffer_info: efi_main::graphics::FramebufferInfo, color: u32) {
    if let Some(mut display) = FramebufferDisplay::from_info(framebuffer_info) {
        display.clear_color(color);
    }
}

/// # Safety
///
/// This is the kernel entry point invoked by the bootloader. `boot_info` must
/// be a valid, properly aligned pointer to a `SaiosBootInfo` structure that
/// remains valid for the lifetime of the kernel. Must be called exactly once
/// with interrupts in a defined state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    hal::arch::x86_64::console::set_output_enabled(KERNEL_SERIAL_LOGGING_ENABLED);
    hal::arch::x86_64::console::_print(format_args!("kernel: _start enter\n"));
    interrupt::disable();
    let boot_info = unsafe { &*boot_info };
    let framebuffer_info = boot_info.framebuffer;
    mark_boot_stage(framebuffer_info, STAGE_KERNEL_ENTRY);
    hal::arch::x86_64::console::_print(format_args!(
        "kernel: boot_info map_entries={} fb_base={:#x}\n",
        boot_info.memorymap.entry_count, framebuffer_info.base,
    ));
    gdt::init();
    idt::init();
    hal::arch::x86_64::console::_print(format_args!("kernel: gdt+idt ok\n"));

    // Convert the raw pointer and count into a temporary Rust slice
    let _entries_slice = unsafe {
        core::slice::from_raw_parts(boot_info.memorymap.entries, boot_info.memorymap.entry_count)
    };
    hal::arch::x86_64::console::_print(format_args!("kernel: memory slice ok\n"));
    // Initialize PMM with the boot memory map.
    pmm::init(_entries_slice);
    hal::arch::x86_64::console::_print(format_args!("kernel: pmm init ok\n"));
    mark_boot_stage(framebuffer_info, STAGE_MEMORY_READY);

    // Use the active CR3 root as VMM root; do not dereference raw physical pointers here.
    let pml4_phys = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;

    vmm::init(pml4_phys).expect("VMM: failed to initialize kernel virtual memory manager");
    hal::arch::x86_64::console::_print(format_args!("kernel: vmm init ok\n"));

    heap::init();
    hal::arch::x86_64::console::_print(format_args!("kernel: heap init ok\n"));
    mark_boot_stage(framebuffer_info, STAGE_HEAP_READY);
    hal::arch::x86_64::console::_print(format_args!(
        "kernel: fb info base={:#x} size={} {}x{} stride={} bpp={} fmt={:?} masks=({:#x},{:#x},{:#x},{:#x})\n",
        framebuffer_info.base,
        framebuffer_info.size,
        framebuffer_info.width,
        framebuffer_info.height,
        framebuffer_info.stride,
        framebuffer_info.bpp,
        framebuffer_info.pixel_format,
        framebuffer_info.red_mask,
        framebuffer_info.green_mask,
        framebuffer_info.blue_mask,
        framebuffer_info.reserved_mask,
    ));
    kernel::timeline::init();
    kernel::timeline::mark("Boot");
    kernel::timeline::mark("Memory");
    // Attach framebuffer using bootloader-provided address.
    hal::arch::x86_64::console::_print(format_args!("kernel: fb attach begin\n"));
    console::attach_framebuffer(framebuffer_info);
    hal::arch::x86_64::console::_print(format_args!("kernel: fb attach done\n"));
    driver::console::init();
    console::set_serial_logging(KERNEL_SERIAL_LOGGING_ENABLED);
    kernel::timeline::mark("Heap");
    let fb_ready = console::promote_framebuffer_renderer();
    hal::arch::x86_64::console::_print(format_args!("kernel: fb renderer ready={}\n", fb_ready));
    if fb_ready {
        mark_boot_stage(framebuffer_info, STAGE_FB_ATTACHED);
    }
    if let Some(props) = console::framebuffer_properties() {
        hal::arch::x86_64::console::_print(format_args!(
            "kernel: fb props {}x{} stride={} bpp={} fmt={:?} size={}\n",
            props.width,
            props.height,
            props.stride,
            props.bytes_per_pixel,
            props.pixel_format,
            props.framebuffer_size,
        ));
    } else {
        hal::arch::x86_64::console::_print(format_args!("kernel: fb props unavailable\n"));
    }
    if fb_ready {
        console::println!("SAIOS kernel framebuffer online");
        console::println!("Starting kernel services...");
    }
    ksf::bootstrap().expect("KSF bootstrap failed");
    mark_boot_stage(framebuffer_info, STAGE_KSF_READY);
    kernel::timeline::mark("Services");

    // Initialize ACPI subsystem
    if boot_info.acpi.rsdp != 0 {
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
    } else {
        hal::arch::x86_64::console::_print(format_args!("kernel: No ACPI RSDP found\n"));
    }

    interrupt::enable();
    if cfg!(debug_assertions) {
        kernel::testing::boot_self_test();
    }

    let seed = Seed::init(boot_info as *const SaiosBootInfo);
    kernel::timeline::mark("Ready");
    mark_boot_stage(framebuffer_info, STAGE_BOOT_READY);
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
