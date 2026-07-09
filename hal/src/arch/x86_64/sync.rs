//! Small synchronization primitives used by early x86_64 HAL code.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

pub struct StaticCell<T>(UnsafeCell<T>);

unsafe impl<T> Sync for StaticCell<T> {}

impl<T> StaticCell<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    pub fn get(&self) -> *mut T {
        self.0.get()
    }
}

/// Acquire a bare `AtomicBool` spinlock (busy-wait until `false → true`).
#[inline(always)]
pub fn spinlock_acquire(lock: &AtomicBool) {
    while lock
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

/// Release a bare `AtomicBool` spinlock.
#[inline(always)]
pub fn spinlock_release(lock: &AtomicBool) {
    lock.store(false, Ordering::Release);
}

/// Run `f` while holding `lock`, returning its result.
#[inline(always)]
pub fn with_spinlock<R>(lock: &AtomicBool, f: impl FnOnce() -> R) -> R {
    spinlock_acquire(lock);
    let result = f();
    spinlock_release(lock);
    result
}

/// Acquire a spinlock with interrupts disabled, returning whether interrupts
/// were enabled beforehand (pass this to [`spinlock_release_irqrestore`]).
///
/// Plain [`spinlock_acquire`] does not touch IF, which is safe only if
/// nothing that can run in interrupt context (a timer tick, an IRQ handler)
/// ever touches the same lock or the data it protects. On a UP kernel like
/// this one, an ISR "racing" a lock held by the code it preempted is just as
/// real a hazard as true multi-core concurrency: the ISR can observe the
/// protected structure (e.g. a `Vec`'s `ptr`/`len`/`cap` triple) mid-mutation,
/// see a torn/inconsistent snapshot, and act on it -- passing a bounds check
/// against a since-grown `len` while `ptr` still points at the old, smaller
/// backing allocation, then faulting on the out-of-range read. This was
/// confirmed to be exactly the shape of a VirtualBox/NEM-specific crash in
/// `vfs::TmpFs` that never reproduced under QEMU (whose deterministic TCG
/// timing apparently never lands a tick inside the vulnerable window).
/// Callers that share state with interrupt-context code (VFS, the kernel
/// heap allocator) should use this pair instead of the plain one.
#[inline(always)]
pub fn spinlock_acquire_irqsave(lock: &AtomicBool) -> bool {
    let was_enabled = crate::arch::x86_64::interrupt::are_enabled();
    crate::arch::x86_64::interrupt::disable();
    while lock
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    was_enabled
}

/// Release a spinlock acquired with [`spinlock_acquire_irqsave`], restoring
/// interrupts to the state `was_enabled` (as returned by that call) rather
/// than unconditionally re-enabling them -- required so nested critical
/// sections (e.g. a VFS operation that triggers a heap allocation) don't
/// prematurely re-enable interrupts while still inside an outer lock.
#[inline(always)]
pub fn spinlock_release_irqrestore(lock: &AtomicBool, was_enabled: bool) {
    lock.store(false, Ordering::Release);
    if was_enabled {
        crate::arch::x86_64::interrupt::enable();
    }
}
