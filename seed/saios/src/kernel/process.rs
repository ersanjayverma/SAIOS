use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;

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
    pub parent_pid: Option<u64>,
    pub name: String,
    pub state: ProcessState,
    pub process_group: u64,
    pub session_id: u64,
    pub controlling_tty: bool,
    pub thread_count: usize,
    pub exit_code: Option<i32>,
    pub image_base: u64,
    pub load_bias: u64,
    pub pie_enabled: bool,
    pub linked_interpreter: Option<String>,
    pub linked_libraries: Vec<String>,
    pub resolved_symbols: Vec<String>,
    pub readable_segments: usize,
    pub writable_segments: usize,
    pub executable_segments: usize,
}

struct ProcessManager {
    initialized: bool,
    records: Vec<ProcessRecord>,
    next_pid: u64,
    init_pid: Option<u64>,
    shell_pid: Option<u64>,
    foreground_pgid: Option<u64>,
    controlling_tty_session: Option<u64>,
}

impl ProcessManager {
    fn new() -> Self {
        Self {
            initialized: false,
            records: Vec::new(),
            next_pid: 1,
            init_pid: None,
            shell_pid: None,
            foreground_pgid: None,
            controlling_tty_session: None,
        }
    }

    fn spawn(&mut self, name: &str, parent_pid: Option<u64>) -> u64 {
        let pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);
        let (process_group, session_id, controlling_tty) = if let Some(ppid) = parent_pid {
            if let Some(parent) = self.records.iter().find(|r| r.pid == ppid) {
                (
                    parent.process_group,
                    parent.session_id,
                    parent.controlling_tty,
                )
            } else {
                (pid, pid, false)
            }
        } else {
            (pid, pid, false)
        };
        self.records.push(ProcessRecord {
            pid,
            parent_pid,
            name: name.to_string(),
            state: ProcessState::Running,
            process_group,
            session_id,
            controlling_tty,
            thread_count: 1,
            exit_code: None,
            image_base: 0,
            load_bias: 0,
            pie_enabled: false,
            linked_interpreter: None,
            linked_libraries: Vec::new(),
            resolved_symbols: Vec::new(),
            readable_segments: 0,
            writable_segments: 0,
            executable_segments: 0,
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

        // Re-parent living descendants so they can still be waited on.
        let reaper = self.reaper_pid();
        for child in self.records.iter_mut() {
            if child.parent_pid == Some(pid) {
                child.parent_pid = reaper;
            }
        }
        self.sweep_orphan_zombies();
        Ok(())
    }

    fn reaper_pid(&self) -> Option<u64> {
        if let Some(pid) = self.init_pid {
            return Some(pid);
        }
        self.shell_pid
    }

    fn sweep_orphan_zombies(&mut self) {
        let init_pid = self.init_pid;
        let shell_pid = self.shell_pid;
        let live_pids: Vec<u64> = self.records.iter().map(|p| p.pid).collect();
        self.records.retain(|r| {
            if r.state != ProcessState::Exited {
                return true;
            }
            if Some(r.pid) == init_pid || Some(r.pid) == shell_pid {
                return true;
            }
            if r.parent_pid.is_none() {
                return false;
            }
            let parent = r.parent_pid.unwrap_or(0);
            live_pids.iter().any(|p| *p == parent)
        });
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
    hal::arch::x86_64::sync::spinlock_acquire(&LOCK);
}

fn unlock() {
    hal::arch::x86_64::sync::spinlock_release(&LOCK);
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
            parent_pid: None,
            name: path.to_string(),
            state: ProcessState::Running,
            process_group: pid,
            session_id: pid,
            controlling_tty: false,
            thread_count: 1,
            exit_code: None,
            image_base: 0,
            load_bias: 0,
            pie_enabled: false,
            linked_interpreter: None,
            linked_libraries: Vec::new(),
            resolved_symbols: Vec::new(),
            readable_segments: 0,
            writable_segments: 0,
            executable_segments: 0,
        });
        m.init_pid = Some(pid);
        if m.controlling_tty_session.is_none() {
            m.controlling_tty_session = Some(pid);
        }
        if m.foreground_pgid.is_none() {
            m.foreground_pgid = Some(pid);
        }
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

        let pid = m.spawn(name, m.init_pid);
        m.shell_pid = Some(pid);
        if let Some(rec) = m.records.iter_mut().find(|r| r.pid == pid) {
            rec.session_id = pid;
            rec.process_group = pid;
            rec.controlling_tty = true;
        }
        m.controlling_tty_session = Some(pid);
        m.foreground_pgid = Some(pid);
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

pub fn has_waitable_child(parent_pid: u64, target: Option<u64>) -> bool {
    with_manager(|m| {
        m.records.iter().any(|r| {
            if r.parent_pid != Some(parent_pid) {
                return false;
            }
            if let Some(tpid) = target {
                r.pid == tpid
            } else {
                true
            }
        })
    })
}

pub fn first_exited_child(parent_pid: u64) -> Option<(u64, i32)> {
    with_manager(|m| {
        m.records
            .iter()
            .find(|r| r.parent_pid == Some(parent_pid) && r.state == ProcessState::Exited)
            .map(|r| (r.pid, r.exit_code.unwrap_or(0)))
    })
}

pub fn child_record(parent_pid: u64, child_pid: u64) -> Option<ProcessRecord> {
    with_manager(|m| {
        m.records
            .iter()
            .find(|r| r.parent_pid == Some(parent_pid) && r.pid == child_pid)
            .cloned()
    })
}

pub fn reap_child(parent_pid: u64, pid: u64) -> Result<i32, &'static str> {
    with_manager_mut(|m| {
        let idx = m
            .records
            .iter()
            .position(|r| r.pid == pid)
            .ok_or("reap: pid not found")?;

        if m.records[idx].parent_pid != Some(parent_pid) {
            return Err("reap: not a child of caller");
        }
        if m.records[idx].state != ProcessState::Exited {
            return Err("reap: process still running");
        }
        if m.init_pid == Some(pid) {
            return Err("reap: cannot reap pid1");
        }
        if m.shell_pid == Some(pid) {
            return Err("reap: cannot reap shell process");
        }

        let code = m.records[idx].exit_code.unwrap_or(0);
        m.records.remove(idx);
        Ok(code)
    })
}

pub fn wait(pid: u64) -> Result<i32, &'static str> {
    with_manager_mut(|m| {
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

pub fn record(pid: u64) -> Option<ProcessRecord> {
    with_manager(|m| m.records.iter().find(|r| r.pid == pid).cloned())
}

pub fn first_exited(exclude_pid: u64) -> Option<(u64, i32)> {
    with_manager(|m| {
        m.records
            .iter()
            .find(|r| r.pid != exclude_pid && r.state == ProcessState::Exited)
            .map(|r| (r.pid, r.exit_code.unwrap_or(0)))
    })
}

pub fn reap(pid: u64) -> Result<i32, &'static str> {
    with_manager_mut(|m| {
        let idx = m
            .records
            .iter()
            .position(|r| r.pid == pid)
            .ok_or("reap: pid not found")?;
        if m.records[idx].state != ProcessState::Exited {
            return Err("reap: process still running");
        }

        if m.init_pid == Some(pid) {
            return Err("reap: cannot reap pid1");
        }

        if m.shell_pid == Some(pid) {
            m.shell_pid = None;
        }

        let code = m.records[idx].exit_code.unwrap_or(0);
        m.records.remove(idx);
        Ok(code)
    })
}

pub fn fork_from(parent_pid: u64, clone_flags: u64) -> Result<u64, &'static str> {
    with_manager_mut(|m| {
        let parent = m
            .records
            .iter()
            .find(|r| r.pid == parent_pid)
            .cloned()
            .ok_or("fork: parent pid not found")?;

        let pid = m.next_pid;
        m.next_pid = m.next_pid.saturating_add(1);

        let mut child = parent;
        child.pid = pid;
        child.parent_pid = Some(parent_pid);
        child.state = ProcessState::Running;
        child.exit_code = None;
        // CLONE_THREAD only affects accounting in this model.
        if (clone_flags & (1 << 16)) != 0 {
            child.thread_count = child.thread_count.saturating_add(1);
        } else {
            child.thread_count = 1;
        }

        m.records.push(child);
        event::publish(
            EventKind::ProcessStarted,
            "process",
            alloc::format!(
                "pid={} forked-from={} flags=0x{:x}",
                pid,
                parent_pid,
                clone_flags
            )
            .as_str(),
        );
        Ok(pid)
    })
}

pub fn send_signal(pid: u64, signo: u8) -> Result<(), &'static str> {
    if signo == 0 {
        return Err("signal: invalid signal");
    }
    with_manager_mut(|m| {
        let rec = m
            .records
            .iter_mut()
            .find(|r| r.pid == pid)
            .ok_or("signal: pid not found")?;
        if rec.state == ProcessState::Exited {
            return Ok(());
        }
        rec.state = ProcessState::Exited;
        rec.exit_code = Some(-i32::from(signo));
        event::publish(
            EventKind::ProcessStopped,
            "process",
            alloc::format!("pid={} signaled={}", pid, signo).as_str(),
        );
        Ok(())
    })
}

pub fn process_group(pid: u64) -> Option<u64> {
    with_manager(|m| {
        m.records
            .iter()
            .find(|r| r.pid == pid)
            .map(|r| r.process_group)
    })
}

pub fn session_id(pid: u64) -> Option<u64> {
    with_manager(|m| {
        m.records
            .iter()
            .find(|r| r.pid == pid)
            .map(|r| r.session_id)
    })
}

pub fn set_process_group(pid: u64, pgid: u64) -> Result<(), &'static str> {
    with_manager_mut(|m| {
        if pgid == 0 {
            return Err("setpgid: invalid pgid");
        }
        let sid = m
            .records
            .iter()
            .find(|r| r.pid == pid)
            .map(|r| r.session_id)
            .ok_or("setpgid: pid not found")?;
        let group_ok = m
            .records
            .iter()
            .find(|r| r.pid == pgid)
            .map(|r| r.session_id == sid)
            .unwrap_or(true);
        if !group_ok {
            return Err("setpgid: cross-session group move denied");
        }

        let rec = m
            .records
            .iter_mut()
            .find(|r| r.pid == pid)
            .ok_or("setpgid: pid not found")?;
        rec.process_group = pgid;
        Ok(())
    })
}

pub fn create_session(pid: u64) -> Result<u64, &'static str> {
    with_manager_mut(|m| {
        let rec = m
            .records
            .iter_mut()
            .find(|r| r.pid == pid)
            .ok_or("setsid: pid not found")?;
        rec.session_id = pid;
        rec.process_group = pid;
        rec.controlling_tty = true;
        m.controlling_tty_session = Some(pid);
        m.foreground_pgid = Some(pid);
        Ok(pid)
    })
}

pub fn foreground_process_group() -> Option<u64> {
    with_manager(|m| m.foreground_pgid)
}

pub fn set_foreground_process_group(pgid: u64) -> Result<(), &'static str> {
    with_manager_mut(|m| {
        let rec = m
            .records
            .iter()
            .find(|r| r.process_group == pgid)
            .ok_or("tcsetpgrp: process group not found")?;
        if let Some(tty_sid) = m.controlling_tty_session {
            if rec.session_id != tty_sid {
                return Err("tcsetpgrp: process group not in controlling tty session");
            }
        }
        m.foreground_pgid = Some(pgid);
        Ok(())
    })
}

pub fn signal_process_group(pgid: u64, signo: u8) -> Result<usize, &'static str> {
    with_manager_mut(|m| {
        if signo == 0 {
            return Err("signal: invalid signal");
        }
        let shell_pid = m.shell_pid;
        let mut delivered = 0usize;
        for rec in m.records.iter_mut() {
            if rec.process_group != pgid || rec.state == ProcessState::Exited {
                continue;
            }
            // Keep interactive shell alive for terminal signals.
            if shell_pid == Some(rec.pid) && (signo == 2 || signo == 3) {
                continue;
            }
            rec.state = ProcessState::Exited;
            rec.exit_code = Some(-i32::from(signo));
            delivered = delivered.saturating_add(1);
        }
        if delivered == 0 {
            return Err("signal: process group not found or no live members");
        }
        Ok(delivered)
    })
}

pub fn signal_foreground_group(signo: u8) -> usize {
    let Some(pgid) = foreground_process_group() else {
        return 0;
    };
    signal_process_group(pgid, signo).unwrap_or(0)
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

pub fn exit_quiet(pid: u64, code: i32) -> Result<(), &'static str> {
    with_manager_mut(|m| m.exit(pid, code))
}

pub fn spawn(name: &str, args: &[&str], env: &[(String, String)]) -> Result<u64, &'static str> {
    let parent_pid = with_manager(|m| m.shell_pid.or(m.init_pid));
    spawn_from(parent_pid, name, args, env)
}

pub fn spawn_from(
    parent_pid: Option<u64>,
    name: &str,
    args: &[&str],
    env: &[(String, String)],
) -> Result<u64, &'static str> {
    let resolved = resolve_program_name(name)?;
    let presented_argv0 = name;
    let program_name = resolved.rsplit('/').next().unwrap_or(resolved.as_str());
    let startup = crt::prepare_startup_block(program_name, args, env);
    let metadata = programs::binary_metadata_checked(resolved.as_str())?;

    let pid = with_manager_mut(|m| m.spawn(resolved.as_str(), parent_pid));
    let exit_code = execute_in_pid(
        pid,
        resolved.as_str(),
        presented_argv0,
        program_name,
        args,
        env,
        &startup,
        &metadata,
    )?;
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

pub fn exec_in_place(
    pid: u64,
    name: &str,
    args: &[&str],
    env: &[(String, String)],
) -> Result<i32, &'static str> {
    let resolved = resolve_program_name(name)?;
    let presented_argv0 = name;
    let program_name = resolved.rsplit('/').next().unwrap_or(resolved.as_str());
    let startup = crt::prepare_startup_block(program_name, args, env);
    let metadata = programs::binary_metadata_checked(resolved.as_str())?;

    execute_in_pid(
        pid,
        resolved.as_str(),
        presented_argv0,
        program_name,
        args,
        env,
        &startup,
        &metadata,
    )
}

pub fn exec_path_in_place(
    pid: u64,
    path: &str,
    presented_argv0: &str,
    args: &[&str],
    env: &[(String, String)],
) -> Result<i32, &'static str> {
    let program_name = path.rsplit('/').next().unwrap_or(path);
    let startup = crt::prepare_startup_block(program_name, args, env);
    let metadata = programs::binary_metadata_checked(path)?;

    execute_in_pid(
        pid,
        path,
        presented_argv0,
        program_name,
        args,
        env,
        &startup,
        &metadata,
    )
}

fn execute_in_pid(
    pid: u64,
    resolved: &str,
    presented_argv0: &str,
    program_name: &str,
    args: &[&str],
    env: &[(String, String)],
    startup: &crt::CrtStartupBlock,
    metadata: &programs::BinaryMetadata,
) -> Result<i32, &'static str> {
    let (image_base, load_bias, pie_enabled) = if metadata.pie {
        let (base, bias) = compute_pie_layout(pid, metadata.preferred_base);
        (base, bias, true)
    } else {
        (metadata.preferred_base, 0, false)
    };

    with_manager_mut(|m| {
        if let Some(rec) = m.records.iter_mut().find(|r| r.pid == pid) {
            rec.name = resolved.to_string();
            rec.state = ProcessState::Running;
            rec.exit_code = None;
            rec.image_base = image_base;
            rec.load_bias = load_bias;
            rec.pie_enabled = pie_enabled;
            rec.linked_interpreter = None;
            rec.linked_libraries.clear();
            rec.resolved_symbols.clear();
            rec.readable_segments = metadata.readable_segments;
            rec.writable_segments = metadata.writable_segments;
            rec.executable_segments = metadata.executable_segments;
        } else {
            return;
        }
    });

    if record(pid).is_none() {
        return Err("exec: pid not found");
    }

    let link_report = match crate::kernel::dynamic_linker::link_image(resolved, &metadata)
    {
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
            "pid={} {} argc={} envc={} pie={} base=0x{:x} bias=0x{:x} libs={} segs={} bss={} rwx={}/{}/{}",
            pid,
            resolved,
            startup.argc,
            startup.envp.len(),
            pie_enabled,
            image_base,
            load_bias,
            link_report.libraries.len(),
            metadata.load_segments,
            metadata.zero_fill_bytes,
            metadata.readable_segments,
            metadata.writable_segments,
            metadata.executable_segments
        )
        .as_str(),
    );

    let run = if metadata.load_segments > 0 {
        crate::kernel::elf_loader::load_and_run(
            resolved,
            presented_argv0,
            image_base,
            pid,
            args,
        )
    } else {
        programs::execute_path(resolved, program_name, args, env)
    };

    let mut run_error: Option<&'static str> = None;
    let exit_code = match run {
        Ok(code) => code,
        Err(e) => {
            run_error = Some(e);
            127
        }
    };

    with_manager_mut(|m| {
        let _ = m.set_running(pid);
        let _ = m.exit(pid, exit_code);
    });
    event::publish(
        EventKind::ProcessStopped,
        "process",
        alloc::format!("pid={} exit={}", pid, exit_code).as_str(),
    );

    if let Some(e) = run_error {
        return Err(e);
    }

    Ok(exit_code)
}

fn candidate_program_paths(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if name.contains('/') {
        out.push(saifs::absolute_path(name));
        return out;
    }

    // 1. Try name literally (absolute path or in cwd).
    out.push(name.to_string());

    // 2. Search system PATH directories.
    let sys_paths = [
        "/bin", "/sbin", "/usr/bin", "/usr/sbin",
        "/usr/local/bin", "/usr/local/sbin",
    ];
    for dir in &sys_paths {
        out.push(alloc::format!("{}/{}", dir, name));
    }

    // 3. Search the same directories under every non-tmpfs mounted volume.
    //    This lets `exec bash` find `/dsk/bin/bash` without needing an
    //    explicit `/dsk/bin/bash` path.
    let mounts = crate::driver::storage::volumes_cached();
    for vol in &mounts {
        if let Some(ref mp) = vol.mounted_at {
            let mp = mp.trim_end_matches('/');
            if mp.is_empty() || mp == "/" {
                continue; // tmpfs root – sys_paths already cover this
            }
            for dir in &sys_paths {
                out.push(alloc::format!("{}{}/{}", mp, dir, name));
            }
        }
    }

    out
}

/// Coreutils-style names that `vfs::seed_standard_tree` seeded as empty
/// placeholder files under `/bin` long before real ELF execution (and
/// `busybox`) existed. They're 0-byte stubs, not real binaries -- a PATH
/// search finds them before it would ever reach a real implementation, so
/// resolving one of these names always finds a broken placeholder instead
/// of ever running the working busybox applet of the same name. Redirect
/// them to busybox instead, the same way a real Linux system would use a
/// `/bin/ls -> busybox` symlink: only the *resolved path* changes, argv is
/// left untouched, so busybox's own argv[0]-based applet dispatch picks the
/// right applet automatically.
const BUSYBOX_REDIRECT_APPLETS: &[&str] = &[
    "ash", "sh", "ls", "cat", "cp", "mv", "rm", "mkdir", "true", "false", "ps", "kill", "top", "uname",
];

fn resolve_busybox_path() -> Option<String> {
    for candidate in ["/bin/busybox", "/usr/bin/busybox"] {
        if saifs::open(candidate).is_ok() {
            return Some(candidate.to_string());
        }
    }
    None
}

fn resolve_program_name(name: &str) -> Result<String, &'static str> {
    let base_name = name.rsplit('/').next().unwrap_or(name);
    if BUSYBOX_REDIRECT_APPLETS.contains(&base_name) {
        if let Some(busybox_path) = resolve_busybox_path() {
            return Ok(busybox_path);
        }
    }

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
    let parent_pid = with_manager(|m| m.shell_pid.or(m.init_pid));
    exec_from(parent_pid, name, args, env)
}

pub fn exec_from(
    parent_pid: Option<u64>,
    name: &str,
    args: &[&str],
    env: &[(String, String)],
) -> Result<i32, &'static str> {
    let pid = spawn_from(parent_pid, name, args, env)?;
    jobs()
        .into_iter()
        .find(|job| job.pid == pid)
        .and_then(|job| job.exit_code)
        .ok_or("exec: process did not exit")
}
