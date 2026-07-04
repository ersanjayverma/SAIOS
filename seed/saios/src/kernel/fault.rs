//! Kernel fault policy scaffolding for v0.3 readiness.
//!
//! This module provides a non-destructive fault policy contract that validation
//! can assert before full user-mode fault containment is implemented.

use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

const PAGE_FAULT_USER_BIT: usize = 1 << 2;

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
}

pub fn domain_from_page_fault_error(error_code: usize) -> FaultDomain {
    if (error_code & PAGE_FAULT_USER_BIT) != 0 {
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
