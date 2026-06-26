use crate::process;

pub fn sys_rt_sigaction(sig: u64, act: u64, oldact: u64, sz: u64) -> i64 {
    crate::ipc::signal::sys_rt_sigaction(sig as u32, act, oldact, sz)
}

pub fn sys_rt_sigprocmask(how: u64, set: u64, oldset: u64, sz: u64) -> i64 {
    crate::ipc::signal::sys_rt_sigprocmask(how as u32, set, oldset, sz)
}

pub fn sys_rt_sigreturn() -> i64 {
    let rsp = process::with_current_process(|p| p.rsp).unwrap_or(0);
    let (rip, new_rsp, rflags, oldmask) = crate::process::signal::rt_sigreturn(rsp);
    let _ = process::with_current_process_mut(|p| {
        p.rip = rip;
        p.rsp = new_rsp;
        p.rflags = rflags;
        p.signals.blocked = oldmask;
    });
    process::resume_user_from_syscall();
}
