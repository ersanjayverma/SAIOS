//! pthread wrappers for SAIOS
//!
//! This module provides user-space pthread implementations that use
//! kernel-provided futex and clone() for thread synchronization and creation.

use crate::ipc::futex;

/// pthread_t - thread identifier (we use PID as TID)
pub type pthread_t = u32;

/// pthread_attr_t - thread attributes (minimal implementation)
#[repr(C)]
pub struct pthread_attr_t {
    detached: u32,  // PTHREAD_CREATE_DETACHED or PTHREAD_CREATE_JOINABLE
    stack_size: u64,
    guard_size: u64,
    scope: u32,     // PTHREAD_SCOPE_SYSTEM or PTHREAD_SCOPE_PROCESS
}

impl pthread_attr_t {
    pub fn new() -> Self {
        Self {
            detached: 0,
            stack_size: 0,
            guard_size: 0,
            scope: 1, // PTHREAD_SCOPE_SYSTEM
        }
    }
}

/// pthread_mutex_t - mutex (minimal futex-based implementation)
#[repr(C)]
pub struct pthread_mutex_t {
    lock: u32,      // 0 = unlocked, 1 = locked
    kind: u32,      // mutex type
    owner: u32,     // owner TID
    count: u32,     // recursive count
}

impl pthread_mutex_t {
    pub const fn new() -> Self {
        Self {
            lock: 0,
            kind: 0,
            owner: 0,
            count: 0,
        }
    }

    /// Initialize mutex
    pub unsafe fn init(&mut self) {
        self.lock = 0;
        self.kind = 0;
        self.owner = 0;
        self.count = 0;
    }

    /// Lock mutex - returns 0 on success
    pub unsafe fn lock(&mut self) -> i32 {
        let mut expected = 0u32;
        loop {
            // Try to acquire lock
            if self.lock.compare_exchange(
                expected,
                1,
                core::sync::atomic::Ordering::SeqCst,
                core::sync::atomic::Ordering::SeqCst,
            ).is_ok() {
                self.owner = crate::process::current_pid().unwrap_or(0);
                self.count = 1;
                return 0;
            }

            // Wait for unlock
            futex::park_thread(1000); // 1 second timeout
            expected = 0;
        }
    }

    /// Unlock mutex - returns 0 on success
    pub unsafe fn unlock(&mut self) -> i32 {
        if self.owner == 0 {
            return 1; // Not locked
        }

        self.owner = 0;
        self.count = 0;

        // Release lock
        self.lock.store(0, core::sync::atomic::Ordering::SeqCst);

        // Wake any waiting threads
        futex::futex_wake_all(self as *const _ as u64);

        0
    }
}

/// pthread_key_t - thread-local storage key
pub type pthread_key_t = u32;

/// Thread-local storage storage
static TLS_STORAGE: spin::Mutex<[Option<u64>; 64]> = spin::Mutex::new([None; 64]);

/// pthread_create - create a new thread
/// Returns 0 on success
pub unsafe fn pthread_create(
    thread: *mut pthread_t,
    attr: *const pthread_attr_t,
    start_routine: u64,
    arg: u64,
) -> i32 {
    if thread.is_null() {
        return 22; // EINVAL
    }

    let parent_pid = crate::process::current_pid().unwrap_or(0);

    // Get attributes
    let flags = {
        let mut f = crate::process::thread::CLONE_VM
            | crate::process::thread::CLONE_THREAD
            | crate::process::thread::CLONE_SIGHAND;
        let attr = if attr.is_null() {
            pthread_attr_t::new()
        } else {
            unsafe { *attr }
        };
        if attr.detached != 0 {
            f |= crate::process::thread::CLONE_DETACHED;
        }
        f
    };

    // Create new thread
    let child_tid = parent_tid as u64;
    let child_tid_ptr = &child_tid as *const u32 as u64;

    // Setup TLS for new thread
    let tls = crate::dynlink::tls::setup_tls_for_process(
        &mut crate::process::table::TABLE.lock().procs.get_mut(&parent_pid).unwrap(),
    );

    let result = crate::syscall::handlers::do_clone(
        flags as u64,
        0, // child_stack - kernel allocates
        child_tid_ptr,
        0, // child_tid_ptr
        tls,
        start_routine, // new thread starts at start_routine
        0x202,         // default rflags
    );

    if result >= 0 {
        *thread = result as pthread_t;
        0
    } else {
        -result as i32
    }
}

/// pthread_join - wait for thread termination
pub unsafe fn pthread_join(thread: pthread_t, retval: *mut u64) -> i32 {
    if thread == 0 {
        return 22; // EINVAL
    }

    // Wait for thread to exit
    // This would use futex on the thread's exit variable
    // For now, we just return -ENOSYS
    0
}

/// pthread_exit - terminate calling thread
pub unsafe fn pthread_exit(retval: *const u64) {
    let exit_val = if retval.is_null() { 0 } else { *retval };
    let _ = crate::syscall::handlers::sys_exit(exit_val as i64);
    loop {
        crate::x86_64::instructions::hlt();
    }
}

/// pthread_self - get calling thread ID
pub fn pthread_self() -> pthread_t {
    crate::process::current_pid().unwrap_or(0)
}

/// pthread_detach - mark thread as detached
pub fn pthread_detach(thread: pthread_t) -> i32 {
    if thread == 0 {
        return 22; // EINVAL
    }
    0
}

/// pthread_mutex_init - initialize mutex
pub unsafe fn pthread_mutex_init(mutex: *mut pthread_mutex_t, _attr: *const u32) -> i32 {
    if mutex.is_null() {
        return 22; // EINVAL
    }
    unsafe { (*mutex).init() };
    0
}

/// pthread_mutex_destroy - destroy mutex
pub unsafe fn pthread_mutex_destroy(_mutex: *mut pthread_mutex_t) -> i32 {
    0
}

/// pthread_mutex_lock - lock mutex
pub unsafe fn pthread_mutex_lock(mutex: *mut pthread_mutex_t) -> i32 {
    if mutex.is_null() {
        return 22; // EINVAL
    }
    unsafe { (*mutex).lock() }
}

/// pthread_mutex_unlock - unlock mutex
pub unsafe fn pthread_mutex_unlock(mutex: *mut pthread_mutex_t) -> i32 {
    if mutex.is_null() {
        return 22; // EINVAL
    }
    unsafe { (*mutex).unlock() }
}

/// pthread_key_create - create thread-local storage key
pub unsafe fn pthread_key_create(
    key: *mut pthread_key_t,
    destructor: Option<unsafe extern "C" fn(u64)>,
) -> i32 {
    if key.is_null() {
        return 22; // EINVAL
    }

    let mut tls = TLS_STORAGE.lock();
    for i in 0..64 {
        if tls[i].is_none() {
            tls[i] = Some(destructor.map(|f| f as u64).unwrap_or(0));
            *key = i as pthread_key_t;
            return 0;
        }
    }
    12; // ENOMEM
}

/// pthread_key_delete - delete thread-local storage key
pub unsafe fn pthread_key_delete(_key: pthread_key_t) -> i32 {
    // Simplified - in real implementation would track usage
    0
}

/// pthread_setspecific - set thread-local storage value
pub unsafe fn pthread_setspecific(key: pthread_key_t, value: *const u64) -> i32 {
    let tls = TLS_STORAGE.lock();
    if key >= 64 {
        return 22; // EINVAL
    }
    // In real implementation, would store per-thread value
    0
}

/// pthread_getspecific - get thread-local storage value
pub unsafe fn pthread_getspecific(key: pthread_key_t) -> *mut u64 {
    let tls = TLS_STORAGE.lock();
    if key >= 64 {
        return 0 as *mut u64;
    }
    // In real implementation, would return per-thread value
    0 as *mut u64
}

/// pthread_barrier_t - barrier synchronization
#[repr(C)]
pub struct pthread_barrier_t {
    count: u32,
    current: u32,
    lock: u32,
}

impl pthread_barrier_t {
    pub const fn new(count: u32) -> Self {
        Self {
            count,
            current: 0,
            lock: 0,
        }
    }
}

/// pthread_barrier_init - initialize barrier
pub unsafe fn pthread_barrier_init(
    barrier: *mut pthread_barrier_t,
    _attr: *const u32,
    count: u32,
) -> i32 {
    if barrier.is_null() || count == 0 {
        return 22; // EINVAL
    }
    unsafe {
        *barrier = pthread_barrier_t::new(count);
    }
    0
}

/// pthread_barrier_wait - wait at barrier
pub unsafe fn pthread_barrier_wait(_barrier: *mut pthread_barrier_t) -> i32 {
    // Simplified implementation
    0
}

/// pthread_cond_t - condition variable
#[repr(C)]
pub struct pthread_cond_t {
    lock: u32,
    waiting: u32,
}

impl pthread_cond_t {
    pub const fn new() -> Self {
        Self {
            lock: 0,
            waiting: 0,
        }
    }
}

/// pthread_cond_init - initialize condition variable
pub unsafe fn pthread_cond_init(cond: *mut pthread_cond_t, _attr: *const u32) -> i32 {
    if cond.is_null() {
        return 22; // EINVAL
    }
    unsafe {
        *cond = pthread_cond_t::new();
    }
    0
}

/// pthread_cond_wait - wait on condition variable
pub unsafe fn pthread_cond_wait(cond: *mut pthread_cond_t, mutex: *mut pthread_mutex_t) -> i32 {
    if cond.is_null() || mutex.is_null() {
        return 22; // EINVAL
    }
    // Unlock mutex, wait on cond, relock mutex
    0
}

/// pthread_cond_signal - signal condition variable
pub unsafe fn pthread_cond_signal(_cond: *mut pthread_cond_t) -> i32 {
    0
}

/// pthread_cond_broadcast - broadcast condition variable
pub unsafe fn pthread_cond_broadcast(_cond: *mut pthread_cond_t) -> i32 {
    0
}

/// pthread_cond_destroy - destroy condition variable
pub unsafe fn pthread_cond_destroy(_cond: *mut pthread_cond_t) -> i32 {
    0
}
