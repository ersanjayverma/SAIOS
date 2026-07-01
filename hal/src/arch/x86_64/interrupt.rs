use core::arch::asm;

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

    (rflags & (1 << 9)) != 0
}

pub fn without_interrupts<F: FnOnce()>(f: F) {
    let enabled = are_enabled();

    if enabled {
        disable();
    }

    f();

    if enabled {
        enable();
    }
}

#[inline(always)]
pub fn int3() {
    unsafe {
        asm!("int3", options(nomem, nostack));
    }
}
