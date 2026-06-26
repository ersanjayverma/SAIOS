use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub static PANIC_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static PANIC_OWNER_CPU: AtomicU32 = AtomicU32::new(u32::MAX);
pub static PANIC_OWNER_PID: AtomicU32 = AtomicU32::new(0);
pub static PANIC_RIP: AtomicU64 = AtomicU64::new(0);
pub static PANIC_TIME: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub struct PanicSnapshot {
    pub owner_cpu: u32,
    pub owner_pid: u32,
    pub rip: u64,
    pub time: u64,
}

pub fn record(owner_cpu: u32, owner_pid: u32, rip: u64, time: u64) {
    PANIC_OWNER_CPU.store(owner_cpu, Ordering::Relaxed);
    PANIC_OWNER_PID.store(owner_pid, Ordering::Relaxed);
    PANIC_RIP.store(rip, Ordering::Relaxed);
    PANIC_TIME.store(time, Ordering::Relaxed);
    PANIC_ACTIVE.store(true, Ordering::Release);
}

pub fn snapshot() -> Option<PanicSnapshot> {
    if !PANIC_ACTIVE.load(Ordering::Acquire) {
        return None;
    }
    Some(PanicSnapshot {
        owner_cpu: PANIC_OWNER_CPU.load(Ordering::Relaxed),
        owner_pid: PANIC_OWNER_PID.load(Ordering::Relaxed),
        rip: PANIC_RIP.load(Ordering::Relaxed),
        time: PANIC_TIME.load(Ordering::Relaxed),
    })
}

pub fn sairu_failure_snapshot() -> Option<PanicSnapshot> {
    snapshot()
}
