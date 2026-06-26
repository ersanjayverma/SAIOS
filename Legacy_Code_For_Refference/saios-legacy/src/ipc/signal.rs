//! Signal handling — SAIOS process signals with Linux-numbered compatibility values.

use crate::process;
use crate::process::signal::SigAction;

const EINVAL: i64 = -22;
const ENOSYS: i64 = -38;
const SA_RESTORER: u64 = 0x0400_0000;
const SUPPORTED_HANDLER_FLAGS: u64 = SA_RESTORER;

// Compatibility signal numbers used by the current syscall ABI.
pub const SIGHUP: u32 = 1;
pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGILL: u32 = 4;
pub const SIGTRAP: u32 = 5;
pub const SIGABRT: u32 = 6;
pub const SIGBUS: u32 = 7;
pub const SIGFPE: u32 = 8;
pub const SIGKILL: u32 = 9;
pub const SIGUSR1: u32 = 10;
pub const SIGSEGV: u32 = 11;
pub const SIGUSR2: u32 = 12;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGTERM: u32 = 15;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIGSTOP: u32 = 19;
pub const SIGTSTP: u32 = 20;
pub const SIGTTIN: u32 = 21;
pub const SIGTTOU: u32 = 22;
pub const SIGWINCH: u32 = 28;

pub struct SigSet(u64); // bitmask of blocked signals

pub fn sys_rt_sigaction(signum: u32, act_ptr: u64, oldact_ptr: u64, _sigsetsize: u64) -> i64 {
    if signum == SIGKILL || signum == SIGSTOP {
        return EINVAL;
    } // EINVAL — can't override

    if oldact_ptr != 0 {
        let old = process::with_current_process(|proc| proc.signals.action(signum))
            .unwrap_or(SigAction::Default);
        let (handler_val, flags, restorer, mask): (u64, u64, u64, u64) = match old {
            SigAction::Default => (0, 0, 0, 0),
            SigAction::Ignore => (1, 0, 0, 0),
            SigAction::Handler {
                func,
                flags,
                mask,
                restorer,
            } => (func, flags, restorer, mask),
        };
        unsafe {
            core::ptr::write_volatile(oldact_ptr as *mut u64, handler_val);
            core::ptr::write_volatile((oldact_ptr + 8) as *mut u64, flags);
            core::ptr::write_volatile((oldact_ptr + 16) as *mut u64, restorer);
            core::ptr::write_volatile((oldact_ptr + 24) as *mut u64, mask);
        }
    }

    if act_ptr != 0 {
        let handler_val = unsafe { core::ptr::read_volatile(act_ptr as *const u64) };
        let flags = unsafe { core::ptr::read_volatile((act_ptr + 8) as *const u64) };
        let restorer = unsafe { core::ptr::read_volatile((act_ptr + 16) as *const u64) };
        let mask = unsafe { core::ptr::read_volatile((act_ptr + 24) as *const u64) };
        if flags & !SUPPORTED_HANDLER_FLAGS != 0 {
            return EINVAL;
        }
        let action = match handler_val {
            0 => SigAction::Default,
            1 => SigAction::Ignore,
            func => {
                if flags & SA_RESTORER == 0 || restorer == 0 {
                    return ENOSYS;
                }
                SigAction::Handler {
                    func,
                    flags,
                    mask,
                    restorer,
                }
            }
        };
        let _ = process::with_current_process_mut(|proc| proc.signals.set_action(signum, action));
    }
    0
}

pub fn sys_rt_sigprocmask(how: u32, set_ptr: u64, oldset_ptr: u64, _sigsetsize: u64) -> i64 {
    const SIG_BLOCK: u32 = 0;
    const SIG_UNBLOCK: u32 = 1;
    const SIG_SETMASK: u32 = 2;

    let old_mask = process::with_current_process(|proc| proc.signals.blocked).unwrap_or(0);
    if oldset_ptr != 0 {
        unsafe {
            core::ptr::write_volatile(oldset_ptr as *mut u64, old_mask);
        }
    }
    if set_ptr != 0 {
        let set = unsafe { core::ptr::read_volatile(set_ptr as *const u64) };
        let updated = match how {
            SIG_BLOCK => old_mask | set,
            SIG_UNBLOCK => old_mask & !set,
            SIG_SETMASK => set,
            _ => return -22,
        };
        let _ = process::with_current_process_mut(|proc| proc.signals.blocked = updated);
    }
    0
}

pub fn raise_signal(sig: u32) {
    if let Some(pid) = process::current_pid() {
        let _ = raise_signal_for_pid(pid, sig);
    }
}

pub fn raise_signal_for_pid(pid: u32, sig: u32) -> bool {
    let was_blocked = process::with_process_mut_by_pid(pid, |proc| {
        proc.signals.raise(sig);
        matches!(proc.state(), crate::process::ProcessState::Blocked)
    });

    match was_blocked {
        Some(true) => {
            let mut table = crate::process::table::TABLE.lock();
            let _ = crate::process_contract::ProcessContract::wake_pid(
                &mut table,
                pid,
                "signal wake blocked process",
            );
            true
        }
        Some(false) => true,
        None => false,
    }
}

/// Check if a signal has a non-default handler registered.
/// Returns Some(handler_addr) if handler exists, None otherwise.
pub fn has_handler(sig: u32) -> Option<u64> {
    process::with_current_process(|proc| match proc.signals.action(sig) {
        SigAction::Handler { func, .. } => Some(func),
        _ => None,
    })
    .flatten()
}

pub fn has_handler_for_pid(pid: u32, sig: u32) -> Option<(u64, u64, u64)> {
    let table = crate::process::table::TABLE.lock();
    table
        .procs
        .get(&pid)
        .and_then(|proc| match proc.signals.action(sig) {
            SigAction::Handler {
                func,
                mask,
                restorer,
                ..
            } => Some((func, mask, restorer)),
            _ => None,
        })
}

/// Process pending signals for the current process.
/// Called on return from every syscall.
pub fn process_pending() {
    loop {
        let Some((sig, action)) = process::with_current_process(|proc| {
            proc.signals
                .next_actionable()
                .map(|sig| (sig, proc.signals.action(sig)))
        })
        .flatten() else {
            return;
        };

        match action {
            SigAction::Ignore => {}
            SigAction::Default => {
                match sig {
                    SIGKILL | SIGTERM | SIGINT | SIGHUP | SIGPIPE => {
                        // Mark process as Zombie and schedule cleanup via scheduler-owned path
                        let pid = process::current_pid().unwrap_or(0);
                        crate::println!("[signal] signal {} terminated pid={}", sig, pid);
                        let _ = crate::process_contract::ProcessContract::request_exit(
                            crate::process_contract::ProcessExitRequest {
                                pid,
                                code: -(sig as i64),
                                reason: crate::process_contract::ProcessExitReason::FatalSignal,
                                tag: "signal_default_exit",
                            },
                        );
                        // Non-returning exit handoff publishes the zombie after switching away.
                        crate::process::scheduler::schedule_handoff_from("signal_default_exit");
                    }
                    SIGSEGV | SIGBUS | SIGFPE | SIGILL => {
                        let pid = process::current_pid().unwrap_or(0);
                        crate::println!("[signal] fatal signal {} — pid={} terminated", sig, pid);
                        // Mark process as Zombie and schedule cleanup
                        let _ = crate::process_contract::ProcessContract::request_exit(
                            crate::process_contract::ProcessExitRequest {
                                pid,
                                code: -(sig as i64),
                                reason: crate::process_contract::ProcessExitReason::FatalSignal,
                                tag: "signal_fault_exit",
                            },
                        );
                        // Non-returning exit handoff publishes the zombie after switching away.
                        crate::process::scheduler::schedule_handoff_from("signal_fault_exit");
                    }
                    SIGCHLD | SIGWINCH | SIGCONT => {} // ignored by default
                    _ => {}
                }
            }
            SigAction::Handler {
                func,
                mask,
                restorer,
                ..
            } => {
                let (cur_rip, cur_rsp, cur_rflags, old_mask) =
                    process::with_current_process(|proc| {
                        (proc.rip, proc.rsp, proc.rflags, proc.signals.blocked)
                    })
                    .unwrap_or((0, 0, 0x202, 0));
                let _ = process::with_current_process_mut(|proc| {
                    proc.signals.blocked = old_mask | mask | (1u64 << sig);
                });
                let (new_rip, new_rsp) = crate::process::signal::deliver(
                    sig,
                    func,
                    restorer,
                    old_mask,
                    cur_rip,
                    cur_rsp,
                    cur_rflags,
                );
                if new_rip != cur_rip {
                    let _ = process::with_current_process_mut(|p| {
                        p.rip = new_rip;
                        p.rsp = new_rsp;
                        crate::serial_println!(
                            "[signal] delivered sig {} to handler {:#x}",
                            sig,
                            func
                        );
                    });
                } else {
                    let _ = process::with_current_process_mut(|proc| {
                        proc.signals.blocked = old_mask;
                    });
                    let pid = process::current_pid().unwrap_or(0);
                    crate::println!(
                        "[signal] failed to deliver sig {} - pid={} terminating",
                        sig,
                        pid
                    );
                    let _ = crate::process_contract::ProcessContract::request_exit(
                        crate::process_contract::ProcessExitRequest {
                            pid,
                            code: -(sig as i64),
                            reason: crate::process_contract::ProcessExitReason::FatalSignal,
                            tag: "signal_handler_failed_exit",
                        },
                    );
                    crate::process::scheduler::schedule_handoff_from("signal_handler_failed_exit");
                }
            }
        }
        let _ = process::with_current_process_mut(|proc| proc.signals.pending &= !(1u64 << sig));
    }
}
