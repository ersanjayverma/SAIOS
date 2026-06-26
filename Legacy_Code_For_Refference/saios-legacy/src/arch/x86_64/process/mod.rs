//! x86_64 process execution plumbing: context-switch assembly, ring-3 entry, and TLS MSR helpers.

pub const IA32_FS_BASE_MSR: u32 = 0xC000_0100;
pub const IA32_GS_BASE_MSR: u32 = 0xC000_0101;
pub const IA32_KERNEL_GS_BASE_MSR: u32 = 0xC000_0102;

unsafe extern "C" {
    #[link_name = "switch_context"]
    fn switch_context_asm(from_rsp: *mut u64, to_rsp: *const u64);

    #[link_name = "switch_context_nosave"]
    fn switch_context_nosave_asm(to_rsp: *const u64) -> !;

    #[link_name = "ksetjmp"]
    fn ksetjmp_asm(buf: *mut u64) -> u64;

    #[link_name = "klongjmp"]
    fn klongjmp_asm(buf: *const u64, val: u64) -> !;

    #[link_name = "kthread_trampoline"]
    fn kthread_trampoline_asm();
}

#[inline]
pub unsafe fn switch_context(from_rsp: *mut u64, to_rsp: *const u64) {
    unsafe { switch_context_asm(from_rsp, to_rsp) }
}

#[inline]
pub unsafe fn switch_context_nosave(to_rsp: *const u64) -> ! {
    unsafe { switch_context_nosave_asm(to_rsp) }
}

#[inline]
pub unsafe fn ksetjmp(buf: *mut u64) -> u64 {
    unsafe { ksetjmp_asm(buf) }
}

#[inline]
pub unsafe fn klongjmp(buf: *const u64, val: u64) -> ! {
    unsafe { klongjmp_asm(buf, val) }
}

#[inline]
pub fn kthread_trampoline_addr() -> u64 {
    kthread_trampoline_asm as *const () as usize as u64
}

#[inline]
pub unsafe fn set_fs_base(addr: u64) {
    unsafe { super::write_msr(IA32_FS_BASE_MSR, addr) }
}

#[inline]
pub unsafe fn set_gs_base(addr: u64) {
    unsafe { super::write_msr(IA32_GS_BASE_MSR, addr) }
}

#[inline]
pub unsafe fn set_kernel_gs_base(addr: u64) {
    unsafe { super::write_msr(IA32_KERNEL_GS_BASE_MSR, addr) }
}

#[inline]
pub unsafe fn read_gs_base() -> u64 {
    unsafe { super::read_msr(IA32_GS_BASE_MSR) }
}

#[inline]
pub unsafe fn read_kernel_gs_base() -> u64 {
    unsafe { super::read_msr(IA32_KERNEL_GS_BASE_MSR) }
}

#[inline]
pub unsafe fn swapgs() {
    unsafe { core::arch::asm!("swapgs", options(nostack, preserves_flags)) }
}

#[inline]
pub unsafe fn restore_user_tls(fs_base: u64, gs_base: u64) {
    unsafe {
        if fs_base != 0 {
            set_fs_base(fs_base);
        }
        set_gs_base(gs_base);
        crate::arch::syscall::install_kernel_gs_base();
        crate::arch::syscall::mark_kernel_gs_active(false);
    }
}

#[inline]
pub unsafe fn restore_user_tls_from_syscall(fs_base: u64, gs_base: u64) {
    unsafe {
        if crate::arch::syscall::kernel_gs_active() {
            if fs_base != 0 {
                set_fs_base(fs_base);
            }
            set_kernel_gs_base(gs_base);
            swapgs();
            crate::arch::syscall::mark_kernel_gs_active(false);
        } else {
            restore_user_tls(fs_base, gs_base);
        }
    }
}

pub fn jump_to_userspace(rip: u64, rsp: u64, rflags: u64, fs_base: u64, gs_base: u64) -> ! {
    use crate::gdt::{USER_CS, USER_DS};

    unsafe {
        restore_user_tls(fs_base, gs_base);
    }

    dump_ring3_transition(rip, rsp, rflags, USER_CS as u64, USER_DS as u64);
    crate::serial_println!("[ring3] about to iretq rip={:#x} rsp={:#x}", rip, rsp);
    unsafe {
        core::arch::asm!(
            "cli",
            "push {ss}", "push {rsp}", "push {rflags}", "push {cs}", "push {rip}",
            "iretq",
            "ud2",
            ss     = in(reg) USER_DS as u64,
            rsp    = in(reg) rsp,
            rflags = in(reg) rflags,
            cs     = in(reg) USER_CS as u64,
            rip    = in(reg) rip,
            options(noreturn),
        );
    }
}

pub fn jump_to_userspace_from_syscall(
    rip: u64,
    rsp: u64,
    rflags: u64,
    fs_base: u64,
    gs_base: u64,
) -> ! {
    use crate::gdt::{USER_CS, USER_DS};

    unsafe {
        restore_user_tls_from_syscall(fs_base, gs_base);
    }

    if crate::diag::diag_proc_on() {
        crate::serial_println!(
            "[ring3] syscall-origin tls restored kernel_gs_active={}",
            crate::arch::syscall::kernel_gs_active()
        );
    }

    dump_ring3_transition(rip, rsp, rflags, USER_CS as u64, USER_DS as u64);
    crate::serial_println!(
        "[ring3] about to syscall-origin iretq rip={:#x} rsp={:#x}",
        rip,
        rsp
    );
    unsafe {
        core::arch::asm!(
            "cli",
            "push {ss}", "push {rsp}", "push {rflags}", "push {cs}", "push {rip}",
            "iretq",
            "ud2",
            ss     = in(reg) USER_DS as u64,
            rsp    = in(reg) rsp,
            rflags = in(reg) rflags,
            cs     = in(reg) USER_CS as u64,
            rip    = in(reg) rip,
            options(noreturn),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn jump_to_userspace_with_registers(
    rip: u64,
    rsp: u64,
    rflags: u64,
    rax: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    rbx: u64,
    rbp: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    fs_base: u64,
    gs_base: u64,
) -> ! {
    use crate::gdt::{USER_CS, USER_DS};
    let saved_regs = [
        rax, rbx, rbp, r12, r13, r14, r15, rdi, rsi, rdx, r8, r9, r10,
    ];

    unsafe {
        restore_user_tls(fs_base, gs_base);
    }

    dump_ring3_transition(rip, rsp, rflags, USER_CS as u64, USER_DS as u64);
    crate::serial_println!("[ring3] about to iretq rip={:#x} rsp={:#x}", rip, rsp);
    unsafe {
        core::arch::asm!(
            "cli",
            "push rdi",
            "push rsi",
            "push rdx",
            "push r8",
            "push r9",
            "mov r10, [rcx + 96]",
            "mov r9,  [rcx + 88]",
            "mov r8,  [rcx + 80]",
            "mov rdx, [rcx + 72]",
            "mov rsi, [rcx + 64]",
            "mov rdi, [rcx + 56]",
            "mov r15, [rcx + 48]",
            "mov r14, [rcx + 40]",
            "mov r13, [rcx + 32]",
            "mov r12, [rcx + 24]",
            "mov rbp, [rcx + 16]",
            "mov rbx, [rcx + 8]",
            "mov rax, [rcx]",
            "iretq",
            "ud2",
            in("rcx") saved_regs.as_ptr(),
            in("rdi") USER_DS as u64,
            in("rsi") rsp,
            in("rdx") rflags,
            in("r8") USER_CS as u64,
            in("r9") rip,
            options(noreturn),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn jump_to_userspace_with_registers_from_syscall(
    rip: u64,
    rsp: u64,
    rflags: u64,
    rax: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    rbx: u64,
    rbp: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    fs_base: u64,
    gs_base: u64,
) -> ! {
    use crate::gdt::{USER_CS, USER_DS};
    let saved_regs = [
        rax, rbx, rbp, r12, r13, r14, r15, rdi, rsi, rdx, r8, r9, r10,
    ];

    unsafe {
        restore_user_tls_from_syscall(fs_base, gs_base);
    }

    dump_ring3_transition(rip, rsp, rflags, USER_CS as u64, USER_DS as u64);
    crate::serial_println!(
        "[ring3] about to syscall-origin iretq rip={:#x} rsp={:#x}",
        rip,
        rsp
    );
    unsafe {
        core::arch::asm!(
            "cli",
            "push rdi",
            "push rsi",
            "push rdx",
            "push r8",
            "push r9",
            "mov r10, [rcx + 96]",
            "mov r9,  [rcx + 88]",
            "mov r8,  [rcx + 80]",
            "mov rdx, [rcx + 72]",
            "mov rsi, [rcx + 64]",
            "mov rdi, [rcx + 56]",
            "mov r15, [rcx + 48]",
            "mov r14, [rcx + 40]",
            "mov r13, [rcx + 32]",
            "mov r12, [rcx + 24]",
            "mov rbp, [rcx + 16]",
            "mov rbx, [rcx + 8]",
            "mov rax, [rcx]",
            "iretq",
            "ud2",
            in("rcx") saved_regs.as_ptr(),
            in("rdi") USER_DS as u64,
            in("rsi") rsp,
            in("rdx") rflags,
            in("r8") USER_CS as u64,
            in("r9") rip,
            options(noreturn),
        );
    }
}

/// Print a first-principles report for the exact `iretq` frame that will be
/// consumed by the CPU during the CPL0 -> CPL3 transition.
fn dump_ring3_transition(rip: u64, rsp: u64, rflags: u64, cs: u64, ss: u64) {
    let cr3 = crate::memory::paging::active_pml4();
    let (gdt_base, gdt_limit) = crate::arch::cpu::sgdt();
    let (idt_base, idt_limit) = crate::arch::cpu::sidt();

    let rip_entry = crate::memory::paging::translate_entry_in(cr3, rip);
    let rsp_entry = crate::memory::paging::translate_entry_in(cr3, rsp);
    let rsp_minus_8_entry = crate::memory::paging::translate_entry_in(cr3, rsp.wrapping_sub(8));

    crate::serial_println!("[ring3] iretq frame about to be pushed");
    crate::serial_println!(
        "[ring3] cr3={:#x} gdt={:#x}/limit={:#x} idt={:#x}/limit={:#x}",
        cr3,
        gdt_base,
        gdt_limit,
        idt_base,
        idt_limit
    );
    crate::serial_println!(
        "[ring3] cs={:#x} rpl={} ss={:#x} rpl={} rflags={:#x}",
        cs,
        cs & 3,
        ss,
        ss & 3,
        rflags
    );
    crate::serial_println!(
        "[ring3] rip={:#x} canonical4={} rsp={:#x} canonical4={}",
        rip,
        is_canonical_4level(rip),
        rsp,
        is_canonical_4level(rsp)
    );
    print_mapping("rip", rip, rip_entry);
    print_mapping("rsp", rsp, rsp_entry);
    print_mapping("rsp-8", rsp.wrapping_sub(8), rsp_minus_8_entry);
    crate::serial_println!(
        "[ring3] checks rip_ok={} rsp_ok={} cs_rpl3={} ss_rpl3={} if_set={} reserved_rflags_ok={}",
        is_canonical_4level(rip),
        is_canonical_4level(rsp),
        (cs & 3) == 3,
        (ss & 3) == 3,
        (rflags & (1 << 9)) != 0,
        (rflags & reserved_rflags_mask()) == 0
    );
}

fn print_mapping(label: &str, virt: u64, entry: Option<(u64, u64)>) {
    match entry {
        Some((phys, flags)) => {
            crate::serial_println!(
                "[ring3] map {:<5} virt={:#x} phys={:#x} P={} W={} U={} NX={}",
                label,
                virt,
                phys,
                yes(flags & crate::memory::paging::PTE_PRESENT != 0),
                yes(flags & crate::memory::paging::PTE_WRITABLE != 0),
                yes(flags & crate::memory::paging::PTE_USER != 0),
                yes(flags & crate::memory::paging::PTE_NO_EXEC != 0)
            );
        }
        None => {
            crate::serial_println!("[ring3] map {:<5} virt={:#x} unmapped", label, virt);
        }
    }
}

fn is_canonical_4level(addr: u64) -> bool {
    let sign = (addr >> 47) & 1;
    let high = addr >> 48;
    if sign == 0 { high == 0 } else { high == 0xFFFF }
}

fn reserved_rflags_mask() -> u64 {
    0xFFFF_FFFF_FF00_0000
}

fn yes(v: bool) -> &'static str {
    if v { "Y" } else { "N" }
}
