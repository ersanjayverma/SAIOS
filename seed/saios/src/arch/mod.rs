mod interrupts;

pub fn init() {}

#[inline]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

pub fn install_exception_handlers() {
    interrupts::install_exception_handlers();
}
