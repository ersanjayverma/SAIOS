use core::arch::asm;
pub fn enable() {
    unsafe {
        asm!("sti", options(nomem, nostack));
    }
}

pub fn disable() {
    unsafe {
        asm!("cli", options(nomem, nostack));
    }
}

pub fn enabled() -> bool {
    let flags: u64;
    unsafe {
        asm!("pushf", out("rax") flags, options(nomem, nostack));
    }
    (flags & (1 << 9)) != 0
}

pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let was_enabled = enabled();
    disable();
    let result = f();
    if was_enabled {
        enable();
    }
    result
}
