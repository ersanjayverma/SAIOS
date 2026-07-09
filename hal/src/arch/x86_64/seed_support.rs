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
const USER_MODE_VERBOSE_LOGS: bool = false;

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
    "push rdx",
    "push rbx",
    "push rcx",
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
    // Interrupted RIP sits at the base of the original (pre-push) iret
    // frame, i.e. 120 bytes above the 15 GPRs just pushed. rbp still holds
    // that fixed offset even after the alignment below moves rsp.
    "mov rdi, [rbp + 120]",
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
    "pop rcx",
    "pop rbx",
    "pop rdx",
    "pop rax",
    "iretq",

    ".section .bss, \"aw\", @nobits",
    ".balign 8",
    ".global hal_user_fault_return_rsp",
    "hal_user_fault_return_rsp:",
    ".quad 0",
    ".global hal_user_fault_return_rip",
    "hal_user_fault_return_rip:",
    ".quad 0",
    ".global hal_user_fault_saved_rbx",
    "hal_user_fault_saved_rbx:",
    ".quad 0",
    ".global hal_user_fault_saved_rbp",
    "hal_user_fault_saved_rbp:",
    ".quad 0",
    ".global hal_user_fault_saved_r12",
    "hal_user_fault_saved_r12:",
    ".quad 0",
    ".global hal_user_fault_saved_r13",
    "hal_user_fault_saved_r13:",
    ".quad 0",
    ".global hal_user_fault_saved_r14",
    "hal_user_fault_saved_r14:",
    ".quad 0",
    ".global hal_user_fault_saved_r15",
    "hal_user_fault_saved_r15:",
    ".quad 0",
    ".balign 1",
    ".global hal_user_fault_return_active",
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
    // Preserve callee-saved registers so recovery return obeys SysV ABI.
    "mov [rip + hal_user_fault_saved_rbx], rbx",
    "mov [rip + hal_user_fault_saved_rbp], rbp",
    "mov [rip + hal_user_fault_saved_r12], r12",
    "mov [rip + hal_user_fault_saved_r13], r13",
    "mov [rip + hal_user_fault_saved_r14], r14",
    "mov [rip + hal_user_fault_saved_r15], r15",
    "mov byte ptr [rip + hal_user_fault_return_active], 1",

    // Build IRET frame
    "push rcx", // SS
    "push rsi", // RSP
    "push rdx", // RFLAGS
    "push r8",  // CS
    "push rdi", // RIP

    // Zero all GPRs the SysV entry ABI cares about before handing off to
    // user code. In particular rdx must be 0 (it's the "rtld_fini"
    // function-pointer slot _start propagates into __libc_start_main's
    // rtld_fini param) -- musl calls it as a function pointer during
    // static-binary startup if nonzero, hanging forever.
    "xor eax, eax",
    "xor edx, edx",
    "iretq",

    // Reached only if recovery path redirects execution here
    "2:",
    "mov byte ptr [rip + hal_user_fault_return_active], 0",

    // Restore callee-saved registers before returning to Rust caller.
    "mov rbx, [rip + hal_user_fault_saved_rbx]",
    "mov rbp, [rip + hal_user_fault_saved_rbp]",
    "mov r12, [rip + hal_user_fault_saved_r12]",
    "mov r13, [rip + hal_user_fault_saved_r13]",
    "mov r14, [rip + hal_user_fault_saved_r14]",
    "mov r15, [rip + hal_user_fault_saved_r15]",

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

    // enter_user_mode_from_frame: jump to ring 3 using a saved UserSyscallFrame.
    // rdi = *const UserSyscallFrame
    // UserSyscallFrame byte offsets:
    //   r15=+0, r14=+8, r13=+16, r12=+24, r10=+32, r9=+40, r8=+48,
    //   rbp=+56, rdi=+64, rsi=+72, rdx=+80, rbx=+88, rax=+96 (return value),
    //   user_rip=+104, user_cs=+112, user_rflags=+120, user_rsp=+128, user_ss=+136
    ".global hal_enter_user_mode_from_frame",
    "hal_enter_user_mode_from_frame:",
    // Record recovery so that resume_from_user_fault returns here.
    "mov [rip + hal_user_fault_return_rsp], rsp",
    "lea rax, [rip + 3f]",
    "mov [rip + hal_user_fault_return_rip], rax",
    "mov [rip + hal_user_fault_saved_rbx], rbx",
    "mov [rip + hal_user_fault_saved_rbp], rbp",
    "mov [rip + hal_user_fault_saved_r12], r12",
    "mov [rip + hal_user_fault_saved_r13], r13",
    "mov [rip + hal_user_fault_saved_r14], r14",
    "mov [rip + hal_user_fault_saved_r15], r15",
    "mov byte ptr [rip + hal_user_fault_return_active], 1",
    // Build iretq frame (SS, RSP, RFLAGS, CS, RIP – pushed in reverse).
    "push qword ptr [rdi + 136]",
    "push qword ptr [rdi + 128]",
    "push qword ptr [rdi + 120]",
    "push qword ptr [rdi + 112]",
    "push qword ptr [rdi + 104]",
    // Load all GPRs; rdi and rax are last.
    "mov rax, rdi",
    "mov rbx, [rax + 88]",
    "mov rcx, [rax + 104]",
    "mov rdx, [rax + 80]",
    "mov rsi, [rax + 72]",
    "mov rbp, [rax + 56]",
    "mov r8,  [rax + 48]",
    "mov r9,  [rax + 40]",
    "mov r10, [rax + 32]",
    "mov r11, [rax + 120]",
    "mov r12, [rax + 24]",
    "mov r13, [rax + 16]",
    "mov r14, [rax + 8]",
    "mov r15, [rax + 0]",
    "mov rdi, [rax + 64]",
    "mov rax, [rax + 96]",
    "iretq",
    // Recovery path — reached via hal_resume_from_user_fault.
    "3:",
    "mov byte ptr [rip + hal_user_fault_return_active], 0",
    "mov rbx, [rip + hal_user_fault_saved_rbx]",
    "mov rbp, [rip + hal_user_fault_saved_rbp]",
    "mov r12, [rip + hal_user_fault_saved_r12]",
    "mov r13, [rip + hal_user_fault_saved_r13]",
    "mov r14, [rip + hal_user_fault_saved_r14]",
    "mov r15, [rip + hal_user_fault_saved_r15]",
    "mov eax, 1",
    "ret",
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
    fn hal_enter_user_mode_from_frame(frame: *const u8) -> u64;

    // BSS statics written/read by the user-mode entry / recovery asm.
    static mut hal_user_fault_return_rsp:    u64;
    static mut hal_user_fault_return_rip:    u64;
    static mut hal_user_fault_saved_rbx:     u64;
    static mut hal_user_fault_saved_rbp:     u64;
    static mut hal_user_fault_saved_r12:     u64;
    static mut hal_user_fault_saved_r13:     u64;
    static mut hal_user_fault_saved_r14:     u64;
    static mut hal_user_fault_saved_r15:     u64;
    static mut hal_user_fault_return_active: u8;
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

/// Snapshot of the global fault-recovery statics for per-thread save/restore.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct FaultRecoveryContext {
    pub return_rsp: u64,
    pub return_rip: u64,
    pub saved_rbx:  u64,
    pub saved_rbp:  u64,
    pub saved_r12:  u64,
    pub saved_r13:  u64,
    pub saved_r14:  u64,
    pub saved_r15:  u64,
    pub active:     u8,
}

/// Capture the current global fault-recovery statics.  Call only from kernel
/// context (e.g., inside `without_interrupts`).
pub fn save_fault_recovery_context() -> FaultRecoveryContext {
    unsafe {
        FaultRecoveryContext {
            return_rsp: hal_user_fault_return_rsp,
            return_rip: hal_user_fault_return_rip,
            saved_rbx:  hal_user_fault_saved_rbx,
            saved_rbp:  hal_user_fault_saved_rbp,
            saved_r12:  hal_user_fault_saved_r12,
            saved_r13:  hal_user_fault_saved_r13,
            saved_r14:  hal_user_fault_saved_r14,
            saved_r15:  hal_user_fault_saved_r15,
            active:     hal_user_fault_return_active,
        }
    }
}

/// Restore the global fault-recovery statics from a saved context.
///
/// # Safety
/// The caller must ensure the saved `return_rsp`/`return_rip` still point
/// to a live kernel call frame.
pub unsafe fn restore_fault_recovery_context(ctx: &FaultRecoveryContext) {
    unsafe {
        hal_user_fault_return_rsp    = ctx.return_rsp;
        hal_user_fault_return_rip    = ctx.return_rip;
        hal_user_fault_saved_rbx     = ctx.saved_rbx;
        hal_user_fault_saved_rbp     = ctx.saved_rbp;
        hal_user_fault_saved_r12     = ctx.saved_r12;
        hal_user_fault_saved_r13     = ctx.saved_r13;
        hal_user_fault_saved_r14     = ctx.saved_r14;
        hal_user_fault_saved_r15     = ctx.saved_r15;
        hal_user_fault_return_active = ctx.active;
    }
}

#[inline(always)]
pub fn resume_from_user_fault() -> ! {
    unsafe { hal_resume_from_user_fault() }
}

/// Enter ring 3 using a fully-saved `UserSyscallFrame` (e.g. the child after
/// a fork).  Sets up the fault-recovery context so that `exit` / any fault
/// returns here with value `true`.  The caller is responsible for ensuring
/// `SAIOS_SYSCALL_RSP0` and `TSS.RSP0` already point to this thread's own
/// kernel transition stack.
///
/// # Safety
/// `frame` must be a valid pointer to a `UserSyscallFrame`.
pub unsafe fn enter_user_mode_from_frame(
    frame: *const crate::arch::x86_64::syscall::UserSyscallFrame,
) -> bool {
    unsafe { hal_enter_user_mode_from_frame(frame as *const u8) != 0 }
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

    if USER_MODE_VERBOSE_LOGS {
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
    }


    unsafe {
        hal_enter_user_mode_recoverable(entry, user_rsp, rflags, user_ss, user_cs) != 0
    }
}
