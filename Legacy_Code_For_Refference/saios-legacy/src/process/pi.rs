//! Priority Inheritance (PI) for futex-based mutexes.
//!
//! F-SCHED-09: When a higher-priority process blocks on a mutex held by a
//! lower-priority process, the owner's scheduling score is temporarily boosted
//! to prevent unbounded priority inversion.
//!
//! API:
//! - `pi_boost(owner_pid, waiter_pid)` — boost owner if waiter has higher priority
//! - `pi_unboost(owner_pid)` — remove boost when mutex is released
//! - `pi_score(pid)` — returns the effective boost for pick_next scoring

use crate::process::table::TABLE;

/// Boost the owner's PI score because a waiter is blocked on its mutex.
/// Called from futex_wait when the lock owner is known.
pub fn pi_boost(owner_pid: u32, waiter_pid: u32) {
    if let Some(mut table) = TABLE.try_lock() {
        let waiter_score = table
            .procs
            .get(&waiter_pid)
            .map(|p| p.pi_boost.saturating_add(100)) // waiter's effective priority
            .unwrap_or(0);
        if let Some(owner) = table.procs.get_mut(&owner_pid)
            && waiter_score > owner.pi_boost
        {
            let old = owner.pi_boost;
            owner.pi_boost = waiter_score;
            crate::kds::kds_event(
                crate::kds::KdsSubsystem::Scheduler,
                crate::kds::KdsEventType::State,
                crate::kds::KdsSeverity::Info,
                [
                    owner_pid as u64,
                    waiter_pid as u64,
                    old as u64,
                    waiter_score as u64,
                ],
            );
        }
    }
}

/// Remove the PI boost from a process when it releases the contested mutex.
/// Called from futex_wake / mutex unlock path.
pub fn pi_unboost(owner_pid: u32) {
    if let Some(mut table) = TABLE.try_lock()
        && let Some(owner) = table.procs.get_mut(&owner_pid)
        && owner.pi_boost > 0
    {
        owner.pi_boost = 0;
    }
}

/// Returns the PI boost for a process (used by pick_next scoring).
pub fn pi_score(proc: &crate::process::Process) -> u8 {
    proc.pi_boost
}
