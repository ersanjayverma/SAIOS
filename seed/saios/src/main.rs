#![no_std]
#![no_main]

pub mod driver;
pub mod seed;

use efi_main::SaiosBootInfo;
use hal::arch::x86_64::{gdt, idt, interrupt};
use seed::Seed;

#[unsafe(no_mangle)]
/// # Safety
///
/// `boot_info` must be a valid pointer supplied by the SAIOS bootloader entry
/// contract and remain valid throughout early kernel initialization.
///
/// The caller (UEFI bootloader) must have already exited boot services.
/// The bootloader provides a valid 64 KiB stack — we keep using it until
/// the kernel has its own memory manager.
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    // ── 1. Disable interrupts immediately ──────────────────────────
    interrupt::disable();

    // ── 2. Install our own GDT, IDT, and TSS ─────────────────────
    gdt::init();
    idt::init();

    // ── 3. Initialize the serial console singleton ────────────────
    driver::console::init();

    // ── 4. Hand off to the Seed subsystem ─────────────────────────
    let seed = Seed::init(boot_info);
    seed.run()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
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
