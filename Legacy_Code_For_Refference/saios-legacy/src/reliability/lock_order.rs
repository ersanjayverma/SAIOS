//! Lock Order Validator — Constitutional Gate 2 requirement.
//!
//! Enforces a global lock acquisition ordering to prevent deadlocks.
//! Every kernel lock has an assigned priority level (1-10, lower = acquired first).
//! Acquiring a lock whose priority is ≤ any currently-held lock on this CPU
//! is a violation and triggers a panic (Red Ring).
//!
//! Constitutional reference: SSOT §Gate 2, DOC-16 §Lock Order.

use crate::process::table::MAX_CPUS;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Maximum lock nesting depth per CPU.
const MAX_HELD: usize = 16;

/// Lock priority levels (lower number = acquired first in valid ordering).
/// Acquiring a lock with priority ≤ highest currently held is a violation.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockClass {
    /// Priority 1: KDS ring guards (interrupt-safe, never blocks).
    KdsRing = 1,
    /// Priority 2: Per-CPU state locks.
    PerCpu = 2,
    /// Priority 3: Scheduler run-queue.
    Scheduler = 3,
    /// Priority 4: Process table.
    ProcessTable = 4,
    /// Priority 5: Memory frame allocator.
    FrameAllocator = 5,
    /// Priority 6: VFS inode/dentry locks.
    Vfs = 6,
    /// Priority 7: IPC (pipe, futex) locks.
    Ipc = 7,
    /// Priority 8: Network socket locks.
    Network = 8,
    /// Priority 9: Driver locks.
    Driver = 9,
    /// Priority 10: Console/serial output.
    Console = 10,
}

/// Per-CPU lock ordering state.  Tracks which lock classes are currently held.
struct CpuLockState {
    /// Stack of held lock priorities (0 = unused slot).
    held: [AtomicU8; MAX_HELD],
    /// Number of locks currently held on this CPU.
    depth: AtomicU8,
}

impl CpuLockState {
    const fn new() -> Self {
        Self {
            held: [const { AtomicU8::new(0) }; MAX_HELD],
            depth: AtomicU8::new(0),
        }
    }

    /// Returns the highest (numerically largest) lock class currently held.
    fn max_held(&self) -> u8 {
        let depth = self.depth.load(Ordering::Relaxed) as usize;
        let mut max = 0u8;
        for i in 0..depth.min(MAX_HELD) {
            let v = self.held[i].load(Ordering::Relaxed);
            if v > max {
                max = v;
            }
        }
        max
    }
}

static CPU_LOCK_STATE: [CpuLockState; MAX_CPUS] = [const { CpuLockState::new() }; MAX_CPUS];
static VALIDATOR_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Install the lock order validator.  Called at Gate 2 during boot.
/// After this call, all lock_acquire/lock_release calls are validated.
pub fn install() {
    VALIDATOR_ACTIVE.store(true, Ordering::Release);
}

/// Returns true if the validator is installed and active.
pub fn is_active() -> bool {
    VALIDATOR_ACTIVE.load(Ordering::Acquire)
}

/// Record a lock acquisition.  Panics if the acquisition would violate
/// the global lock ordering (acquiring at priority ≤ currently held).
///
/// Call this BEFORE the actual lock acquisition.
#[inline]
pub fn lock_acquire(class: LockClass) {
    if !VALIDATOR_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    let cpu = crate::process::table::cpu_idx();
    let state = &CPU_LOCK_STATE[cpu];
    let max_held = state.max_held();
    let priority = class as u8;

    if priority <= max_held && max_held > 0 {
        // Lock order violation detected.
        crate::serial_println!(
            "[lock-order] VIOLATION cpu={} acquiring={:?}(prio={}) max_held={}",
            cpu,
            class,
            priority,
            max_held,
        );
        crate::kds::kds_event(
            crate::kds::KdsSubsystem::Reliability,
            crate::kds::KdsEventType::LockOrderViolation,
            crate::kds::KdsSeverity::Fatal,
            [cpu as u64, priority as u64, max_held as u64, 0],
        );
        panic!(
            "lock order violation: cpu={} acquiring class {:?} (prio {}) while holding prio {}",
            cpu, class, priority, max_held
        );
    }

    // Push onto the held stack.
    let depth = state.depth.load(Ordering::Relaxed) as usize;
    if depth < MAX_HELD {
        state.held[depth].store(priority, Ordering::Relaxed);
        state.depth.store((depth + 1) as u8, Ordering::Relaxed);
    }
}

/// Record a lock release.  Removes the given class from the held stack.
///
/// Call this AFTER the actual lock release.
#[inline]
pub fn lock_release(class: LockClass) {
    if !VALIDATOR_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    let cpu = crate::process::table::cpu_idx();
    let state = &CPU_LOCK_STATE[cpu];
    let depth = state.depth.load(Ordering::Relaxed) as usize;
    let priority = class as u8;

    // Find and remove the matching entry (LIFO expected, but handle out-of-order).
    for i in (0..depth.min(MAX_HELD)).rev() {
        if state.held[i].load(Ordering::Relaxed) == priority {
            // Swap with last entry and shrink.
            let last = depth.saturating_sub(1);
            if i != last {
                let last_val = state.held[last].load(Ordering::Relaxed);
                state.held[i].store(last_val, Ordering::Relaxed);
            }
            state.held[last].store(0, Ordering::Relaxed);
            state.depth.store(last as u8, Ordering::Relaxed);
            return;
        }
    }
}

/// Query the current lock depth on this CPU (for Red Ring evidence).
pub fn current_depth() -> u8 {
    let cpu = crate::process::table::cpu_idx();
    CPU_LOCK_STATE[cpu].depth.load(Ordering::Relaxed)
}

/// Query the max held priority on this CPU.
pub fn current_max_held() -> u8 {
    let cpu = crate::process::table::cpu_idx();
    CPU_LOCK_STATE[cpu].max_held()
}
