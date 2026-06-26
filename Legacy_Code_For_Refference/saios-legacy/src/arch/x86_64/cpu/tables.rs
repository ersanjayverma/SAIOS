//! x86_64 descriptor-table register helpers used by low-level diagnostics.

/// Read GDTR as raw base+limit without depending on high-level abstractions.
/// The packed layout is the architectural SGDT memory operand: u16 limit,
/// followed by a u64 base in 64-bit mode.
pub fn sgdt() -> (u64, u16) {
    #[repr(C, packed)]
    struct DescriptorTablePtr {
        limit: u16,
        base: u64,
    }

    let mut ptr = DescriptorTablePtr { limit: 0, base: 0 };
    unsafe {
        core::arch::asm!("sgdt [{}]", in(reg) &mut ptr, options(nostack, preserves_flags));
    }
    let base = unsafe { core::ptr::addr_of!(ptr.base).read_unaligned() };
    let limit = unsafe { core::ptr::addr_of!(ptr.limit).read_unaligned() };
    (base, limit)
}

/// Read IDTR for the same reason as SGDT: if exception delivery works after
/// CR3 switching, this output proves which IDT base the CPU is using.
pub fn sidt() -> (u64, u16) {
    #[repr(C, packed)]
    struct DescriptorTablePtr {
        limit: u16,
        base: u64,
    }

    let mut ptr = DescriptorTablePtr { limit: 0, base: 0 };
    unsafe {
        core::arch::asm!("sidt [{}]", in(reg) &mut ptr, options(nostack, preserves_flags));
    }
    let base = unsafe { core::ptr::addr_of!(ptr.base).read_unaligned() };
    let limit = unsafe { core::ptr::addr_of!(ptr.limit).read_unaligned() };
    (base, limit)
}
