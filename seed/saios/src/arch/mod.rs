mod interrupts;
pub mod x86_64;

pub fn init() {}

#[inline]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

/// Zero the .bss section
/// This is necessary because some UEFI bootloaders don't properly zero .bss
/// The linker script defines _bss_start and _kernel_end symbols for this purpose.
#[inline]
pub unsafe fn zero_bss() {
    // extern blocks must be marked unsafe in Rust 2024+
    unsafe extern "C" {
        static _bss_start: u8;
        static _kernel_end: u8;
    }

    let bss_start: *const u8 = unsafe { &_bss_start };
    let bss_end: *const u8 = unsafe { &_kernel_end };
    let len = unsafe { bss_end.offset_from(bss_start) } as usize;

    if len > 0 {
        // Zero .bss using slice fill to ensure all bytes are cleared
        unsafe { core::slice::from_raw_parts_mut(bss_start as *mut u8, len).fill(0) };
    }
}

pub fn install_exception_handlers() {
    interrupts::install_exception_handlers();
}
