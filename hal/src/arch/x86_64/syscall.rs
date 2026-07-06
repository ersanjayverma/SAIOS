//! SYSCALL/SYSRET bring-up helpers for x86_64.
//!
//! This configures the architectural syscall entry MSRs so 64-bit userspace
//! `syscall` instructions enter the kernel through a controlled entry path.

use core::arch::global_asm;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::arch::x86_64::constants::{
    MSR_IA32_EFER, MSR_IA32_STAR, MSR_IA32_LSTAR, MSR_IA32_FMASK,
    EFER_SCE, RFLAGS_IF,
};
use crate::arch::x86_64::{gdt, msr};
const USER_CODE_SELECTOR: u64 = gdt::USER_CODE.0 as u64;
const USER_DATA_SELECTOR: u64 = gdt::USER_DATA.0 as u64;

#[unsafe(no_mangle)]
pub static SAIOS_SYSCALL_RSP0: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
pub static SAIOS_SYSCALL_USER_RSP: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug)]
pub struct SyscallMsrSnapshot {
    pub efer: u64,
    pub star: u64,
    pub lstar: u64,
    pub fmask: u64,
    pub syscall_kernel_cs: u16,
    pub syscall_kernel_ss: u16,
    pub sysret_user_cs: u16,
    pub sysret_user_ss: u16,
    pub iret_user_cs: u16,
    pub iret_user_ss: u16,
    pub rsp0: u64,
    pub entry: u64,
}

global_asm!(
    ".section .text.syscall, \"ax\"",
    ".code64",
    ".global saios_syscall_entry",
    "saios_syscall_entry:",
    // SYSCALL does not switch stacks automatically. Save user RSP and switch
    // to the kernel transition stack before touching normal kernel state.
    "mov [rip + SAIOS_SYSCALL_USER_RSP], rsp",
    "mov rsp, [rip + SAIOS_SYSCALL_RSP0]",
    "and rsp, -16",
    // Preserve userspace return state across the Rust dispatcher call.
    "mov r12, rcx",
    "mov r13, r11",
    "mov r14, [rip + SAIOS_SYSCALL_USER_RSP]",
    // Linux x86_64 syscall ABI at entry: rax=nr, rdi/rsi/rdx/r10/r8/r9=args.
    // Rust SysV call target: saios_linux_syscall(nr,a0,a1,a2,a3,a4,a5)
    // -> rdi,rsi,rdx,rcx,r8,r9 and 7th arg on stack.
    "mov r15, rax",
    "sub rsp, 16",
    "mov [rsp], r9",
    "mov r9, r8",
    "mov r8, r10",
    "mov rcx, rdx",
    "mov rdx, rsi",
    "mov rsi, rdi",
    "mov rdi, r15",
    "call {dispatch}",
    "add rsp, 16",
    "push {user_data}",
    "push r14",
    "push r13", // saved user RFLAGS from SYSCALL
    "push {user_code}",
    "push r12", // saved user RIP from SYSCALL
    "iretq",
    dispatch = sym saios_linux_syscall,
    user_data = const USER_DATA_SELECTOR,
    user_code = const USER_CODE_SELECTOR,
);

unsafe extern "C" {
    fn saios_syscall_entry();
    fn saios_linux_syscall(nr: u64, a0: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64)
        -> i64;
}

pub fn set_kernel_rsp0(rsp0: u64) {
    SAIOS_SYSCALL_RSP0.store(rsp0, Ordering::Release);
}

pub fn snapshot() -> SyscallMsrSnapshot {
    let star = msr::rdmsr(MSR_IA32_STAR);
    let syscall_kernel_cs = ((star >> 32) & 0xffff) as u16;
    let syscall_kernel_ss = syscall_kernel_cs.wrapping_add(8);
    let sysret_base = ((star >> 48) & 0xffff) as u16;
    let sysret_user_cs = sysret_base.wrapping_add(16) | 3;
    let sysret_user_ss = sysret_base.wrapping_add(8) | 3;

    SyscallMsrSnapshot {
        efer: msr::rdmsr(MSR_IA32_EFER),
        star,
        lstar: msr::rdmsr(MSR_IA32_LSTAR),
        fmask: msr::rdmsr(MSR_IA32_FMASK),
        syscall_kernel_cs,
        syscall_kernel_ss,
        sysret_user_cs,
        sysret_user_ss,
        iret_user_cs: gdt::USER_CODE.0,
        iret_user_ss: gdt::USER_DATA.0,
        rsp0: SAIOS_SYSCALL_RSP0.load(Ordering::Acquire),
        entry: saios_syscall_entry as *const () as u64,
    }
}

pub fn init() -> Result<(), &'static str> {
    let kernel_cs = gdt::KERNEL_CODE.0 as u64;
    let user_cs = gdt::USER_CODE.0 as u64;

    if user_cs < 16 {
        return Err("syscall: user code selector must be >= 16");
    }

    // For SYSRET in 64-bit mode, CPU computes CS = STAR[63:48] + 16.
    let sysret_cs_base = user_cs - 16;
    let star = (sysret_cs_base << 48) | (kernel_cs << 32);
    let lstar = saios_syscall_entry as *const () as u64;

    let mut efer = msr::rdmsr(MSR_IA32_EFER);
    efer |= EFER_SCE;

    msr::wrmsr(MSR_IA32_STAR, star);
    msr::wrmsr(MSR_IA32_LSTAR, lstar);
    msr::wrmsr(MSR_IA32_FMASK, RFLAGS_IF);
    msr::wrmsr(MSR_IA32_EFER, efer);

    Ok(())
}
