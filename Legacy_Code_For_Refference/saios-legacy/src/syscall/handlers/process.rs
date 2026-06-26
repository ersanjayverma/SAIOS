use super::{EFAULT, EINTR, EINVAL, ENOEXEC, ENOSYS, read_user_str, vfs_err, write_user};
use crate::process;
use crate::vfs;
use crate::vfs_contract::VfsContract;

fn current_proc<R>(f: impl FnOnce(&crate::process::Process) -> R) -> Option<R> {
    let table = crate::process::table::TABLE.lock();
    table.current_ref().map(f)
}

fn current_pid() -> u32 {
    crate::process::table::TABLE.lock().current_pid()
}

pub fn sys_getpid() -> i64 {
    current_pid().max(1) as i64
}

pub fn sys_getppid() -> i64 {
    current_proc(|proc| proc.parent_pid as i64).unwrap_or(1)
}

pub fn sys_getuid() -> i64 {
    current_proc(|proc| proc.uid as i64).unwrap_or(0)
}

pub fn sys_geteuid() -> i64 {
    current_proc(|proc| proc.euid as i64).unwrap_or(0)
}

pub fn sys_getgid() -> i64 {
    current_proc(|proc| proc.gid as i64).unwrap_or(0)
}

pub fn sys_getegid() -> i64 {
    current_proc(|proc| proc.egid as i64).unwrap_or(0)
}

pub fn sys_gettid() -> i64 {
    sys_getpid()
}

pub fn sys_getpgrp() -> i64 {
    current_proc(|proc| proc.pgid as i64).unwrap_or(1)
}

pub fn sys_getpgid(pid: u64) -> i64 {
    let pid = pid as u32;
    let table = crate::process::table::TABLE.lock();
    if pid == 0 {
        table.current_ref().map(|p| p.pgid as i64).unwrap_or(1)
    } else {
        table.procs.get(&pid).map(|p| p.pgid as i64).unwrap_or(-3)
    }
}

pub fn sys_setpgid(pid: u64, pgid: u64) -> i64 {
    let current_pid = current_pid();
    let target_pid = if pid == 0 { current_pid } else { pid as u32 };
    let target_pgid = if pgid == 0 { target_pid } else { pgid as u32 };
    crate::process_contract::ProcessContract::set_process_group(
        current_pid,
        target_pid,
        target_pgid,
    )
    .map(|()| 0)
    .unwrap_or_else(|err| err)
}

pub fn sys_setsid() -> i64 {
    let current_pid = current_pid();
    crate::process_contract::ProcessContract::create_session(current_pid)
        .map(|sid| sid as i64)
        .unwrap_or_else(|err| err)
}

pub fn sys_getsid(pid: u64) -> i64 {
    let pid = pid as u32;
    let table = crate::process::table::TABLE.lock();
    if pid == 0 {
        table
            .current_ref()
            .map(|p| p.session_id as i64)
            .unwrap_or(1)
    } else {
        table
            .procs
            .get(&pid)
            .map(|p| p.session_id as i64)
            .unwrap_or(-3)
    }
}

pub fn sys_setuid(uid: u64) -> i64 {
    if crate::security_contract::SecurityContract::require_capability(
        crate::security_contract::SecurityCapability::SysAdmin,
        "sys_setuid",
    )
    .is_err()
    {
        return -1;
    }
    let pid = current_pid();
    let old_uid = crate::process::with_current_process(|p| p.uid).unwrap_or(u32::MAX);
    let result = crate::process_contract::ProcessContract::set_uid(pid, uid as u32)
        .map(|()| 0i64)
        .unwrap_or_else(|err| err);
    if result == 0 {
        // Constitutional: credential change audit event (SSOT §Security Monitoring)
        crate::kds::kds_event(
            crate::kds::KdsSubsystem::Security,
            crate::kds::KdsEventType::SecurityPrivilegeEscalation,
            if (uid as u32) < old_uid {
                crate::kds::KdsSeverity::Warn
            } else {
                crate::kds::KdsSeverity::Info
            },
            [pid as u64, old_uid as u64, uid, 105], // syscall nr 105
        );
    }
    result
}

pub fn sys_setgid(gid: u64) -> i64 {
    if crate::security_contract::SecurityContract::require_capability(
        crate::security_contract::SecurityCapability::SysAdmin,
        "sys_setgid",
    )
    .is_err()
    {
        return -1;
    }
    let pid = current_pid();
    let old_gid = crate::process::with_current_process(|p| p.gid).unwrap_or(u32::MAX);
    let result = crate::process_contract::ProcessContract::set_gid(pid, gid as u32)
        .map(|()| 0i64)
        .unwrap_or_else(|err| err);
    if result == 0 {
        crate::kds::kds_event(
            crate::kds::KdsSubsystem::Security,
            crate::kds::KdsEventType::SecurityPrivilegeEscalation,
            if (gid as u32) < old_gid {
                crate::kds::KdsSeverity::Warn
            } else {
                crate::kds::KdsSeverity::Info
            },
            [pid as u64, old_gid as u64, gid, 106], // syscall nr 106
        );
    }
    result
}

pub fn sys_setreuid(r_uid: u64, e_uid: u64) -> i64 {
    let pid = current_pid();
    crate::process_contract::ProcessContract::set_reuid(pid, r_uid, e_uid)
        .map(|()| 0)
        .unwrap_or_else(|err| err)
}

pub fn sys_setregid(r_gid: u64, e_gid: u64) -> i64 {
    let pid = current_pid();
    crate::process_contract::ProcessContract::set_regid(pid, r_gid, e_gid)
        .map(|()| 0)
        .unwrap_or_else(|err| err)
}

pub fn sys_setresuid(r_uid: u64, e_uid: u64, s_uid: u64) -> i64 {
    let pid = current_pid();
    crate::process_contract::ProcessContract::set_resuid(pid, r_uid, e_uid, s_uid)
        .map(|()| 0)
        .unwrap_or_else(|err| err)
}

pub fn sys_setresgid(r_gid: u64, e_gid: u64, s_gid: u64) -> i64 {
    let pid = current_pid();
    crate::process_contract::ProcessContract::set_resgid(pid, r_gid, e_gid, s_gid)
        .map(|()| 0)
        .unwrap_or_else(|err| err)
}

pub fn sys_getresuid(r: u64, e: u64, s: u64) -> i64 {
    let Some((uid, euid, suid)) = current_proc(|proc| (proc.uid, proc.euid, proc.suid)) else {
        return -3;
    };
    unsafe {
        if !write_user(r, uid) || !write_user(e, euid) || !write_user(s, suid) {
            return EFAULT;
        }
    }
    0
}

pub fn sys_getresgid(r: u64, e: u64, s: u64) -> i64 {
    let Some((gid, egid, sgid)) = current_proc(|proc| (proc.gid, proc.egid, proc.sgid)) else {
        return -3;
    };
    unsafe {
        if !write_user(r, gid) || !write_user(e, egid) || !write_user(s, sgid) {
            return EFAULT;
        }
    }
    0
}

pub fn sys_getgroups(size: u64, list: u64) -> i64 {
    if size != 0 && list == 0 {
        return EFAULT;
    }
    0
}

pub fn sys_setgroups(_size: u64, _list: u64) -> i64 {
    ENOSYS
}

pub fn sys_fork() -> i64 {
    let (rip, rsp, rflags) = crate::arch::syscall::saved_user_syscall_site();
    if rip == 0 || rsp == 0 {
        return EINVAL;
    }
    let (rdi, rsi, rdx, r8, r9, r10) = crate::arch::syscall::saved_user_caller_regs();
    let (rbx, rbp, r12, r13, r14, r15) = crate::arch::syscall::saved_user_callee_regs();
    crate::process::fork::do_fork(
        rip, rsp, rflags, rdi, rsi, rdx, r8, r9, r10, rbx, rbp, r12, r13, r14, r15,
    )
}

pub fn sys_vfork() -> i64 {
    sys_fork()
}

pub fn sys_execve(path_ptr: u64, argv_ptr: u64, envp_ptr: u64) -> i64 {
    let (pid, current_name) = process::with_current_process(|proc| (proc.pid, proc.name.clone()))
        .unwrap_or((0, alloc::string::String::from("<none>")));
    if crate::process::table::trace_pid(pid) {
        crate::println!(
            "[execve] enter pid={} path={:#x} argv={:#x} envp={:#x}",
            pid,
            path_ptr,
            argv_ptr,
            envp_ptr
        );
    }
    crate::serial_println!(
        "[execve] enter pid={} current_name='{}' user_path_ptr={:#x}",
        pid,
        current_name,
        path_ptr
    );
    let path = unsafe {
        match read_user_str(path_ptr, 4096) {
            Some(p) => p,
            None => {
                crate::serial_println!("[execve] read_user_str failed ptr={:#x}", path_ptr);
                crate::println!("sys_execve failure pid={}", pid);
                return EFAULT;
            }
        }
    };
    crate::serial_println!("[execve] copied_path='{}'", path);

    let argv = crate::process::exec::read_user_argv(argv_ptr);
    let envp = crate::process::exec::read_user_argv(envp_ptr);
    crate::serial_println!(
        "[execve] argv_ptr={:#x} envp_ptr={:#x} argc={} envc={}",
        argv_ptr,
        envp_ptr,
        argv.len(),
        envp.len()
    );
    for (idx, arg) in argv.iter().take(4).enumerate() {
        crate::serial_println!("[execve] argv[{}]='{}'", idx, arg);
    }
    if let Some(first_env) = envp.first() {
        crate::serial_println!("[execve] envp[0]='{}'", first_env);
    }

    let execvetest_embedded = "/tmp/execve_child";
    match vfs::resolve(execvetest_embedded) {
        Ok(inode) => {
            let size = inode.ops.stat().map(|s| s.st_size).unwrap_or(-1);
            crate::serial_println!(
                "[execve] embedded_lookup path='{}' ok type={:?} size={}",
                execvetest_embedded,
                inode.ftype,
                size
            );
        }
        Err(e) => {
            crate::serial_println!(
                "[execve] embedded_lookup path='{}' err_errno={}",
                execvetest_embedded,
                vfs_err(e)
            );
        }
    }

    let image = match VfsContract::exec_image(&path) {
        Ok(image) => {
            crate::serial_println!(
                "[execve] vfs_exec ok path='{}' type={:?} size={} bytes={}",
                path,
                image.file_type,
                image.size,
                image.data.len()
            );
            image
        }
        Err(e) => {
            let errno = vfs_err(e);
            crate::serial_println!("[execve] vfs_exec err path='{}' errno={}", path, errno);
            crate::println!("sys_execve failure pid={}", pid);
            return errno;
        }
    };

    match process::with_current_process_mut(|proc| {
        crate::serial_println!(
            "[execve] replacing image pid={} current_name='{}' elf_load_entry_pending",
            proc.pid,
            proc.name
        );
        crate::process::exec::do_exec(proc, &image.data, &argv, &envp).map(|(entry, rsp)| {
            crate::execution_contract::UserContext {
                rip: entry,
                rsp,
                rflags: proc.rflags,
                fs_base: proc.fs_base.fs_base,
                gs_base: proc.fs_base.gs_base,
            }
        })
    }) {
        Some(Ok(context)) => {
            crate::println!("sys_execve success pid={}", pid);
            let outcome = match crate::syscall_contract::SyscallContract::execve_outcome(
                pid,
                path.as_ptr() as u64,
                context,
            ) {
                Ok(crate::syscall_contract::SyscallOutcome::Execve(context)) => context,
                Ok(_) => unreachable!(),
                Err(reason) => {
                    crate::serial_println!("[execve] contract reject reason='{}'", reason);
                    crate::println!("sys_execve failure pid={}", pid);
                    return ENOEXEC;
                }
            };
            crate::serial_println!(
                "[execve] image replacement success pid={} current_name='{}' elf_load_entry={:#x} new_rsp={:#x}",
                pid,
                current_name,
                outcome.rip,
                outcome.rsp
            );
        }
        Some(Err(s)) => {
            crate::serial_println!("[execve] image replacement err='{}'", s);
            crate::println!("sys_execve failure pid={}", pid);
            return ENOEXEC;
        }
        None => {
            crate::serial_println!("[execve] no current process during replacement");
            crate::println!("sys_execve failure pid={}", pid);
            return ENOEXEC;
        }
    }

    process::resume_user_from_syscall();
}

pub fn sys_execveat(_dirfd: u64, path: u64, argv: u64, envp: u64, _flags: u64) -> i64 {
    sys_execve(path, argv, envp)
}

pub fn sys_exit(code: i64) -> i64 {
    let pid = process::current_pid().unwrap_or(0);
    crate::serial_println!("[process-exit] syscall pid={} code={}", pid, code);
    if crate::diag::diag_proc_on() {
        crate::println!("sys_exit pid={} code={}", pid, code);
        crate::println!("\n[process] pid={} exited({})", pid, code);
    }

    let _ = crate::process_contract::ProcessContract::request_exit(
        crate::process_contract::ProcessExitRequest {
            pid,
            code,
            reason: crate::process_contract::ProcessExitReason::SyscallExit,
            tag: "sys_exit",
        },
    );

    // Leave syscall-swapped GS state atomically with the non-returning handoff.
    crate::arch::without_interrupts(|| {
        unsafe {
            crate::arch::process::swapgs();
        }
        crate::arch::syscall::mark_kernel_gs_active(false);
        crate::process::scheduler::schedule_handoff_no_save_from("sys_exit")
    })
}

pub fn sys_exit_group(code: i64) -> i64 {
    sys_exit(code)
}

pub fn sys_wait4(pid: u64, status_ptr: u64, options: u64, _rusage: u64) -> i64 {
    let wnohang = options & 1 != 0;
    let parent = current_pid();
    let waiter_pid = parent;
    let want_pid = pid as u32;
    crate::serial_println!(
        "[wait4] enter parent={} waiter={} want={} options={:#x} status_ptr={:#x}",
        parent,
        waiter_pid,
        want_pid,
        options,
        status_ptr
    );
    let wait_request = crate::process_contract::ProcessWaitRequest {
        parent_pid: parent,
        waiter_pid,
        want_pid,
        options,
    };

    if crate::diag::diag_proc_on() {
        crate::println!("wait4-debug: wait4: pid={} options={:#x}", pid, options);
    }

    loop {
        if !wnohang {
            crate::process_contract::ProcessContract::register_child_waiter(wait_request);
        }

        if let Some(reap) =
            crate::process_contract::ProcessContract::try_reap_waitable(wait_request)
        {
            if status_ptr != 0 {
                let current_shadow_pml4 =
                    process::with_current_process(|proc| proc.address_space_pml4())
                        .unwrap_or_else(crate::memory::paging::active_pml4);
                let parent_pml4 = if reap.waiter_pml4 != 0 {
                    reap.waiter_pml4
                } else {
                    current_shadow_pml4
                };
                if crate::diag::diag_proc_on() {
                    crate::println!(
                        "wait4-debug: wait4: writing status={} to {:x} pml4={:#x}",
                        reap.status,
                        status_ptr,
                        parent_pml4
                    );
                }
                if !crate::memory::paging::write_user_in(parent_pml4, status_ptr, reap.status) {
                    if crate::diag::diag_proc_on() {
                        crate::println!(
                            "wait4-debug: wait4: status copy failed ptr={:x}",
                            status_ptr
                        );
                    }
                    return EFAULT;
                }
            }
            let retval = reap.child_pid as i64;
            if crate::diag::diag_proc_on() {
                crate::println!("wait4-debug: wait4: returning {}", retval);
            }
            crate::process_contract::ProcessContract::record_wait_success(wait_request, reap);
            crate::serial_println!(
                "[wait4] return parent={} child={} status={:#x}",
                parent,
                retval,
                if status_ptr == 0 { 0 } else { reap.status }
            );
            return reap.child_pid as i64;
        }
        if wnohang {
            if crate::diag::diag_proc_on() {
                crate::println!("wait4-debug: wait4: returning 0");
            }
            crate::process_contract::ProcessContract::record_wait_nohang(wait_request);
            return 0;
        }

        let interrupted =
            process::with_current_process(|proc| proc.signals.next_actionable().is_some())
                .unwrap_or(false);
        if interrupted {
            if crate::diag::diag_proc_on() {
                crate::println!("wait4-debug: wait4: returning {}", EINTR);
            }
            crate::process_contract::ProcessContract::record_wait_interrupted(wait_request, EINTR);
            return EINTR;
        }

        if crate::diag::diag_proc_on() {
            crate::println!(
                "wait4-before-sleep: parent={} child={} zombie_count={}",
                parent,
                want_pid,
                crate::process_contract::ProcessContract::zombie_count()
            );
        }
        if !crate::process_contract::ProcessContract::block_registered_child_waiter(waiter_pid) {
            continue;
        }
        crate::serial_println!(
            "[wait4] wake parent={} waiter={} want={}",
            parent,
            waiter_pid,
            want_pid
        );
        if crate::diag::diag_proc_on() {
            crate::println!(
                "wait4-after-wake: parent={} child={} zombie_count={}",
                parent,
                want_pid,
                crate::process_contract::ProcessContract::zombie_count()
            );
        }
        crate::process_contract::ProcessContract::unregister_child_waiter(waiter_pid);
    }
}

pub fn sys_kill(pid: u64, sig: u64) -> i64 {
    let sig = sig as u32;
    if sig >= 64 {
        return EINVAL;
    }
    let pid = pid as i64;
    if pid > 0 {
        if sig == 0 {
            let table = crate::process::table::TABLE.lock();
            return if table.procs.contains_key(&(pid as u32)) {
                0
            } else {
                -3
            };
        }
        if crate::ipc::signal::raise_signal_for_pid(pid as u32, sig) {
            0
        } else {
            -3
        }
    } else if pid == 0 {
        let pgid = current_proc(|proc| proc.pgid).unwrap_or(1);
        deliver_signal_to_process_group(pgid, sig)
    } else if pid < -1 {
        deliver_signal_to_process_group((-pid) as u32, sig)
    } else {
        ENOSYS
    }
}

fn deliver_signal_to_process_group(pgid: u32, sig: u32) -> i64 {
    let pids = crate::process::table::TABLE
        .lock()
        .pids_in_process_group(pgid);
    if pids.is_empty() || sig == 0 {
        return if pids.is_empty() { -3 } else { 0 };
    }
    let mut delivered = false;
    for target in pids {
        delivered |= crate::ipc::signal::raise_signal_for_pid(target, sig);
    }
    if delivered { 0 } else { -3 }
}

pub fn sys_tgkill(_tgid: u64, tid: u64, sig: u64) -> i64 {
    sys_kill(tid, sig)
}

pub fn sys_tkill(tid: u64, sig: u64) -> i64 {
    sys_kill(tid, sig)
}

pub fn sys_uname(ptr: u64) -> i64 {
    if ptr < 0x1000 {
        return EFAULT;
    }
    let mut buf = [0u8; 390];
    macro_rules! set_field {
        ($off:expr, $s:expr) => {{
            let b = $s.as_bytes();
            let n = b.len().min(64);
            buf[$off..$off + n].copy_from_slice(&b[..n]);
        }};
    }
    set_field!(0, "Linux");
    set_field!(65, "saios");
    set_field!(130, "6.1.0-saios");
    set_field!(195, "#1 SMP SAIOS");
    set_field!(260, "x86_64");
    set_field!(325, "saios.local");
    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), ptr as *mut u8, buf.len());
    }
    0
}
