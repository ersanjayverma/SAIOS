//! Minimal CPU instruction wrappers for x86_64.

use core::arch::asm;

#[derive(Copy, Clone, Debug)]
pub struct DescriptorTablePtr {
    pub limit: u16,
    pub base: u64,
}

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

#[inline(always)]
pub fn read_cr2() -> u64 {
    let value: u64;

    unsafe {
        asm!("mov {}, cr2", out(reg) value, options(nomem, nostack, preserves_flags));
    }

    value
}

#[inline(always)]
pub fn read_rsp() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, rsp", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

#[inline(always)]
pub fn read_rip() -> u64 {
    let value: u64;
    unsafe {
        asm!("lea {}, [rip]", out(reg) value, options(nostack, preserves_flags));
    }
    value
}

#[inline(always)]
pub fn read_idt_ptr() -> DescriptorTablePtr {
    let mut raw = [0u8; 10];
    unsafe {
        asm!("sidt [{}]", in(reg) raw.as_mut_ptr(), options(nostack, preserves_flags));
    }
    DescriptorTablePtr {
        limit: u16::from_le_bytes([raw[0], raw[1]]),
        base: u64::from_le_bytes([
            raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8], raw[9],
        ]),
    }
}

#[inline(always)]
pub fn read_gdt_ptr() -> DescriptorTablePtr {
    let mut raw = [0u8; 10];
    unsafe {
        asm!("sgdt [{}]", in(reg) raw.as_mut_ptr(), options(nostack, preserves_flags));
    }
    DescriptorTablePtr {
        limit: u16::from_le_bytes([raw[0], raw[1]]),
        base: u64::from_le_bytes([
            raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8], raw[9],
        ]),
    }
}
