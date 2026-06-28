pub fn freeze_other_cpus() {
    // Best effort for now: prevent further local CPU interrupts.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

pub fn halt_forever() -> ! {
    loop {
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
