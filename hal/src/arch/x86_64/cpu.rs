use core::arch::asm;

#[inline(always)]
pub fn hlt() {
    unsafe {
        asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
pub fn pause() {
    unsafe {
        asm!("pause", options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
pub fn nop() {
    unsafe {
        asm!("nop", options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
pub fn rdtsc() -> u64 {
    let low: u32;
    let high: u32;

    unsafe {
        asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack)
        );
    }

    ((high as u64) << 32) | (low as u64)
}
