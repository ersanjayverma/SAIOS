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

/// Pointer to the saved user register frame on the kernel transition stack.
/// Set by `saios_syscall_entry` before calling `saios_linux_syscall` so that
/// fork/clone handlers can capture a full ring-3 snapshot for child threads.
#[unsafe(no_mangle)]
pub static SAIOS_CURRENT_USER_CTX_FRAME: AtomicU64 = AtomicU64::new(0);

/// Full ring-3 register state saved on the kernel transition stack by
/// `saios_syscall_entry`.  The layout matches the push sequence in the asm:
/// r15..rax (GPRs), then the iretq frame (user_rip, user_cs, user_rflags,
/// user_rsp, user_ss).
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct UserSyscallFrame {
    pub r15:         u64,  // +0
    pub r14:         u64,  // +8
    pub r13:         u64,  // +16
    pub r12:         u64,  // +24
    pub r10:         u64,  // +32
    pub r9:          u64,  // +40
    pub r8:          u64,  // +48
    pub rbp:         u64,  // +56
    pub rdi:         u64,  // +64
    pub rsi:         u64,  // +72
    pub rdx:         u64,  // +80
    pub rbx:         u64,  // +88
    pub rax:         u64,  // +96  (syscall nr / fork return value)
    pub user_rip:    u64,  // +104 (rcx at SYSCALL)
    pub user_cs:     u64,  // +112
    pub user_rflags: u64,  // +120 (r11 at SYSCALL)
    pub user_rsp:    u64,  // +128
    pub user_ss:     u64,  // +136
}

/// Capture the saved user register frame from the *current* syscall.
/// Safe to call only from within `saios_linux_syscall` while the syscall
/// entry frame is live on the kernel transition stack.
pub fn capture_user_syscall_frame() -> UserSyscallFrame {
    let ptr = SAIOS_CURRENT_USER_CTX_FRAME.load(Ordering::Acquire);
    if ptr == 0 {
        return UserSyscallFrame::default();
    }
    // SAFETY: pointer into the live kernel transition stack, valid for the
    // duration of the syscall.
    unsafe { core::ptr::read_volatile(ptr as *const UserSyscallFrame) }
}

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
    // Build the iret frame up front: SS, RSP, RFLAGS, CS, RIP.
    "push {user_data}",
    "push qword ptr [rip + SAIOS_SYSCALL_USER_RSP]",
    "push r11", // user RFLAGS saved by SYSCALL
    "push {user_code}",
    "push rcx", // user RIP saved by SYSCALL
    // Save the full user GPR state. Linux ABI: only rax/rcx/r11 may be
    // clobbered across a syscall; the Rust dispatcher clobbers all
    // caller-saved registers, so everything must be restored from here.
    "push rax",
    "push rbx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    // Save pointer to the saved user-register frame for fork context capture.
    // rsp now points to the saved r15 field; the full UserSyscallFrame layout
    // starts here.  Use rax as a scratch register (it is restored below).
    "lea rax, [rsp]",
    "mov [rip + SAIOS_CURRENT_USER_CTX_FRAME], rax",
    // Linux x86_64 syscall ABI at entry: rax=nr, rdi/rsi/rdx/r10/r8/r9=args.
    // Rust SysV call target: saios_linux_syscall(nr,a0,a1,a2,a3,a4,a5)
    // -> rdi,rsi,rdx,rcx,r8,r9 and 7th arg on stack.
    // Load arguments from the saved user registers on the stack.
    "sub rsp, 16",
    "mov rax, [rsp + 16 + 40]", // user r9  = a5
    "mov [rsp], rax",
    "mov rdi, [rsp + 16 + 96]", // user rax = nr
    "mov rsi, [rsp + 16 + 64]", // user rdi = a0
    "mov rdx, [rsp + 16 + 72]", // user rsi = a1
    "mov rcx, [rsp + 16 + 80]", // user rdx = a2
    "mov r8,  [rsp + 16 + 32]", // user r10 = a3
    "mov r9,  [rsp + 16 + 48]", // user r8  = a4
    "call {dispatch}",
    "add rsp, 16",
    // Store the return value into the saved user rax slot.
    "mov [rsp + 96], rax",
    // Restore the full user register state and return to ring 3.
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rbx",
    "pop rax",
    "mov rcx, [rsp + 0x00]", // user RIP (also restored by iretq frame)
    "mov r11, [rsp + 0x10]", // user RFLAGS
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

    // Prime the default kernel transition stack so `saios_syscall_entry`
    // has a valid destination from the very first ring-3 syscall.
    SAIOS_SYSCALL_RSP0.store(
        crate::arch::x86_64::seed_support::user_transition_kernel_rsp0(),
        Ordering::Release,
    );

    Ok(())
}
