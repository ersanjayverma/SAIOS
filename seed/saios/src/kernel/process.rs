use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::kernel::crt;
use crate::kernel::event::{self, EventKind};
use crate::saifs;
use crate::shell::programs;
use crate::timer;

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
    pub image_base: u64,
    pub load_bias: u64,
    pub pie_enabled: bool,
    pub linked_interpreter: Option<String>,
    pub linked_libraries: Vec<String>,
    pub resolved_symbols: Vec<String>,
}

struct ProcessManager {
    initialized: bool,
    records: Vec<ProcessRecord>,
    next_pid: u64,
    init_pid: Option<u64>,
    shell_pid: Option<u64>,
}

impl ProcessManager {
    fn new() -> Self {
        Self {
            initialized: false,
            records: Vec::new(),
            next_pid: 1,
            init_pid: None,
            shell_pid: None,
        }
    }

    fn spawn(&mut self, name: &str) -> u64 {
        let pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);
        self.records.push(ProcessRecord {
            pid,
            name: name.to_string(),
            state: ProcessState::Running,
            thread_count: 1,
            exit_code: None,
            image_base: 0,
            load_bias: 0,
            pie_enabled: false,
            linked_interpreter: None,
            linked_libraries: Vec::new(),
            resolved_symbols: Vec::new(),
        });
        pid
    }

    fn exit(&mut self, pid: u64, code: i32) -> Result<(), &'static str> {
        let rec = self
            .records
            .iter_mut()
            .find(|r| r.pid == pid)
            .ok_or("process: pid not found")?;
        rec.state = ProcessState::Exited;
        rec.exit_code = Some(code);
        Ok(())
    }

    fn set_waiting(&mut self, pid: u64) -> Result<(), &'static str> {
        let rec = self
            .records
            .iter_mut()
            .find(|r| r.pid == pid)
            .ok_or("process: pid not found")?;
        if rec.state != ProcessState::Exited {
            rec.state = ProcessState::Waiting;
        }
        Ok(())
    }

    fn set_running(&mut self, pid: u64) -> Result<(), &'static str> {
        let rec = self
            .records
            .iter_mut()
            .find(|r| r.pid == pid)
            .ok_or("process: pid not found")?;
        if rec.state != ProcessState::Exited {
            rec.state = ProcessState::Running;
        }
        Ok(())
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
        m.initialized = true;
    });
}

pub fn start_pid1(path: &str) -> u64 {
    with_manager_mut(|m| {
        if let Some(pid) = m.init_pid {
            return pid;
        }

        let pid = m.next_pid;
        m.next_pid = m.next_pid.saturating_add(1);
        m.records.push(ProcessRecord {
            pid,
            name: path.to_string(),
            state: ProcessState::Running,
            thread_count: 1,
            exit_code: None,
            image_base: 0,
            load_bias: 0,
            pie_enabled: false,
            linked_interpreter: None,
            linked_libraries: Vec::new(),
            resolved_symbols: Vec::new(),
        });
        m.init_pid = Some(pid);
        event::publish(
            EventKind::ProcessStarted,
            "process",
            alloc::format!("pid={} {} started", pid, path).as_str(),
        );
        pid
    })
}

pub fn finish_pid1(code: i32) -> Result<(), &'static str> {
    with_manager_mut(|m| {
        let pid = m.init_pid.ok_or("pid1: not started")?;
        m.exit(pid, code)?;
        event::publish(
            EventKind::ProcessStopped,
            "process",
            alloc::format!("pid={} exit={}", pid, code).as_str(),
        );
        Ok(())
    })
}

pub fn ensure_shell_process(name: &str) -> u64 {
    with_manager_mut(|m| {
        if let Some(pid) = m.shell_pid {
            return pid;
        }

        let pid = m.spawn(name);
        m.shell_pid = Some(pid);
        event::publish(
            EventKind::ProcessStarted,
            "process",
            alloc::format!("pid={} {} started", pid, name).as_str(),
        );
        pid
    })
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
    with_manager_mut(|m| {
        let _ = m.set_waiting(pid);
        let rec = m
            .records
            .iter()
            .find(|r| r.pid == pid)
            .ok_or("wait: pid not found")?;
        if rec.state != ProcessState::Exited {
            return Err("wait: process still running");
        }
        Ok(rec.exit_code.unwrap_or(0))
    })
}

pub fn exit(pid: u64, code: i32) -> Result<(), &'static str> {
    with_manager_mut(|m| {
        m.exit(pid, code)?;
        event::publish(
            EventKind::ProcessStopped,
            "process",
            alloc::format!("pid={} exit={}", pid, code).as_str(),
        );
        Ok(())
    })
}

pub fn spawn(name: &str, args: &[&str], env: &[(String, String)]) -> Result<u64, &'static str> {
    let resolved = resolve_program_name(name)?;
    let program_name = resolved.rsplit('/').next().unwrap_or(resolved.as_str());
    let startup = crt::prepare_startup_block(program_name, args, env);
    let metadata = programs::binary_metadata(resolved.as_str());

    let pid = with_manager_mut(|m| m.spawn(resolved.as_str()));
    let (image_base, load_bias, pie_enabled) = if let Some(meta) = metadata.as_ref() {
        if meta.pie {
            let (base, bias) = compute_pie_layout(pid, meta.preferred_base);
            (base, bias, true)
        } else {
            (meta.preferred_base, 0, false)
        }
    } else {
        (0, 0, false)
    };

    with_manager_mut(|m| {
        if let Some(rec) = m.records.iter_mut().find(|r| r.pid == pid) {
            rec.image_base = image_base;
            rec.load_bias = load_bias;
            rec.pie_enabled = pie_enabled;
        }
    });

    let link_report = if let Some(meta) = metadata.as_ref() {
        match crate::kernel::dynamic_linker::link_image(resolved.as_str(), meta) {
            Ok(report) => report,
            Err(e) => {
                with_manager_mut(|m| {
                    if let Some(rec) = m.records.iter_mut().find(|r| r.pid == pid) {
                        rec.state = ProcessState::Exited;
                        rec.exit_code = Some(127);
                    }
                });
                event::publish(
                    EventKind::ProcessStopped,
                    "process",
                    alloc::format!("pid={} dynamic-link-failed {}", pid, e).as_str(),
                );
                return Err(e);
            }
        }
    } else {
        crate::kernel::dynamic_linker::LinkReport {
            interpreter: "-".to_string(),
            libraries: Vec::new(),
            resolved_symbols: Vec::new(),
        }
    };

    with_manager_mut(|m| {
        if let Some(rec) = m.records.iter_mut().find(|r| r.pid == pid) {
            if link_report.interpreter != "-" {
                rec.linked_interpreter = Some(link_report.interpreter.clone());
            }
            rec.linked_libraries = link_report.libraries.clone();
            rec.resolved_symbols = link_report.resolved_symbols.clone();
        }
    });

    event::publish(
        EventKind::ProcessStarted,
        "process",
        alloc::format!(
            "pid={} {} argc={} envc={} pie={} base=0x{:x} bias=0x{:x} libs={}",
            pid,
            resolved,
            startup.argc,
            startup.envp.len(),
            pie_enabled,
            image_base,
            load_bias,
            link_report.libraries.len()
        )
        .as_str(),
    );

    let run = programs::execute_path(resolved.as_str(), program_name, args, env);
    let exit_code = run.unwrap_or(127);

    with_manager_mut(|m| {
        let _ = m.set_running(pid);
        let _ = m.exit(pid, exit_code);
    });
    event::publish(
        EventKind::ProcessStopped,
        "process",
        alloc::format!("pid={} exit={}", pid, exit_code).as_str(),
    );

    Ok(pid)
}

fn candidate_program_paths(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if name.contains('/') {
        out.push(name.to_string());
        return out;
    }

    out.push(name.to_string());
    out.push(alloc::format!("/bin/{}", name));
    out
}

fn resolve_program_name(name: &str) -> Result<String, &'static str> {
    let candidates = candidate_program_paths(name);
    for candidate in candidates {
        if saifs::open(candidate.as_str()).is_ok() {
            return Ok(candidate);
        }
    }
    Err("exec: program not found")
}

fn compute_pie_layout(pid: u64, preferred_base: u64) -> (u64, u64) {
    let aslr_window = 0x0100_0000u64;
    let granularity = 0x1000u64;
    let entropy = timer::ticks() ^ pid.wrapping_mul(0x9E37_79B9_7F4A_7C15u64);
    let offset = (entropy % aslr_window) & !(granularity - 1);
    let base = preferred_base.saturating_add(offset);
    (base, offset)
}

pub fn exec(name: &str, args: &[&str], env: &[(String, String)]) -> Result<i32, &'static str> {
    let pid = spawn(name, args, env)?;
    jobs()
        .into_iter()
        .find(|job| job.pid == pid)
        .and_then(|job| job.exit_code)
        .ok_or("exec: process did not exit")
}
