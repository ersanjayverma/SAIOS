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
