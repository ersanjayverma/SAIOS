//! Kernel work pool — exposes all online CPUs to deferred kernel work.
//!
//! The pool spawns persistent, non-pinned `kworker` threads.  They pull closures
//! off a shared queue, execute them in parallel wherever the SMP scheduler places
//! them, then sleep when there is nothing to do (like Linux's kworker/N).
//!
//! Any kernel code can offload CPU-bound work with `kwork::submit(|| ...)`.

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

type Job = Box<dyn FnOnce() + Send + 'static>;

static QUEUE: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());
static WAITERS: Mutex<alloc::vec::Vec<u32>> = Mutex::new(alloc::vec::Vec::new());
static SUBMITTED: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);

/// Submit a job to run on any available worker core.
pub fn submit<F: FnOnce() + Send + 'static>(f: F) {
    let wake_pid = {
        let mut queue = QUEUE.lock();
        queue.push_back(Box::new(f));
        WAITERS.lock().pop()
    };
    SUBMITTED.fetch_add(1, Ordering::Relaxed);
    if let Some(pid) = wake_pid {
        let mut table = crate::process::table::TABLE.lock();
        let _ = crate::process_contract::ProcessContract::wake_pid(
            &mut table,
            pid,
            "kwork submit wake",
        );
    }
}

/// (submitted, completed) job counters — for `cpus`/diagnostics.
pub fn stats() -> (u64, u64) {
    (
        SUBMITTED.load(Ordering::Relaxed),
        COMPLETED.load(Ordering::Relaxed),
    )
}

fn block_until_work() {
    let Some(pid) = crate::process::current_pid() else {
        crate::process::scheduler::yield_now();
        return;
    };

    let should_schedule = crate::arch::without_interrupts(|| {
        let queue = QUEUE.lock();
        if !queue.is_empty() {
            return false;
        }

        let mut waiters = WAITERS.lock();
        if !waiters.contains(&pid) {
            waiters.push(pid);
        }

        let mut table = crate::process::table::TABLE.lock();
        if crate::process_contract::ProcessContract::block_current(
            &mut table,
            "kwork block until work",
        )
        .is_none()
        {
            waiters.retain(|&waiter| waiter != pid);
            false
        } else {
            true
        }
    });

    if should_schedule {
        crate::process::scheduler::schedule_blocking_from("kwork_block");
        WAITERS.lock().retain(|&waiter| waiter != pid);
    }
}

/// A worker thread: pull a job and run it, else block until new work arrives.
pub extern "C" fn kworker_thread() {
    loop {
        let job = QUEUE.lock().pop_front();
        match job {
            Some(j) => {
                j();
                COMPLETED.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                block_until_work();
            }
        }
    }
}

/// Spawn one worker per online CPU.  The scheduler decides where they run;
/// shell/bg threads stay pinned to the BSP, but the pool should expose all
/// available CPU capacity instead of reserving one core up front.
pub fn start_pool() {
    let cpus = crate::smp::cpu_count();
    let workers = cpus.max(1);
    crate::arch::without_interrupts(|| {
        for _ in 0..workers {
            crate::process::kthread::spawn("kworker", kworker_thread);
        }
    });
    crate::println!("[kwork] {} worker thread(s) for {} core(s)", workers, cpus);
}
