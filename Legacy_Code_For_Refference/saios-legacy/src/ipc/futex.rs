//! Futex — fast user-space wait/wake primitive for the SAIOS syscall ABI.
//! Intended to support libc and pthread-style runtimes as semantics mature.
//!
//! This implementation uses a wait queue mechanism to park threads and wake them
//! when the futex value changes, avoiding spin-waiting.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

/// Wait queue entry for a blocked thread.
#[derive(Default)]
struct FutexWaiter {
    pid: u32,
}

/// Global futex table mapping address → waiters
static FUTEX_TABLE: Mutex<BTreeMap<u64, Vec<FutexWaiter>>> = Mutex::new(BTreeMap::new());

/// Thread wait queue for park/unpark
struct ThreadWaiter {
    pid: u32,
    unparked: bool,
}

static THREAD_WAIT_QUEUE: Mutex<BTreeMap<u32, ThreadWaiter>> = Mutex::new(BTreeMap::new());

const FUTEX_WAIT: u32 = 0;
const FUTEX_WAKE: u32 = 1;
const FUTEX_WAIT_PRIVATE: u32 = 128;
const FUTEX_WAKE_PRIVATE: u32 = 129;
const FUTEX_PRIVATE: u32 = 128;

pub fn sys_futex(uaddr: u64, op: u32, val: u32, _timeout: u64, _uaddr2: u64, val3: u32) -> i64 {
    let op = op & !FUTEX_PRIVATE; // strip PRIVATE flag — same semantics for single-process
    match op {
        FUTEX_WAIT => {
            // Read the value at uaddr
            let current = unsafe { core::ptr::read_volatile(uaddr as *const u32) };
            if current != val {
                return -11; // EAGAIN — value already changed
            }

            let Some(pid) = crate::process::current_pid() else {
                return -22; // EINVAL - no current schedulable task
            };

            {
                let mut table = FUTEX_TABLE.lock();
                let waiters = table.entry(uaddr).or_default();
                if !waiters.iter().any(|w| w.pid == pid) {
                    waiters.push(FutexWaiter { pid });
                }
            }

            crate::kds::kds_event(
                crate::kds::KdsSubsystem::Ipc,
                crate::kds::KdsEventType::FutexContention,
                crate::kds::KdsSeverity::Trace,
                [pid as u64, uaddr, val as u64, 0],
            );

            loop {
                let interrupted =
                    crate::process::with_current_process(|proc| proc.signals.is_pending())
                        .unwrap_or(false);
                if interrupted {
                    let mut table = FUTEX_TABLE.lock();
                    if let Some(waiters) = table.get_mut(&uaddr) {
                        waiters.retain(|w| w.pid != pid);
                        if waiters.is_empty() {
                            table.remove(&uaddr);
                        }
                    }
                    return -4; // EINTR
                }

                let still_waiting = {
                    let table = FUTEX_TABLE.lock();
                    table
                        .get(&uaddr)
                        .map(|waiters| waiters.iter().any(|w| w.pid == pid))
                        .unwrap_or(false)
                };

                if !still_waiting {
                    return 0;
                }

                crate::process::block_current();
            }
        }
        FUTEX_WAKE => {
            let wake_pids = {
                let mut table = FUTEX_TABLE.lock();
                let Some(waiters) = table.get_mut(&uaddr) else {
                    return 0;
                };

                let to_wake = (val as usize).min(waiters.len());
                let mut pids = Vec::with_capacity(to_wake);
                for waiter in waiters.drain(..to_wake) {
                    pids.push(waiter.pid);
                }

                if waiters.is_empty() {
                    table.remove(&uaddr);
                }
                pids
            };

            let mut woken_count = 0;
            let mut proc_table = crate::process::table::TABLE.lock();
            for pid in wake_pids {
                if crate::process_contract::ProcessContract::wake_pid(
                    &mut proc_table,
                    pid,
                    "futex wake",
                ) {
                    woken_count += 1;
                }
            }
            woken_count as i64
        }
        _ => -38, // ENOSYS
    }
}

/// Park the current thread until it is unparked by another thread.
/// Used for implementing proper sleep in futex and other synchronization.
pub fn park_thread(timeout_ms: u64) -> bool {
    let Some(pid) = crate::process::current_pid() else {
        return false;
    };

    // Add to wait queue
    {
        let mut queue = THREAD_WAIT_QUEUE.lock();
        queue.insert(
            pid,
            ThreadWaiter {
                pid,
                unparked: false,
            },
        );
    }

    // Wait for unpark or timeout
    let max_loops = if timeout_ms == 0 {
        100000 // No timeout - wait indefinitely (but still check periodically)
    } else {
        timeout_ms * 100 // Convert ms to loops (approx)
    };

    let mut unparked = false;
    for _ in 0..max_loops {
        let interrupted =
            crate::process::with_current_process(|proc| proc.signals.is_pending()).unwrap_or(false);
        if interrupted {
            break;
        }
        {
            let queue = THREAD_WAIT_QUEUE.lock();
            if let Some(waiter) = queue.get(&pid)
                && waiter.unparked
            {
                unparked = true;
                break;
            }
        }
        x86_64::instructions::hlt();
    }

    // Remove from queue
    {
        let mut queue = THREAD_WAIT_QUEUE.lock();
        queue.remove(&pid);
    }

    unparked
}

/// Unpark a thread by PID.
/// Returns true if the thread was found and unparked.
pub fn unpark_thread(pid: u32) -> bool {
    let mut queue = THREAD_WAIT_QUEUE.lock();
    if let Some(waiter) = queue.get_mut(&pid) {
        waiter.unparked = true;
        return true;
    }
    false
}

/// Wake all threads waiting on a futex address.
/// Returns the number of threads woken.
pub fn futex_wake_all(uaddr: u64) -> i64 {
    let wake_pids = {
        let mut table = FUTEX_TABLE.lock();
        table
            .remove(&uaddr)
            .unwrap_or_default()
            .into_iter()
            .map(|waiter| waiter.pid)
            .collect::<Vec<_>>()
    };

    let mut woken_count = 0;
    let mut proc_table = crate::process::table::TABLE.lock();
    for pid in wake_pids {
        if crate::process_contract::ProcessContract::wake_pid(
            &mut proc_table,
            pid,
            "futex wake all",
        ) {
            woken_count += 1;
        }
    }
    woken_count as i64
}
