//! x86_64 syscall setup and entry-stack plumbing.

use crate::gdt::{KERNEL_CS, KERNEL_SS, USER_CS, USER_DS};
use crate::process::table::{MAX_CPUS, cpu_idx};
use core::sync::atomic::{AtomicBool, Ordering};
use x86_64::VirtAddr;
use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;

unsafe extern "C" {
    fn syscall_entry();
}

#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct SyscallCpuState {
    kernel_rsp: u64,
    user_rsp_save: u64,
    saved_rip: u64,
    saved_rflags: u64,
    saved_rdi: u64,
    saved_rsi: u64,
    saved_rdx: u64,
    saved_r8: u64,
    saved_r9: u64,
    saved_r10: u64,
    saved_rbx: u64,
    saved_rbp: u64,
    saved_r12: u64,
    saved_r13: u64,
    saved_r14: u64,
    saved_r15: u64,
    entry_gs0_probe: u64,
    entry_gs8_probe: u64,
    entry_gs16_probe: u64,
}

const EMPTY_SYSCALL_CPU_STATE: SyscallCpuState = SyscallCpuState {
    kernel_rsp: 0,
    user_rsp_save: 0,
    saved_rip: 0,
    saved_rflags: 0,
    saved_rdi: 0,
    saved_rsi: 0,
    saved_rdx: 0,
    saved_r8: 0,
    saved_r9: 0,
    saved_r10: 0,
    saved_rbx: 0,
    saved_rbp: 0,
    saved_r12: 0,
    saved_r13: 0,
    saved_r14: 0,
    saved_r15: 0,
    entry_gs0_probe: 0,
    entry_gs8_probe: 0,
    entry_gs16_probe: 0,
};

#[unsafe(no_mangle)]
pub static mut _syscall_cpu_state: [SyscallCpuState; MAX_CPUS] =
    [EMPTY_SYSCALL_CPU_STATE; MAX_CPUS];

static KERNEL_GS_ACTIVE: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];

#[inline]
fn cpu_state_ptr(cpu: usize) -> *mut SyscallCpuState {
    unsafe {
        core::ptr::addr_of_mut!(_syscall_cpu_state)
            .cast::<SyscallCpuState>()
            .add(cpu)
    }
}

#[inline]
fn current_cpu_state() -> SyscallCpuState {
    unsafe { core::ptr::read_volatile(cpu_state_ptr(cpu_idx())) }
}

pub fn init() {
    unsafe {
        Efer::update(|e| {
            *e |= EferFlags::SYSTEM_CALL_EXTENSIONS | EferFlags::NO_EXECUTE_ENABLE;
        });
        Star::write(
            x86_64::structures::gdt::SegmentSelector(USER_CS),
            x86_64::structures::gdt::SegmentSelector(USER_DS),
            x86_64::structures::gdt::SegmentSelector(KERNEL_CS),
            x86_64::structures::gdt::SegmentSelector(KERNEL_SS),
        )
        .expect("syscall STAR write failed");
        LStar::write(VirtAddr::new(syscall_entry as *const () as u64));
        SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::TRAP_FLAG);
        crate::arch::process::set_kernel_gs_base(cpu_state_ptr(cpu_idx()) as u64);
    }
    crate::serial_println!(
        "[syscall] cpu{} Linux ABI ready ({} syscalls)",
        cpu_idx(),
        300
    );
}

pub fn install_kernel_gs_base() {
    unsafe {
        crate::arch::process::set_kernel_gs_base(cpu_state_ptr(cpu_idx()) as u64);
    }
}

pub fn mark_kernel_gs_active(active: bool) {
    KERNEL_GS_ACTIVE[cpu_idx()].store(active, Ordering::Relaxed);
}

pub fn kernel_gs_active() -> bool {
    KERNEL_GS_ACTIVE[cpu_idx()].load(Ordering::Relaxed)
}

pub fn kernel_gs_active_ptr() -> *mut bool {
    KERNEL_GS_ACTIVE[cpu_idx()].as_ptr()
}

pub fn set_kernel_stack(rsp: u64) {
    install_kernel_gs_base();
    unsafe { (*cpu_state_ptr(cpu_idx())).kernel_rsp = rsp }
    crate::gdt::set_kernel_stack(rsp);
}

pub fn saved_user_caller_regs() -> (u64, u64, u64, u64, u64, u64) {
    let state = current_cpu_state();
    (
        state.saved_rdi,
        state.saved_rsi,
        state.saved_rdx,
        state.saved_r8,
        state.saved_r9,
        state.saved_r10,
    )
}

pub fn saved_user_callee_regs() -> (u64, u64, u64, u64, u64, u64) {
    let state = current_cpu_state();
    (
        state.saved_rbx,
        state.saved_rbp,
        state.saved_r12,
        state.saved_r13,
        state.saved_r14,
        state.saved_r15,
    )
}

pub fn saved_user_syscall_site() -> (u64, u64, u64) {
    let state = current_cpu_state();
    (state.saved_rip, state.user_rsp_save, state.saved_rflags)
}

pub fn syscall_entry_probes() -> (u64, u64, u64) {
    let state = current_cpu_state();
    (
        state.entry_gs0_probe,
        state.entry_gs8_probe,
        state.entry_gs16_probe,
    )
}
