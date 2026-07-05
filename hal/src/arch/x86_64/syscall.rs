//! SYSCALL/SYSRET bring-up helpers for x86_64.
//!
//! This configures the architectural syscall entry MSRs so 64-bit userspace
//! `syscall` instructions enter the kernel through a controlled entry path.

use core::arch::global_asm;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::arch::x86_64::{gdt, msr};

const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;

const EFER_SCE: u64 = 1 << 0;
const RFLAGS_IF: u64 = 1 << 9;
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
    "cli",
    "mov dx, 0x3f8",
    "mov al, 'S'",
    "out dx, al",
    // SYSCALL does not switch stacks automatically. Save user RSP and switch
    // to the kernel transition stack before touching normal kernel state.
    "mov al, '0'",
    "out dx, al",
    "mov [rip + SAIOS_SYSCALL_USER_RSP], rsp",
    "mov al, '1'",
    "out dx, al",
    "mov rsp, [rip + SAIOS_SYSCALL_RSP0]",
    "mov al, '2'",
    "out dx, al",
    "and rsp, -16",
    // Temporary syscall ABI: report ENOSYS, then return to ring3 with iretq.
    // This avoids SYSRET selector-layout constraints while GDT still uses the
    // natural iret layout (user code before user data).
    "mov rax, -38",
    "mov rdx, [rip + SAIOS_SYSCALL_USER_RSP]",
    "mov dx, 0x3f8",
    "mov al, '3'",
    "out dx, al",
    "push {user_data}",
    "push rdx",
    "push r11", // saved user RFLAGS from SYSCALL
    "push {user_code}",
    "push rcx", // saved user RIP from SYSCALL
    "mov dx, 0x3f8",
    "mov al, '4'",
    "out dx, al",
    "iretq",
    user_data = const USER_DATA_SELECTOR,
    user_code = const USER_CODE_SELECTOR,
);

unsafe extern "C" {
    fn saios_syscall_entry();
}

pub fn set_kernel_rsp0(rsp0: u64) {
    SAIOS_SYSCALL_RSP0.store(rsp0, Ordering::Release);
}

pub fn snapshot() -> SyscallMsrSnapshot {
    let star = msr::rdmsr(IA32_STAR);
    let syscall_kernel_cs = ((star >> 32) & 0xffff) as u16;
    let syscall_kernel_ss = syscall_kernel_cs.wrapping_add(8);
    let sysret_base = ((star >> 48) & 0xffff) as u16;
    let sysret_user_cs = sysret_base.wrapping_add(16) | 3;
    let sysret_user_ss = sysret_base.wrapping_add(8) | 3;

    SyscallMsrSnapshot {
        efer: msr::rdmsr(IA32_EFER),
        star,
        lstar: msr::rdmsr(IA32_LSTAR),
        fmask: msr::rdmsr(IA32_FMASK),
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

    let mut efer = msr::rdmsr(IA32_EFER);
    efer |= EFER_SCE;

    msr::wrmsr(IA32_STAR, star);
    msr::wrmsr(IA32_LSTAR, lstar);
    msr::wrmsr(IA32_FMASK, RFLAGS_IF);
    msr::wrmsr(IA32_EFER, efer);

    Ok(())
}
