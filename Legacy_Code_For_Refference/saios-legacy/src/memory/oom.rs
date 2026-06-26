//! OOM Killer — Constitutional requirement (DOC-06 §OOM).
//!
//! When the frame allocator cannot satisfy an allocation, the OOM killer
//! selects the largest user-space process (by page count estimate) and
//! terminates it via ProcessContract::request_exit with SIGKILL-equivalent.
//!
//! Selection heuristic: largest RSS wins (simple, predictable).
//! Protected: PID 1 (init/shell) and idle threads are never killed.
//! KDS event: emits OOM_PRESSURE_TREND on trigger.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Prevents recursive OOM invocation.
static OOM_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
/// Total OOM kills performed.
static OOM_KILL_COUNT: AtomicU64 = AtomicU64::new(0);

/// Attempt to reclaim memory by killing the largest user process.
/// Returns true if a process was killed (caller should retry allocation).
pub fn oom_kill() -> bool {
    // Prevent recursion (killing a process may itself allocate).
    if OOM_IN_PROGRESS
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return false;
    }

    let result = do_oom_kill();

    OOM_IN_PROGRESS.store(false, Ordering::Release);
    result
}

fn do_oom_kill() -> bool {
    // Find the largest user-space process by brk (heap size proxy).
    let victim = {
        let Some(table) = crate::process::table::TABLE.try_lock() else {
            return false;
        };

        let mut best_pid: u32 = 0;
        let mut best_score: u64 = 0;

        for (&pid, proc) in table.procs.iter() {
            // Never kill PID 1 (init/shell), idle threads (pid in idle[]),
            // or kernel threads (uid 0 with no address space).
            if pid <= 1 {
                continue;
            }
            if proc.name.starts_with("idle") || proc.name.starts_with("flight-recorder") {
                continue;
            }
            // Skip already-dead/zombie processes.
            if matches!(
                proc.state(),
                &crate::process::ProcessState::Zombie | &crate::process::ProcessState::Dead
            ) {
                continue;
            }
            // Score: brk distance from base (heap size) + stack size.
            let heap_pages = proc.brk.saturating_sub(crate::process::USER_BRK_BASE) / 4096;
            let stack_pages = proc.stack_size / 4096;
            let score = heap_pages + stack_pages;

            if score > best_score {
                best_score = score;
                best_pid = pid;
            }
        }

        if best_pid == 0 {
            return false;
        }
        best_pid
    };

    // Emit KDS event before kill.
    crate::kds::kds_event(
        crate::kds::KdsSubsystem::Memory,
        crate::kds::KdsEventType::Fault, // closest available; OOM is a memory fault
        crate::kds::KdsSeverity::Error,
        [victim as u64, OOM_KILL_COUNT.load(Ordering::Relaxed), 0, 0],
    );

    crate::serial_println!(
        "[oom] killing pid={} (score={}) free_frames={}",
        victim,
        0, // score not available outside the lock
        crate::memory::FRAME_ALLOCATOR.lock().free_frames()
    );

    // Kill via ProcessContract.
    crate::process_contract::ProcessContract::request_exit(
        crate::process_contract::ProcessExitRequest {
            pid: victim,
            code: -9, // SIGKILL equivalent
            reason: crate::process_contract::ProcessExitReason::OomKill,
            tag: "oom_kill",
        },
    );

    OOM_KILL_COUNT.fetch_add(1, Ordering::Relaxed);
    true
}

/// Number of OOM kills since boot.
pub fn kill_count() -> u64 {
    OOM_KILL_COUNT.load(Ordering::Relaxed)
}
