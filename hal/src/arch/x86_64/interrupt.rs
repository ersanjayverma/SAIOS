//! Interrupt flag manipulation helpers for x86_64.

use core::arch::asm;

const RFLAGS_INTERRUPT_ENABLE_BIT: u64 = 9;

#[inline(always)]
pub fn enable() {
    unsafe {
        asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
pub fn disable() {
    unsafe {
        asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
pub fn are_enabled() -> bool {
    let rflags: u64;

    unsafe {
        asm!(
            "pushfq",
            "pop {}",
            out(reg) rflags,
            options(nomem, preserves_flags),
        );
    }

    (rflags & (1 << RFLAGS_INTERRUPT_ENABLE_BIT)) != 0
}

pub fn without_interrupts<F: FnOnce() -> R, R>(f: F) -> R {
    let enabled = are_enabled();

    if enabled {
        disable();
    }

    let out = f();

    if enabled {
        enable();
    }

    out
}

#[inline(always)]
pub fn int3() {
    unsafe {
        asm!("int3", options(nomem, nostack));
    }
}
