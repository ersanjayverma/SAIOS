use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::kernel::event::{self, EventKind};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ProcessState {
    Running,
    Waiting,
    Exited,
}

#[derive(Clone, Debug)]
pub struct ProcessRecord {
    pub pid: u64,
    pub name: String,
    pub state: ProcessState,
    pub thread_count: usize,
    pub exit_code: Option<i32>,
}

struct ProcessManager {
    initialized: bool,
    records: Vec<ProcessRecord>,
    next_pid: u64,
}

impl ProcessManager {
    fn new() -> Self {
        Self {
            initialized: false,
            records: Vec::new(),
            next_pid: 1,
        }
    }

    fn spawn_seed_process(&mut self, name: &str) {
        let pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);
        self.records.push(ProcessRecord {
            pid,
            name: name.to_string(),
            state: ProcessState::Running,
            thread_count: 1,
            exit_code: None,
        });
        event::publish(EventKind::ProcessStarted, "process", "snsh started");
    }
}

static MANAGER: StaticCell<Option<ProcessManager>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_manager_mut<R>(f: impl FnOnce(&mut ProcessManager) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *MANAGER.get() };
    if slot.is_none() {
        *slot = Some(ProcessManager::new());
    }
    let out = f(slot.as_mut().expect("process manager unavailable"));
    unlock();
    out
}

fn with_manager<R>(f: impl FnOnce(&ProcessManager) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *MANAGER.get() };
    if slot.is_none() {
        *slot = Some(ProcessManager::new());
    }
    let out = f(slot.as_ref().expect("process manager unavailable"));
    unlock();
    out
}

pub fn init() {
    with_manager_mut(|m| {
        if m.initialized {
            return;
        }
        m.spawn_seed_process("snsh");
        m.initialized = true;
    });
}

pub fn jobs() -> Vec<ProcessRecord> {
    with_manager(|m| m.records.clone())
}

pub fn kill(pid: u64) -> Result<(), &'static str> {
    with_manager_mut(|m| {
        let rec = m
            .records
            .iter_mut()
            .find(|r| r.pid == pid)
            .ok_or("kill: pid not found")?;
        rec.state = ProcessState::Exited;
        rec.exit_code = Some(137);
        event::publish(EventKind::ProcessStopped, "process", "killed");
        Ok(())
    })
}

pub fn wait(pid: u64) -> Result<i32, &'static str> {
    with_manager(|m| {
        let rec = m
            .records
            .iter()
            .find(|r| r.pid == pid)
            .ok_or("wait: pid not found")?;
        Ok(rec.exit_code.unwrap_or(0))
    })
}
