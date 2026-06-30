#![no_std]
#![no_main]

pub mod driver;
pub mod seed;

use core::arch::asm;
use efi_main::SaiosBootInfo;
use hal::arch::x86_64::{gdt, idt, interrupt, tss};
use seed::Seed;

/// Size of the kernel stack (16 pages = 64 KiB).
const KERNEL_STACK_PAGES: usize = 16;
const PAGE_SIZE: usize = 4096;

/// A static kernel stack, placed in .bss so it is zero-initialised and
/// does not bloat the ELF file.
#[repr(C, align(4096))]
struct KernelStack([u8; KERNEL_STACK_PAGES * PAGE_SIZE]);

static KERNEL_STACK: KernelStack = KernelStack([0; KERNEL_STACK_PAGES * PAGE_SIZE]);

#[unsafe(no_mangle)]
/// # Safety
///
/// `boot_info` must be a valid pointer supplied by the SAIOS bootloader entry
/// contract and remain valid throughout early kernel initialization.
///
/// The caller (UEFI bootloader) must have already exited boot services.
/// Interrupts must be disabled on entry.
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    // ── 1. Disable interrupts immediately ──────────────────────────
    // After ExitBootServices the UEFI IDT is gone; any interrupt
    // (spurious timer IRQ, NMI, etc.) would jump through a garbage
    // vector and crash the kernel.
    interrupt::disable();

    // ── 2. Switch to our own kernel stack ──────────────────────────
    // The bootloader-provided stack is in UEFI LOADER_DATA memory
    // which we will eventually reclaim.  Switch to the static .bss
    // stack that lives inside the kernel image.
    let stack_top = KERNEL_STACK.0.as_ptr() as u64 + (KERNEL_STACK_PAGES * PAGE_SIZE) as u64;
    // x86_64 ABI requires rsp % 16 == 8 on function entry (call-compatible).
    let kernel_rsp = stack_top - 8;

    unsafe {
        asm!(
            "mov rsp, {}",
            in(reg) kernel_rsp,
            options(nostack),
        );
    }

    // ── 3. Install our own GDT, IDT, and TSS ─────────────────────
    // These replace the now-defunct UEFI firmware tables.
    gdt::init();
    idt::init();

    // Set the TSS kernel stack pointer (RSP0) so that interrupts
    // that ring-switch (e.g. from user mode in the future) land on
    // a known-good stack.
    tss::set_rsp0(stack_top);

    // ── 4. Initialize the serial console singleton ────────────────
    driver::console::init();

    // ── 5. Hand off to the Seed subsystem ─────────────────────────
    let seed = Seed::init(boot_info);
    seed.run()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    // Disable interrupts so we don't get a recursive panic.
    interrupt::disable();

    if let Some(loc) = info.location() {
        println!("KERNEL PANIC at {}:{}", loc.file(), loc.line());
    } else {
        println!("KERNEL PANIC");
    }

    if let Some(msg) = info.message().as_str() {
        println!("  {}", msg);
    }

    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}
