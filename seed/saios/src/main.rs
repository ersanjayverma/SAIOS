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
use graphics::display::FramebufferDisplay;
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

const BOOT_STAGE_COLOR_DIAGNOSTICS: bool = false;
const EARLY_CR3_SWITCH_ENABLED: bool = false;
const FALLBACK_IDENTITY_HEAP_MAX_PHYS: u64 = 0x0400_0000;

const KERNEL_FB_TRACE_CELL_W: usize = 12;
const KERNEL_FB_TRACE_CELL_H: usize = 12;
const KERNEL_FB_TRACE_SPACING: usize = 2;
const KERNEL_FB_TRACE_ORIGIN_X: usize = 8;
const KERNEL_FB_TRACE_ORIGIN_Y: usize = 28;
const KERNEL_FB_STATUS_BAND_H: usize = 20;

const STAGE_KERNEL_ENTRY: u32 = 0x00FF_0000;
const STAGE_BOOTINFO_OK: u32 = 0x00FF_4000;
const STAGE_GDT_OK: u32 = 0x00FF_A000;
const STAGE_IDT_OK: u32 = 0x00FF_FF00;
const STAGE_MAP_SLICE_OK: u32 = 0x00FF_00FF;
const STAGE_PMM_OK: u32 = 0x0000_FF00;
const STAGE_VMM_OK: u32 = 0x0000_80FF;
const STAGE_MEMORY_READY: u32 = 0x0000_FF00;
const STAGE_HEAP_READY: u32 = 0x0000_00FF;
const STAGE_FB_ATTACHED: u32 = 0x00FF_FF00;
const STAGE_KSF_READY: u32 = 0x0000_FFFF;
const STAGE_BOOT_READY: u32 = 0x00FF_FFFF;

#[inline(always)]
fn pack_channel(value: u8, mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }

    let shift = mask.trailing_zeros();
    let width = mask.count_ones();
    if width == 0 {
        return 0;
    }

    let max = (1u32 << width) - 1;
    let scaled = ((value as u32) * max + 127) / 255;
    (scaled << shift) & mask
}

#[inline(always)]
fn pack_fb_color(info: efi_main::graphics::FramebufferInfo, rgb: u32) -> u32 {
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;

    match info.pixel_format {
        efi_main::graphics::PixelFormat::Bgr => {
            (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | (0xFF << 24)
        }
        efi_main::graphics::PixelFormat::Rgb => {
            (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | (0xFF << 24)
        }
        efi_main::graphics::PixelFormat::Bitmask => {
            pack_channel(r, info.red_mask)
                | pack_channel(g, info.green_mask)
                | pack_channel(b, info.blue_mask)
                | info.reserved_mask
        }
        efi_main::graphics::PixelFormat::BltOnly => {
            (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | (0xFF << 24)
        }
    }
}

#[inline(always)]
fn kernel_stage_color(stage: u8) -> u32 {
    const PALETTE: [u32; 16] = [
        0x00FF1111, 0x00FF6A00, 0x00FFD100, 0x0094E000, 0x0000C96E, 0x00009EEB, 0x003C63FF,
        0x006D3CFF, 0x00C700B7, 0x00FFFFFF, 0x006E6E6E, 0x0066FF66, 0x00FF7AB6, 0x005DCBFF,
        0x00FFC66B, 0x00C7FF4C,
    ];

    PALETTE[(stage as usize) & 0x0F]
}

fn kernel_fb_trace_mark(info: efi_main::graphics::FramebufferInfo, stage: u8) {
    if info.base == 0 || info.width == 0 || info.height == 0 || info.stride == 0 || info.size == 0 {
        return;
    }

    let bytes_per_pixel = core::cmp::max(info.bpp.div_ceil(8), 1);
    if bytes_per_pixel > 4 {
        return;
    }

    let marker = stage as usize;
    let step_x = KERNEL_FB_TRACE_CELL_W + KERNEL_FB_TRACE_SPACING;
    let columns = core::cmp::max(1, (info.width.saturating_sub(KERNEL_FB_TRACE_ORIGIN_X)) / step_x);
    let x0 = KERNEL_FB_TRACE_ORIGIN_X + (marker % columns) * step_x;
    let y0 = KERNEL_FB_TRACE_ORIGIN_Y
        + (marker / columns) * (KERNEL_FB_TRACE_CELL_H + KERNEL_FB_TRACE_SPACING);
    if x0 >= info.width || y0 >= info.height {
        return;
    }

    let x1 = core::cmp::min(info.width, x0 + KERNEL_FB_TRACE_CELL_W);
    let y1 = core::cmp::min(info.height, y0 + KERNEL_FB_TRACE_CELL_H);
    let packed = pack_fb_color(info, kernel_stage_color(stage));
    let packed_bytes = packed.to_le_bytes();
    let fb = info.base as *mut u8;

    for y in y0..y1 {
        let row = y.saturating_mul(info.stride);
        for x in x0..x1 {
            let offset = (row + x).saturating_mul(bytes_per_pixel);
            if offset + bytes_per_pixel > info.size {
                continue;
            }

            unsafe {
                let dst = fb.add(offset);
                let mut i = 0;
                while i < bytes_per_pixel {
                    core::ptr::write(dst.add(i), packed_bytes[i]);
                    i += 1;
                }
            }
        }
    }
}

fn kernel_fb_status_band(info: efi_main::graphics::FramebufferInfo, rgb: u32) {
    if info.base == 0 || info.width == 0 || info.height == 0 || info.stride == 0 || info.size == 0 {
        return;
    }

    let bytes_per_pixel = core::cmp::max(info.bpp.div_ceil(8), 1);
    if bytes_per_pixel > 4 {
        return;
    }

    let band_h = core::cmp::min(KERNEL_FB_STATUS_BAND_H, info.height);
    let y0 = info.height.saturating_sub(band_h);
    let packed = pack_fb_color(info, rgb);
    let packed_bytes = packed.to_le_bytes();
    let fb = info.base as *mut u8;

    for y in y0..info.height {
        let row = y.saturating_mul(info.stride);
        for x in 0..info.width {
            let offset = (row + x).saturating_mul(bytes_per_pixel);
            if offset + bytes_per_pixel > info.size {
                continue;
            }

            unsafe {
                let dst = fb.add(offset);
                let mut i = 0;
                while i < bytes_per_pixel {
                    core::ptr::write(dst.add(i), packed_bytes[i]);
                    i += 1;
                }
            }
        }
    }
}

#[inline(always)]
fn fallback_status(framebuffer_info: efi_main::graphics::FramebufferInfo, stage: u8, rgb: u32) {
    kernel_fb_trace_mark(framebuffer_info, stage);
    kernel_fb_status_band(framebuffer_info, rgb);
}

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
    kernel_fb_trace_mark(framebuffer_info, 0);

    // Bring up UART immediately so post-ExitBootServices progress is always visible.
    hal::arch::x86_64::console::init_serial();
    kernel_trace_byte(b'R');
    hal::arch::x86_64::console::set_output_enabled(true);
    hal::arch::x86_64::console::_print(format_args!("kernel: _start enter\n"));
    interrupt::disable();
    kernel_trace_byte(b'0');
    kernel_fb_trace_mark(framebuffer_info, 1);
    mark_boot_stage(framebuffer_info, STAGE_KERNEL_ENTRY);
    hal::arch::x86_64::console::_print(format_args!(
        "kernel: boot_info map_entries={} fb_base={:#x}\n",
        boot_info.memorymap.entry_count, framebuffer_info.base,
    ));
    mark_boot_stage(framebuffer_info, STAGE_BOOTINFO_OK);
    kernel_fb_trace_mark(framebuffer_info, 2);
    gdt::init();
    mark_boot_stage(framebuffer_info, STAGE_GDT_OK);
    kernel_fb_trace_mark(framebuffer_info, 3);
    idt::init();
    mark_boot_stage(framebuffer_info, STAGE_IDT_OK);
    kernel_fb_trace_mark(framebuffer_info, 4);
    hal::arch::x86_64::console::_print(format_args!("kernel: gdt+idt ok\n"));

    // Convert the raw pointer and count into a temporary Rust slice
    let _entries_slice = unsafe {
        core::slice::from_raw_parts(boot_info.memorymap.entries, boot_info.memorymap.entry_count)
    };
    mark_boot_stage(framebuffer_info, STAGE_MAP_SLICE_OK);
    kernel_fb_trace_mark(framebuffer_info, 5);
    hal::arch::x86_64::console::_print(format_args!("kernel: memory slice ok\n"));
    // Initialize PMM with the boot memory map.
    pmm::init(_entries_slice);
    mark_boot_stage(framebuffer_info, STAGE_PMM_OK);
    kernel_fb_trace_mark(framebuffer_info, 6);
    hal::arch::x86_64::console::_print(format_args!("kernel: pmm init ok\n"));
    mark_boot_stage(framebuffer_info, STAGE_MEMORY_READY);

    let kernel_start = unsafe { &_kernel_start as *const u8 as u64 };
    let kernel_end = unsafe { &_kernel_end as *const u8 as u64 };
    let boot_info_ptr = boot_info as *const SaiosBootInfo as u64;
    let boot_info_size = size_of::<SaiosBootInfo>();

    kernel_fb_trace_mark(framebuffer_info, 13);

    let (active_cr3, vmm_bootstrap_ok) = match vmm::bootstrap_kernel_page_tables(
        framebuffer_info.base,
        framebuffer_info.size,
        boot_info_ptr,
        boot_info_size,
        kernel_start,
        kernel_end,
    ) {
        Ok(pml4) => {
            kernel_fb_trace_mark(framebuffer_info, 14);
            if EARLY_CR3_SWITCH_ENABLED {
                if let Err(e) = vmm::activate_kernel_page_tables(pml4) {
                    let current_cr3 = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;
                    hal::arch::x86_64::console::_print(format_args!(
                        "kernel: CR3 switch failed: {} (fallback cr3={:#x})\n",
                        e, current_cr3
                    ));
                    kernel_fb_trace_mark(framebuffer_info, 30);
                    (current_cr3, false)
                } else {
                    (pml4, true)
                }
            } else {
                let current_cr3 = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;
                hal::arch::x86_64::console::_print(format_args!(
                    "kernel: early CR3 switch disabled (using firmware cr3={:#x})\n",
                    current_cr3
                ));
                fallback_status(framebuffer_info, 30, 0x009A5A2A);
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
            fallback_status(framebuffer_info, 30, 0x009A5A2A);
            (current_cr3, false)
        }
    };

    if let Err(e) = vmm::init(active_cr3) {
        hal::arch::x86_64::console::_print(format_args!("kernel: VMM init failed: {}\n", e));
        fallback_status(framebuffer_info, 31, 0x00BFFF00);
        panic!("VMM: failed to initialize kernel virtual memory manager");
    }
    kernel_fb_trace_mark(framebuffer_info, 15);
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 32, 0x00FF0000);
        fallback_status(framebuffer_info, 38, 0x000040FF);
    }
    mark_boot_stage(framebuffer_info, STAGE_VMM_OK);
    kernel_fb_trace_mark(framebuffer_info, 7);
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 39, 0x007000FF);
    }
    hal::arch::x86_64::console::_print(format_args!("kernel: vmm init ok cr3={:#x}\n", active_cr3));

    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 40, 0x00FF00FF);
        heap::configure_identity_mode(Some(FALLBACK_IDENTITY_HEAP_MAX_PHYS));
        fallback_status(framebuffer_info, 41, 0x00FFFFFF);
    } else {
        heap::configure_identity_mode(None);
    }
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 42, 0x00808080);
    }
    heap::init();
    hal::arch::x86_64::console::_print(format_args!("kernel: heap init ok\n"));
    mark_boot_stage(framebuffer_info, STAGE_HEAP_READY);
    kernel_fb_trace_mark(framebuffer_info, 8);
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 33, 0x00FF8800);
    }
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 43, 0x00603000);
    }
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
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 44, 0x00805020);
    }
    kernel::timeline::init();
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 45, 0x00A07020);
    }
    kernel::timeline::mark("Boot");
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 46, 0x00C09020);
    }
    kernel::timeline::mark("Memory");
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 47, 0x00E0B020);
    }
    // Attach framebuffer using bootloader-provided address.
    hal::arch::x86_64::console::_print(format_args!("kernel: fb attach begin\n"));
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 48, 0x00FFD040);
    }
    if vmm_bootstrap_ok {
        console::attach_framebuffer(framebuffer_info);
    } else {
        console::attach_framebuffer_direct(framebuffer_info);
    }
    hal::arch::x86_64::console::_print(format_args!("kernel: fb attach done\n"));
    kernel_fb_trace_mark(framebuffer_info, 9);
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 34, 0x00FFD000);
    }
    driver::console::init();
    console::set_serial_logging(true);
    kernel::timeline::mark("Heap");
    let fb_ready = console::promote_framebuffer_renderer();
    hal::arch::x86_64::console::_print(format_args!("kernel: fb renderer ready={}\n", fb_ready));
    if fb_ready {
        mark_boot_stage(framebuffer_info, STAGE_FB_ATTACHED);
    }
    kernel_fb_trace_mark(framebuffer_info, 10);
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 35, 0x0000FF00);
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
    kernel_fb_trace_mark(framebuffer_info, 11);
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 36, 0x0000FFFF);
    }
    kernel::timeline::mark("Services");

    // Initialize ACPI subsystem
    if !vmm_bootstrap_ok {
        hal::arch::x86_64::console::_print(format_args!(
            "kernel: ACPI init skipped (VMM fallback mode)\n"
        ));
    } else if boot_info.acpi.rsdp != 0 {
        hal::arch::x86_64::console::_print(format_args!(
            "kernel: ACPI init begin rsdp={:#x}\n",
            boot_info.acpi.rsdp
        ));
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
        hal::arch::x86_64::console::_print(format_args!("kernel: ACPI init end\n"));
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
    kernel_fb_trace_mark(framebuffer_info, 12);
    if !vmm_bootstrap_ok {
        fallback_status(framebuffer_info, 37, 0x00FF80C0);
    }
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
