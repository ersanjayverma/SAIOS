//! Seed kernel assembly support stubs for x86_64.
//!
//! Keeps architecture-specific assembly in HAL while exposing small Rust
//! wrappers consumed by the seed kernel crate.

use crate::arch::x86_64::constants::{
    USER_ENTRY_ENABLE_INTERRUPTS, USER_TRANSITION_STACK_SIZE, USER_TRANSITION_GUARD_SIZE,
};
use core::arch::global_asm;

#[repr(align(16))]
struct AlignedStack([u8; USER_TRANSITION_STACK_SIZE]);

#[repr(align(16))]
struct StackGuard([u8; USER_TRANSITION_GUARD_SIZE]);

static mut USER_TRANSITION_KERNEL_STACK: AlignedStack = AlignedStack([0; USER_TRANSITION_STACK_SIZE]);
static mut USER_TRANSITION_KERNEL_STACK_GUARD: StackGuard = StackGuard([0; USER_TRANSITION_GUARD_SIZE]);

const PT_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

fn translate_virtual_to_physical(virt: u64) -> Option<u64> {
    let cr3 = crate::arch::x86_64::paging::read_cr3();
    let l4 = ((virt >> 39) & 0x1ff) as usize;
    let l3 = ((virt >> 30) & 0x1ff) as usize;
    let l2 = ((virt >> 21) & 0x1ff) as usize;
    let l1 = ((virt >> 12) & 0x1ff) as usize;
    let page_off = virt & 0xfff;

    // SAFETY: Reads live page tables from the current CR3 to produce diagnostics.
    let pml4e = unsafe { *((cr3 & PT_ADDR_MASK) as *const u64).add(l4) };
    if (pml4e & 0x1) == 0 {
        return None;
    }

    // SAFETY: pml4e present; next-level table address comes from HW-defined entry bits.
    let pdpte = unsafe { *((pml4e & PT_ADDR_MASK) as *const u64).add(l3) };
    if (pdpte & 0x1) == 0 {
        return None;
    }
    if (pdpte & 0x80) != 0 {
        let base = pdpte & 0x000F_FFFF_C000_0000;
        return Some(base | (virt & 0x3fff_ffff));
    }

    // SAFETY: pdpte present and points to the next page-table level.
    let pde = unsafe { *((pdpte & PT_ADDR_MASK) as *const u64).add(l2) };
    if (pde & 0x1) == 0 {
        return None;
    }
    if (pde & 0x80) != 0 {
        let base = pde & 0x000F_FFFF_FFE0_0000;
        return Some(base | (virt & 0x1f_ffff));
    }

    // SAFETY: pde present and points to a normal PT page.
    let pte = unsafe { *((pde & PT_ADDR_MASK) as *const u64).add(l1) };
    if (pte & 0x1) == 0 {
        return None;
    }

    Some((pte & PT_ADDR_MASK) | page_off)
}

fn read_user_u8_checked(virt: u64) -> Option<u8> {
    let phys = translate_virtual_to_physical(virt)?;
    // SAFETY: Only read after translation confirms a present mapping.
    Some(unsafe { core::ptr::read_volatile(phys as *const u8) })
}

fn read_user_u64_checked(virt: u64) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    while shift < 64 {
        let b = read_user_u8_checked(virt.wrapping_add((shift / 8) as u64))? as u64;
        value |= b << shift;
        shift += 8;
    }
    Some(value)
}

fn dump_user_stack_qwords(rsp: u64) {
    crate::arch::x86_64::console::_print_force(format_args!(
        "[user-rsp] dump64 base={:#x}\n",
        rsp
    ));
    let mut i = 0u64;
    while i < 8 {
        let addr = rsp.wrapping_add(i * 8);
        if let Some(word) = read_user_u64_checked(addr) {
            crate::arch::x86_64::console::_print_force(format_args!(
                "[user-rsp] {:#018x}: {:#018x}\n",
                addr,
                word
            ));
        } else {
            crate::arch::x86_64::console::_print_force(format_args!(
                "[user-rsp] {:#018x}: <unmapped>\n",
                addr
            ));
        }
        i += 1;
    }
}

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

    ".section .bss, \"aw\", @nobits",
    ".balign 8",
    "hal_user_fault_return_rsp:",
    ".quad 0",
    "hal_user_fault_return_rip:",
    ".quad 0",
    ".balign 1",
    "hal_user_fault_return_active:",
    ".byte 0",
    ".section .text.boot, \"ax\"",

 ".global hal_enter_user_mode_recoverable",
    "hal_enter_user_mode_recoverable:",
    // SysV args:
    // rdi = user RIP
    // rsi = user RSP
    // rdx = RFLAGS
    // rcx = user SS
    // r8  = user CS

    "mov [rip + hal_user_fault_return_rsp], rsp",
    "lea rax, [rip + 2f]",
    "mov [rip + hal_user_fault_return_rip], rax",
    "mov byte ptr [rip + hal_user_fault_return_active], 1",

    // Build IRET frame
    "push rcx", // SS
    "push rsi", // RSP
    "push rdx", // RFLAGS
    "push r8",  // CS
    "push rdi", // RIP

    // DEBUG: output 'B' to COM1 before iretq
    "mov dx, 0x3F8",
    "mov al, 'B'",
    "out dx, al",

    "xor eax, eax",
    "iretq",

    // Reached only if recovery path redirects execution here
    "2:",
    "mov byte ptr [rip + hal_user_fault_return_active], 0",

    // DEBUG: output 'A' after recovery
    "mov dx, 0x3F8",
    "mov al, 'A'",
    "out dx, al",

    "mov eax, 1",
    "ret",

    ".global hal_resume_from_user_fault",
    "hal_resume_from_user_fault:",
    "cmp byte ptr [rip + hal_user_fault_return_active], 0",
    "je 2f",
    "mov rsp, [rip + hal_user_fault_return_rsp]",
    "jmp qword ptr [rip + hal_user_fault_return_rip]",
    "2:",
    "hlt",
    "jmp 2b",
);

unsafe extern "C" {
    fn hal_context_switch(old: *mut u8, new: *const u8);
    fn hal_timer_irq0_stub();
    fn hal_enter_user_mode_recoverable(
        entry: u64,
        user_rsp: u64,
        rflags: u64,
        user_ss: u64,
        user_cs: u64,
    ) -> u64;
    fn hal_resume_from_user_fault() -> !;
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
        base + USER_TRANSITION_STACK_SIZE as u64
    }
}

#[inline(always)]
pub fn user_transition_stack_layout() -> (u64, u64, u64, u64) {
    unsafe {
        let stack_base = core::ptr::addr_of!(USER_TRANSITION_KERNEL_STACK.0) as u64;
        let guard_base = core::ptr::addr_of!(USER_TRANSITION_KERNEL_STACK_GUARD.0) as u64;
        (
            stack_base,
            stack_base + USER_TRANSITION_STACK_SIZE as u64,
            guard_base,
            guard_base + USER_TRANSITION_GUARD_SIZE as u64,
        )
    }
}

#[inline(always)]
pub fn resume_from_user_fault() -> ! {
    unsafe { hal_resume_from_user_fault() }
}

#[inline(always)]
/// # Safety
///
/// Caller must ensure `entry`/`user_rsp` are canonical user virtual addresses
/// mapped in the active address space and that TSS.rsp0 points to a valid
/// kernel stack for privilege transitions.
pub unsafe fn enter_user_mode(entry: u64, user_rsp: u64) -> bool {
    let mut rflags: u64;
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            out(reg) rflags,
            options(nomem, preserves_flags),
        );
    }
    if USER_ENTRY_ENABLE_INTERRUPTS {
        rflags |= 1 << 9; // IF
    } else {
        rflags &= !(1 << 9);
    }

    let user_cs = crate::arch::x86_64::gdt::USER_CODE.0 as u64;
    let user_ss = crate::arch::x86_64::gdt::USER_DATA.0 as u64;
    let gdt = crate::arch::x86_64::cpu::read_gdt_ptr();
    let idt = crate::arch::x86_64::cpu::read_idt_ptr();
    let rsp0 = crate::arch::x86_64::tss::rsp0();
    let tss = crate::arch::x86_64::tss::instance() as u64;
    let kernel_rsp = crate::arch::x86_64::cpu::read_rsp();
    let (stack_base, stack_end, guard_base, guard_end) = user_transition_stack_layout();
    let gdt_storage = crate::arch::x86_64::gdt::storage_base();
    let gdt_storage_limit = crate::arch::x86_64::gdt::storage_limit();

    crate::arch::x86_64::console::_print_force(format_args!(
        "[user-jump] rip={:#x} rsp={:#x} rflags={:#x} cs={:#x} ss={:#x} rsp0={:#x} if={}\n",
        entry,
        user_rsp,
        rflags,
        user_cs,
        user_ss,
        rsp0,
        if USER_ENTRY_ENABLE_INTERRUPTS { 1 } else { 0 }
    ));
    crate::arch::x86_64::console::_print_force(format_args!(
        "[user-jump] gdt=({:#x},limit={:#x}) idt=({:#x},limit={:#x})\n",
        gdt.base,
        gdt.limit,
        idt.base,
        idt.limit
    ));
    crate::arch::x86_64::console::_print_force(format_args!(
        "[user-jump] gdt-storage=({:#x},limit={:#x}) tss={:#x} kernel-rsp={:#x}\n",
        gdt_storage,
        gdt_storage_limit,
        tss,
        kernel_rsp,
    ));
    crate::arch::x86_64::console::_print_force(format_args!(
        "[user-jump] stack=[{:#x}..{:#x}) guard=[{:#x}..{:#x})\n",
        stack_base,
        stack_end,
        guard_base,
        guard_end,
    ));

    dump_user_stack_qwords(user_rsp);


    unsafe {
        hal_enter_user_mode_recoverable(entry, user_rsp, rflags, user_ss, user_cs) != 0
    }
}
