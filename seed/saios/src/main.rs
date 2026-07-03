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
use hal::arch::x86_64::{cpuid, gdt, idt, interrupt, msr};
use seed::Seed;

const KERNEL_SERIAL_LOGGING_ENABLED: bool = false;
const BOOT_STAGE_COLOR_DIAGNOSTICS: bool = false;
const LATE_MICROCODE_PROBE_ENABLED: bool = true;

const STAGE_KERNEL_ENTRY: u32 = 0x00FF_0000;
const STAGE_BOOTINFO_OK: u32 = 0x00FF_4000;
const STAGE_GDT_OK: u32 = 0x00FF_A000;
const STAGE_IDT_OK: u32 = 0x00FF_FF00;
const STAGE_MICROCODE_OK: u32 = 0x0000_FFFF;
const STAGE_MAP_SLICE_OK: u32 = 0x00FF_00FF;
const STAGE_PMM_OK: u32 = 0x0000_FF00;
const STAGE_VMM_OK: u32 = 0x0000_80FF;
const STAGE_MEMORY_READY: u32 = 0x0000_FF00;
const STAGE_HEAP_READY: u32 = 0x0000_00FF;
const STAGE_FB_ATTACHED: u32 = 0x00FF_FF00;
const STAGE_KSF_READY: u32 = 0x0000_FFFF;
const STAGE_BOOT_READY: u32 = 0x00FF_FFFF;

#[inline(always)]
fn kernel_trace_byte(marker: u8) {
    // Raw COM1 marker that works even before higher-level console setup.
    hal::arch::x86_64::io::outb(0x3F8, marker);
}

#[global_allocator]
static GLOBAL_ALLOCATOR: heap::KernelHeapAllocator = heap::KernelHeapAllocator;

fn mark_boot_stage(framebuffer_info: efi_main::graphics::FramebufferInfo, color: u32) {
    if !BOOT_STAGE_COLOR_DIAGNOSTICS {
        return;
    }
    if let Some(mut display) = FramebufferDisplay::from_info(framebuffer_info) {
        display.clear_color(color);
    }
}

fn detect_microcode_revision() -> Result<u32, &'static str> {
    const CPUID_FEATURES_LEAF: u32 = 0x0000_0001;
    const IA32_BIOS_SIGN_ID: u32 = 0x0000_008B;

    let features = cpuid::features();
    if !features.msr {
        return Err("MSR unsupported");
    }

    let (_, _, ecx, _) = cpuid::cpuid(CPUID_FEATURES_LEAF);
    if (ecx & (1 << 31)) != 0 {
        return Err("running under hypervisor");
    }

    // Latch current microcode revision into IA32_BIOS_SIGN_ID.
    msr::wrmsr(IA32_BIOS_SIGN_ID, 0);
    let _ = cpuid::cpuid(CPUID_FEATURES_LEAF);
    Ok((msr::rdmsr(IA32_BIOS_SIGN_ID) >> 32) as u32)
}

/// # Safety
///
/// This is the kernel entry point invoked by the bootloader. `boot_info` must
/// be a valid, properly aligned pointer to a `SaiosBootInfo` structure that
/// remains valid for the lifetime of the kernel. Must be called exactly once
/// with interrupts in a defined state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    // Bring up UART immediately so post-ExitBootServices progress is always visible.
    hal::arch::x86_64::console::init_serial();
    kernel_trace_byte(b'K');
    hal::arch::x86_64::console::set_output_enabled(KERNEL_SERIAL_LOGGING_ENABLED);
    hal::arch::x86_64::console::_print(format_args!("kernel: _start enter\n"));
    interrupt::disable();
    kernel_trace_byte(b'0');
    let boot_info = unsafe { &*boot_info };
    let framebuffer_info = boot_info.framebuffer;
    mark_boot_stage(framebuffer_info, STAGE_KERNEL_ENTRY);
    hal::arch::x86_64::console::_print(format_args!(
        "kernel: boot_info map_entries={} fb_base={:#x}\n",
        boot_info.memorymap.entry_count, framebuffer_info.base,
    ));
    mark_boot_stage(framebuffer_info, STAGE_BOOTINFO_OK);
    gdt::init();
    mark_boot_stage(framebuffer_info, STAGE_GDT_OK);
    idt::init();
    mark_boot_stage(framebuffer_info, STAGE_IDT_OK);
    hal::arch::x86_64::console::_print(format_args!("kernel: gdt+idt ok\n"));

    // Convert the raw pointer and count into a temporary Rust slice
    let _entries_slice = unsafe {
        core::slice::from_raw_parts(boot_info.memorymap.entries, boot_info.memorymap.entry_count)
    };
    mark_boot_stage(framebuffer_info, STAGE_MAP_SLICE_OK);
    hal::arch::x86_64::console::_print(format_args!("kernel: memory slice ok\n"));
    // Initialize PMM with the boot memory map.
    pmm::init(_entries_slice);
    mark_boot_stage(framebuffer_info, STAGE_PMM_OK);
    hal::arch::x86_64::console::_print(format_args!("kernel: pmm init ok\n"));
    mark_boot_stage(framebuffer_info, STAGE_MEMORY_READY);

    // Use the active CR3 root as VMM root; do not dereference raw physical pointers here.
    let pml4_phys = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;

    vmm::init(pml4_phys).expect("VMM: failed to initialize kernel virtual memory manager");
    mark_boot_stage(framebuffer_info, STAGE_VMM_OK);
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

    if LATE_MICROCODE_PROBE_ENABLED {
        match detect_microcode_revision() {
            Ok(revision) => hal::arch::x86_64::console::_print(format_args!(
                "kernel: cpu microcode revision={:#x}\n",
                revision
            )),
            Err(reason) => hal::arch::x86_64::console::_print(format_args!(
                "kernel: cpu microcode revision unavailable ({})\n",
                reason
            )),
        }
        mark_boot_stage(framebuffer_info, STAGE_MICROCODE_OK);
    }

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
