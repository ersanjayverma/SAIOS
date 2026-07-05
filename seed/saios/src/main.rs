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
use core::{
    arch::{asm, global_asm},
    mem::size_of,
};
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

const FALLBACK_IDENTITY_HEAP_MAX_PHYS: u64 = 0x0400_0000;
const LATE_CR3_MIN_SWITCH_PHYS: u64 = 0x0010_0000;
const EARLY_CR3_SWITCH_ENABLED: bool = true;

fn detect_nx_page_protection_policy() -> bool {
    let features = hal::arch::x86_64::cpuid::features();
    let mut efer_nxe = false;
    if features.nx && features.msr {
        let efer = hal::arch::x86_64::msr::rdmsr(0xC000_0080);
        efer_nxe = (efer & (1 << 11)) != 0;
    }
    features.nx && efer_nxe
}

#[derive(Copy, Clone)]
struct DescriptorTablePtr {
    limit: u16,
    base: u64,
}

fn read_rsp() -> u64 {
    let rsp: u64;
    unsafe {
        asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags));
    }
    rsp
}

fn read_rip() -> u64 {
    let rip: u64;
    unsafe {
        asm!("lea {}, [rip]", out(reg) rip, options(nostack, preserves_flags));
    }
    rip
}

fn read_idt_ptr() -> DescriptorTablePtr {
    let mut raw = [0u8; 10];
    unsafe {
        asm!("sidt [{}]", in(reg) raw.as_mut_ptr(), options(nostack, preserves_flags));
    }
    let limit = u16::from_le_bytes([raw[0], raw[1]]);
    let base = u64::from_le_bytes([
        raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8], raw[9],
    ]);
    DescriptorTablePtr { limit, base }
}

fn read_gdt_ptr() -> DescriptorTablePtr {
    let mut raw = [0u8; 10];
    unsafe {
        asm!("sgdt [{}]", in(reg) raw.as_mut_ptr(), options(nostack, preserves_flags));
    }
    let limit = u16::from_le_bytes([raw[0], raw[1]]);
    let base = u64::from_le_bytes([
        raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8], raw[9],
    ]);
    DescriptorTablePtr { limit, base }
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
        asm!("", in("rax") stack_word, options(nomem, nostack, preserves_flags));
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

    let nx_enabled = detect_nx_page_protection_policy();
    vmm::set_nx_page_protection_enabled(nx_enabled);

    let (prepared_kernel_pml4, mut active_cr3, mut vmm_bootstrap_ok) =
        match vmm::bootstrap_kernel_page_tables(
            framebuffer_info.base,
            framebuffer_info.size,
            boot_info_ptr,
            boot_info_size,
            kernel_start,
            kernel_end,
        ) {
            Ok(pml4) => {
                let current_cr3 = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;
                (Some(pml4), current_cr3, false)
            }
            Err(e) => {
                // Fallback path for firmware that cannot tolerate early CR3/bootstrap assumptions.
                let current_cr3 = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;
                hal::arch::x86_64::console::_print(format_args!(
                    "kernel: VMM bootstrap failed: {} (fallback cr3={:#x})\n",
                    e, current_cr3
                ));
                (None, current_cr3, false)
            }
        };

    if EARLY_CR3_SWITCH_ENABLED && !vmm_bootstrap_ok {
        if let Some(kernel_pml4) = prepared_kernel_pml4 {
            let switch_target = kernel_pml4 & 0x000F_FFFF_FFFF_F000;
            let rip = read_rip();
            let rsp = read_rsp();
            let idtr = read_idt_ptr();
            let gdtr = read_gdt_ptr();
            let idt_base = idtr.base;
            let gdt_base = gdtr.base;
            let idt_limit = idtr.limit as u64;
            let gdt_limit = gdtr.limit as u64;
            let idt_tail = idt_base.saturating_add(idt_limit);
            let gdt_tail = gdt_base.saturating_add(gdt_limit);
            let mut preflight_ok = true;
            let mut first_missing: Option<&'static str> = None;
            match vmm::validate_prepared_kernel_pml4(switch_target) {
                Ok(()) => {}
                Err(_e) => {
                    preflight_ok = false;
                    first_missing = Some("pml4_root");
                }
            }
            let rip_next = rip.saturating_add(0x1000);
            let rsp_write_8 = rsp.saturating_sub(8);
            let rsp_guard_4k = rsp.saturating_sub(0x1000);
            let rsp_guard_16k = rsp.saturating_sub(0x4000);
            let rsp_guard_64k = rsp.saturating_sub(0x10000);
            let alloc_marker = &GLOBAL_ALLOCATOR as *const _ as u64;
            let kernel_hh_start = vmm::KERNEL_IMAGE_MIRROR_BASE.saturating_add(kernel_start);
            let kernel_hh_tail =
                vmm::KERNEL_IMAGE_MIRROR_BASE.saturating_add(kernel_end.saturating_sub(1));
            let required_points = [
                ("rip", rip),
                ("rip+0x1000", rip_next),
                ("rsp", rsp),
                ("rsp-8", rsp_write_8),
                ("rsp-0x1000", rsp_guard_4k),
                ("rsp-0x4000", rsp_guard_16k),
                ("rsp-0x10000", rsp_guard_64k),
                ("idt", idt_base),
                ("idt_end", idt_tail),
                ("gdt", gdt_base),
                ("gdt_end", gdt_tail),
                ("boot_info", boot_info_ptr),
                ("fb", framebuffer_info.base),
                ("kernel_start", kernel_start),
                ("kernel_hh_start", kernel_hh_start),
                ("kernel_hh_end", kernel_hh_tail),
                ("alloc_marker", alloc_marker),
            ];

            for (label, addr) in required_points {
                match vmm::is_mapped_in_page_tables(switch_target, addr) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        preflight_ok = false;
                        if first_missing.is_none() {
                            first_missing = Some(label);
                        }
                    }
                }
            }

            if kernel_end > kernel_start {
                let kernel_tail = kernel_end.saturating_sub(1);
                match vmm::is_mapped_in_page_tables(switch_target, kernel_tail) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        preflight_ok = false;
                        if first_missing.is_none() {
                            first_missing = Some("kernel_end-1");
                        }
                    }
                }
            }

            if boot_info_size > 0 {
                let boot_info_tail = boot_info_ptr
                    .saturating_add(boot_info_size as u64)
                    .saturating_sub(1);
                match vmm::is_mapped_in_page_tables(switch_target, boot_info_tail) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        preflight_ok = false;
                        if first_missing.is_none() {
                            first_missing = Some("boot_info_end-1");
                        }
                    }
                }
            }

            if framebuffer_info.size > 0 {
                let fb_tail = framebuffer_info
                    .base
                    .saturating_add(framebuffer_info.size as u64)
                    .saturating_sub(1);
                match vmm::is_mapped_in_page_tables(switch_target, fb_tail) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        preflight_ok = false;
                        if first_missing.is_none() {
                            first_missing = Some("fb_end-1");
                        }
                    }
                }
            }

            if switch_target < LATE_CR3_MIN_SWITCH_PHYS {
                hal::arch::x86_64::console::_print(format_args!(
                    "kernel: early CR3 switch skipped (target below safety floor: {:#x} < {:#x})\n",
                    switch_target, LATE_CR3_MIN_SWITCH_PHYS
                ));
            } else if !preflight_ok {
                hal::arch::x86_64::console::_print(format_args!(
                    "kernel: early CR3 preflight failed ({})\n",
                    first_missing.unwrap_or("unknown")
                ));
            } else {
                match vmm::activate_kernel_page_tables(switch_target) {
                    Ok(()) => {
                        late_cr3_smoke_test();
                        active_cr3 = switch_target;
                        vmm_bootstrap_ok = true;
                        hal::arch::x86_64::console::_print(format_args!(
                            "kernel: early CR3 switch succeeded (cr3={:#x})\n",
                            switch_target
                        ));
                    }
                    Err(e) => {
                        let current_cr3 = paging::read_cr3() & 0x000F_FFFF_FFFF_F000;
                        hal::arch::x86_64::console::_print(format_args!(
                            "kernel: early CR3 switch failed: {} (fallback cr3={:#x})\n",
                            e, current_cr3
                        ));
                    }
                }
            }
        }
    }

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
    if boot_info.acpi.rsdp != 0 {
        if !vmm_bootstrap_ok {
            console::println!("kernel: ACPI init running in fallback mode");
        }
        match kernel::acpi::init(boot_info.acpi.rsdp) {
            Ok(()) => {
                if let Some(acpi_mgr) = kernel::acpi::get_manager()
                    && let Ok((oem_id, revision)) = acpi_mgr.oem_info()
                {
                    console::println!(
                        "kernel: ACPI v{} initialized, OEM={}, processors={}",
                        revision,
                        oem_id,
                        acpi_mgr.processor_count()
                    );
                    kernel::timeline::mark("ACPI");
                }
            }
            Err(e) => {
                console::println!("kernel: ACPI init failed: {}", e);
            }
        }
    } else {
        console::println!("kernel: ACPI init skipped (no RSDP)");
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
