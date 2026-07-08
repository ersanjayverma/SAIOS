//! Kernel fault policy scaffolding for v0.3 readiness.
//!
//! This module provides a non-destructive fault policy contract that validation
//! can assert before full user-mode fault containment is implemented.

use crate::kernel::constants::PF_ERR_USER;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use hal::arch::paging;
use hal::arch::x86_64::sync::StaticCell;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum FaultDomain {
    Kernel,
    User,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct FaultSnapshot {
    pub address: usize,
    pub error_code: usize,
    pub domain: FaultDomain,
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static HAS_POLICY: AtomicBool = AtomicBool::new(false);
static LAST_FAULT: StaticCell<Option<FaultSnapshot>> = StaticCell::new(None);
static LAST_FAULT_LOCK: AtomicBool = AtomicBool::new(false);
static ACTIVE_EXEC_PID: StaticCell<Option<u64>> = StaticCell::new(None);
static ACTIVE_EXEC_FAULTED: AtomicBool = AtomicBool::new(false);
/// Lock-free mirror of `ACTIVE_EXEC_PID` (0 = none, else pid). Real pids are
/// never 0, so 0 is a safe "no active process" sentinel. This exists so
/// interrupt handlers (e.g. the timer ISR) can check the active pid without
/// ever taking `LAST_FAULT_LOCK` — that spinlock is also held briefly by
/// ordinary kernel code in `begin_user_exec`/`end_user_exec`, and a plain
/// hardware interrupt gate masks further interrupts on entry, so an ISR that
/// blocks on the same lock while it happens to be held by the code it just
/// preempted deadlocks the core forever with no further output.
static ACTIVE_EXEC_PID_ATOMIC: AtomicU64 = AtomicU64::new(0);

fn lock() {
    while LAST_FAULT_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LAST_FAULT_LOCK.store(false, Ordering::Release);
}

pub fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    HAS_POLICY.store(true, Ordering::Release);
    hal::arch::x86_64::idt::set_invalid_opcode_handler(handle_invalid_opcode);
    hal::arch::x86_64::idt::set_general_protection_handler(handle_general_protection);
    hal::arch::x86_64::idt::set_page_fault_handler(handle_page_fault);
    hal::arch::x86_64::idt::set_user_fault_abort_handler(abort_active_exec);
}

pub fn domain_from_page_fault_error(error_code: usize) -> FaultDomain {
    if (error_code & PF_ERR_USER) != 0 {
        FaultDomain::User
    } else {
        FaultDomain::Kernel
    }
}

pub fn record_page_fault(address: usize, error_code: usize) {
    let snapshot = FaultSnapshot {
        address,
        error_code,
        domain: domain_from_page_fault_error(error_code),
    };

    lock();
    // SAFETY: singleton guarded by spin lock.
    unsafe {
        *LAST_FAULT.get() = Some(snapshot);
    }
    unlock();
}

pub fn last_fault() -> Option<FaultSnapshot> {
    lock();
    // SAFETY: singleton guarded by spin lock.
    let out = unsafe { *LAST_FAULT.get() };
    unlock();
    out
}

pub fn policy_ready() -> bool {
    INITIALIZED.load(Ordering::Acquire) && HAS_POLICY.load(Ordering::Acquire)
}

pub fn begin_user_exec(pid: u64) {
    lock();
    unsafe {
        *ACTIVE_EXEC_PID.get() = Some(pid);
    }
    ACTIVE_EXEC_FAULTED.store(false, Ordering::Release);
    unlock();
    ACTIVE_EXEC_PID_ATOMIC.store(pid, Ordering::Release);
}

pub fn end_user_exec() {
    ACTIVE_EXEC_PID_ATOMIC.store(0, Ordering::Release);
    lock();
    unsafe {
        *ACTIVE_EXEC_PID.get() = None;
    }
    unlock();
}

pub fn active_exec_pid() -> Option<u64> {
    lock();
    let pid = unsafe { *ACTIVE_EXEC_PID.get() };
    unlock();
    pid
}

/// Lock-free variant safe to call from interrupt context. See
/// `ACTIVE_EXEC_PID_ATOMIC` for why this must not take `LAST_FAULT_LOCK`.
pub fn active_exec_pid_lockfree() -> Option<u64> {
    match ACTIVE_EXEC_PID_ATOMIC.load(Ordering::Acquire) {
        0 => None,
        pid => Some(pid),
    }
}

pub fn mark_active_exec_faulted() {
    ACTIVE_EXEC_FAULTED.store(true, Ordering::Release);
}

pub fn take_active_exec_faulted() -> bool {
    ACTIVE_EXEC_FAULTED.swap(false, Ordering::AcqRel)
}

fn handle_page_fault(fault_addr: usize, error_code: usize, stack_ptr: usize) -> bool {
    record_page_fault(fault_addr, error_code);

    if active_exec_pid().is_none() {
        return false;
    }

    let _ = stack_ptr;

    // Proof marker: page fault occurred during active user execution window.
    crate::console::put_char('F');

    mark_active_exec_faulted();
    true
}

fn handle_invalid_opcode(stack_ptr: usize) -> bool {
    if active_exec_pid().is_none() {
        return false;
    }

    let _ = stack_ptr;
    // Proof marker: invalid opcode during active user execution.
    crate::console::put_char('U');
    mark_active_exec_faulted();
    true
}

fn handle_general_protection(_error_code: usize, stack_ptr: usize) -> bool {
    if active_exec_pid().is_none() {
        return false;
    }

    let _ = stack_ptr;
    // Proof marker: general protection fault during active user execution.
    crate::console::put_char('G');
    mark_active_exec_faulted();
    true
}

pub extern "C" fn abort_active_exec() -> ! {
    const ADDR_MASK: u64 = crate::vmm::ADDR_MASK;

    let pid = active_exec_pid();
    // Proof marker: escalated user fault abort path entered.
    crate::console::put_char('X');
    mark_active_exec_faulted();
    crate::console::println!(
        "fault: abort_active_exec pid={} cr3=0x{:x}",
        pid.unwrap_or(0),
        paging::read_cr3() & ADDR_MASK
    );

    // Fault recovery runs while the isolated process CR3 is still active.
    // Switch back to the kernel root before touching process/scheduler state.
    let kernel_cr3 = crate::vmm::stats().cr3 & ADDR_MASK;
    let current_cr3 = paging::read_cr3() & ADDR_MASK;
    if kernel_cr3 != 0 && kernel_cr3 != current_cr3 {
        unsafe {
            paging::write_cr3(kernel_cr3);
        }
    }

    end_user_exec();

    if let Some(pid) = pid {
        let _ = crate::kernel::process::kill(pid);
    }

    hal::arch::x86_64::seed_support::resume_from_user_fault()
}
