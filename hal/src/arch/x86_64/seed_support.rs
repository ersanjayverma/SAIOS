//! Seed kernel assembly support stubs for x86_64.
//!
//! Keeps architecture-specific assembly in HAL while exposing small Rust
//! wrappers consumed by the seed kernel crate.

use core::arch::global_asm;

#[repr(align(16))]
struct AlignedStack([u8; 32 * 1024]);

#[repr(align(16))]
struct StackGuard([u8; 4096]);

static mut USER_TRANSITION_KERNEL_STACK: AlignedStack = AlignedStack([0; 32 * 1024]);
static mut USER_TRANSITION_KERNEL_STACK_GUARD: StackGuard = StackGuard([0; 4096]);

global_asm!(
    // Boot entry trampoline and static boot page tables.
    ".section .boot, \"ax\"",
    ".code64",
    ".global _start",
    "_start:",
    "cli",
    "cld",
    // Ensure 2 MiB PDE mappings are valid for the boot page tables.
    "mov rax, cr4",
    "or rax, 0x10",
    "mov cr4, rax",
    "lea rax, [rip + boot_pml4]",
    "mov cr3, rax",
    "movabs rax, offset _saios_high_entry",
    "jmp rax",

    ".section .text.boot, \"ax\"",
    ".code64",
    ".global _saios_high_entry",
    "_saios_high_entry:",
    "and rsp, -16",
    "call saios_kernel_main",
    "2:",
    "hlt",
    "jmp 2b",

    ".section .boot.data, \"aw\"",
    ".balign 4096",
    ".global boot_pml4",
    "boot_pml4:",
    ".quad boot_pdpt_low + 0x3",
    ".zero 4072",
    ".quad boot_pml4 + 0x3",
    ".quad boot_pdpt_high + 0x3",

    ".balign 4096",
    "boot_pdpt_low:",
    ".quad boot_pd0 + 0x3",
    ".quad boot_pd1 + 0x3",
    ".quad boot_pd2 + 0x3",
    ".quad boot_pd3 + 0x3",
    ".zero 4064",

    ".balign 4096",
    "boot_pdpt_high:",
    ".zero 4080",
    ".quad boot_pd0 + 0x3",
    ".zero 8",

    ".balign 4096",
    "boot_pd0:",
    ".set boot_pd_idx, 0",
    ".rept 512",
    ".quad (boot_pd_idx << 21) | 0x83",
    ".set boot_pd_idx, boot_pd_idx + 1",
    ".endr",

    ".balign 4096",
    "boot_pd1:",
    ".set boot_pd_idx, 512",
    ".rept 512",
    ".quad (boot_pd_idx << 21) | 0x83",
    ".set boot_pd_idx, boot_pd_idx + 1",
    ".endr",

    ".balign 4096",
    "boot_pd2:",
    ".set boot_pd_idx, 1024",
    ".rept 512",
    ".quad (boot_pd_idx << 21) | 0x83",
    ".set boot_pd_idx, boot_pd_idx + 1",
    ".endr",

    ".balign 4096",
    "boot_pd3:",
    ".set boot_pd_idx, 1536",
    ".rept 512",
    ".quad (boot_pd_idx << 21) | 0x83",
    ".set boot_pd_idx, boot_pd_idx + 1",
    ".endr",

    // Scheduler context switch helper.
    ".global hal_context_switch",
    "hal_context_switch:",
    "mov [rdi + 0x00], rsp",
    "mov [rdi + 0x08], rbx",
    "mov [rdi + 0x10], rbp",
    "mov [rdi + 0x18], r12",
    "mov [rdi + 0x20], r13",
    "mov [rdi + 0x28], r14",
    "mov [rdi + 0x30], r15",
    "mov rsp, [rsi + 0x00]",
    "mov rbx, [rsi + 0x08]",
    "mov rbp, [rsi + 0x10]",
    "mov r12, [rsi + 0x18]",
    "mov r13, [rsi + 0x20]",
    "mov r14, [rsi + 0x28]",
    "mov r15, [rsi + 0x30]",
    "ret",

    // PIT IRQ0 interrupt trampoline.
    ".global hal_timer_irq0_stub",
    "hal_timer_irq0_stub:",
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rbp",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // Rust assumes DF=0 and a SysV-aligned stack at call boundaries.
    "cld",
    "mov rbp, rsp",
    "and rsp, -16",
    "call saios_timer_tick",
    "mov rsp, rbp",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rbp",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",
    "iretq",
);

unsafe extern "C" {
    fn hal_context_switch(old: *mut u8, new: *const u8);
    fn hal_timer_irq0_stub();
}

#[inline(always)]
pub fn ensure_linked() {}

#[inline(always)]
/// # Safety
///
/// `old` and `new` must point to valid context structs with the expected
/// x86_64 callee-saved register layout.
pub unsafe fn context_switch(old: *mut u8, new: *const u8) {
    unsafe { hal_context_switch(old, new) }
}

#[inline(always)]
pub fn timer_irq0_stub_addr() -> usize {
    hal_timer_irq0_stub as *const () as usize
}

#[inline(always)]
pub fn user_transition_kernel_rsp0() -> u64 {
    unsafe {
        let _guard = core::ptr::addr_of!(USER_TRANSITION_KERNEL_STACK_GUARD.0);
        let base = core::ptr::addr_of!(USER_TRANSITION_KERNEL_STACK.0) as u64;
        base + (32 * 1024) as u64
    }
}

#[inline(always)]
/// # Safety
///
/// Caller must ensure `entry`/`user_rsp` are canonical user virtual addresses
/// mapped in the active address space and that TSS.rsp0 points to a valid
/// kernel stack for privilege transitions.
pub unsafe fn enter_user_mode(entry: u64, user_rsp: u64) -> ! {
    let mut rflags: u64;
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            out(reg) rflags,
            options(nomem, preserves_flags),
        );
    }
    rflags |= 1 << 9; // IF

    let user_cs = crate::arch::x86_64::gdt::USER_CODE.0 as u64;
    let user_ss = crate::arch::x86_64::gdt::USER_DATA.0 as u64;
    let gdt = crate::arch::x86_64::cpu::read_gdt_ptr();
    let idt = crate::arch::x86_64::cpu::read_idt_ptr();
    let rsp0 = crate::arch::x86_64::tss::rsp0();

    crate::arch::x86_64::console::_print_force(format_args!(
        "[user-jump] rip={:#x} rsp={:#x} rflags={:#x} cs={:#x} ss={:#x} rsp0={:#x}\n",
        entry,
        user_rsp,
        rflags,
        user_cs,
        user_ss,
        rsp0
    ));
    crate::arch::x86_64::console::_print_force(format_args!(
        "[user-jump] gdt=({:#x},limit={:#x}) idt=({:#x},limit={:#x})\n",
        gdt.base,
        gdt.limit,
        idt.base,
        idt.limit
    ));

    unsafe {
        core::arch::asm!(
            "push rdx", // SS
            "push rsi", // RSP
            "push rcx", // RFLAGS
            "push r8",  // CS
            "push rdi", // RIP
            "push rax",
            "mov dx, 0x3f8",
            "mov al, 'J'",
            "out dx, al",
            "pop rax",
            // Linux-style process entry expects rdx=0 (rtld_fini for static
            // binaries). iretq does not consume general registers, so clear
            // the user-visible state after building the frame and avoid
            // touching those registers again before the privilege transition.
            "xor eax, eax",
            "xor edx, edx",
            "iretq",
            in("rdi") entry,
            in("rsi") user_rsp,
            in("rcx") rflags,
            in("rdx") user_ss,
            in("r8") user_cs,
            options(noreturn),
        );
    }
}
