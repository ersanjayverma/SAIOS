//! Syscall interface — SYSCALL/SYSRETQ fast path.
//! SAIOS-owned ABI with Linux x86_64-numbered compatibility entry points.

pub mod dispatch;
pub mod handlers;

pub use crate::arch::syscall::{init, set_kernel_stack};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const SAIOS_SYSCALL_BASE: u64 = 0x8000_0000;
pub const SAIOS_SYS_WRITE: u64 = 0x8000_0001; // write(fd, buf, len)
pub const SAIOS_SYS_PUTC: u64 = 0x8000_0002; // putc(fd, byte)
pub const SAIOS_SYS_GETPID: u64 = 0x8000_0003; // getpid()
pub const SAIOS_SYS_YIELD: u64 = 0x8000_0004; // sched_yield()
pub const SAIOS_SYS_EXIT: u64 = 0x8000_0005; // exit(code)
pub const SAIOS_SYS_TIME: u64 = 0x8000_0006; // time()
pub const SAIOS_SYS_INTERNAL_SHELL: u64 = 0x8000_0007; // enter SAIOS internal shell

pub const SAIOS_SYS_PUTS: u64 = SAIOS_SYS_WRITE; // legacy name

#[derive(Clone, Copy)]
pub(crate) struct TraceContext {
    pub pid: u32,
    pub nr: u64,
    pub rip: u64,
}

static TRACE_PID: [AtomicU32; crate::process::table::MAX_CPUS] =
    [const { AtomicU32::new(0) }; crate::process::table::MAX_CPUS];
static TRACE_NR: [AtomicU64; crate::process::table::MAX_CPUS] =
    [const { AtomicU64::new(0) }; crate::process::table::MAX_CPUS];
static TRACE_RIP: [AtomicU64; crate::process::table::MAX_CPUS] =
    [const { AtomicU64::new(0) }; crate::process::table::MAX_CPUS];

fn set_trace_context(ctx: TraceContext) {
    let cpu = crate::process::table::cpu_idx();
    TRACE_PID[cpu].store(ctx.pid, Ordering::Relaxed);
    TRACE_NR[cpu].store(ctx.nr, Ordering::Relaxed);
    TRACE_RIP[cpu].store(ctx.rip, Ordering::Relaxed);
}

pub(crate) fn current_trace_context() -> TraceContext {
    let cpu = crate::process::table::cpu_idx();
    TraceContext {
        pid: TRACE_PID[cpu].load(Ordering::Relaxed),
        nr: TRACE_NR[cpu].load(Ordering::Relaxed),
        rip: TRACE_RIP[cpu].load(Ordering::Relaxed),
    }
}

fn trace_syscall_enter(ctx: TraceContext) {
    if crate::process::table::trace_pid(ctx.pid) {
        crate::println!(
            "[syscall] enter pid={} nr={} rip={:#x}",
            ctx.pid,
            ctx.nr,
            ctx.rip
        );
    }
}

fn trace_syscall_exit(ctx: TraceContext, ret: i64) {
    if crate::process::table::trace_pid(ctx.pid) {
        crate::println!("[syscall] exit pid={} nr={} ret={}", ctx.pid, ctx.nr, ret);
    }
}

pub(crate) fn trace_write_enter(fd: u64, len: u64, buf_ptr: u64) {
    let ctx = current_trace_context();
    if crate::process::table::trace_pid(ctx.pid) {
        crate::println!(
            "[write] enter pid={} fd={} len={} buf={:#x}",
            ctx.pid,
            fd,
            len,
            buf_ptr
        );
    }
}

pub(crate) fn trace_write_exit(ret: i64) {
    let ctx = current_trace_context();
    if crate::process::table::trace_pid(ctx.pid) {
        crate::println!("[write] exit pid={} ret={}", ctx.pid, ret);
    }
}

// -- Linux-numbered compatibility syscalls (x86_64) -------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn syscall_dispatch(
    num: u64,
    a: u64,
    b: u64,
    c: u64,
    d: u64,
    e: u64,
    f: u64,
) -> i64 {
    use handlers::*;

    crate::arch::syscall::mark_kernel_gs_active(true);
    crate::syscall_contract::SyscallContract::validate_stage_or_panic(
        crate::syscall_contract::SyscallStage::Entry,
        "syscall dispatch entry",
    );

    // Process any pending signals before returning to userspace
    crate::process::validate_shadow("syscall entry before refresh");
    let _ = crate::process::refresh_current_from_table();
    crate::process::validate_shadow("syscall entry after refresh");
    refresh_syscall_kernel_stack_from_current();
    let trace_pid = crate::process::current_pid().unwrap_or(0);
    let (trace_rip, _, _) = crate::arch::syscall::saved_user_syscall_site();
    let trace_ctx = TraceContext {
        pid: trace_pid,
        nr: num,
        rip: trace_rip,
    };
    crate::syscall_contract::SyscallContract::observe_entry(num, trace_rip);
    set_trace_context(trace_ctx);
    trace_syscall_enter(trace_ctx);
    let _ = crate::process::fork::take_execve_child_first_syscall_trace(trace_pid);
    crate::syscall_contract::SyscallContract::validate_stage_or_panic(
        crate::syscall_contract::SyscallStage::Dispatch,
        "syscall dispatch call",
    );
    crate::syscall_contract::SyscallContract::observe_dispatch(num, a, b);
    let ret = dispatch_inner(num, a, b, c, d, e, f);
    if ret == handlers::EPERM || ret == handlers::EACCES {
        crate::syscall_contract::SyscallContract::observe_denied(num, ret);
    }
    crate::syscall_contract::SyscallContract::observe_exit(num, ret);
    trace_syscall_exit(trace_ctx, ret);
    crate::syscall_contract::SyscallContract::validate_stage_or_panic(
        crate::syscall_contract::SyscallStage::Return,
        "syscall dispatch return",
    );
    crate::process::validate_shadow("syscall exit before pending signals");
    crate::ipc::signal::process_pending();
    let _ = crate::process::refresh_current_from_table();
    crate::process::validate_shadow("syscall exit after pending signals");
    refresh_syscall_kernel_stack_from_current();
    crate::arch::syscall::mark_kernel_gs_active(false);
    ret
}

fn refresh_syscall_kernel_stack_from_current() {
    let kstack = {
        let table = crate::process::table::TABLE.lock();
        table
            .current_ref()
            .map(|p| p.kernel_stack_top())
            .unwrap_or(0)
    };
    if kstack != 0 {
        crate::syscall::set_kernel_stack(kstack);
    }
}

fn dispatch_inner(num: u64, a: u64, b: u64, c: u64, d: u64, e: u64, f: u64) -> i64 {
    use handlers::*;

    // Check if the current process is a Windows process
    let is_windows = crate::process::current_is_windows_process();

    if is_windows {
        if crate::compatibility_contract::CompatibilityContract::require_layer(
            crate::compatibility_contract::CompatibilityLayer::WindowsCompatibility,
        )
        .is_err()
        {
            return ENOSYS;
        }
        // Route Windows-marked processes to the NT compatibility layer.
        // Windows passes syscall number in eax
        return crate::windows::syscall::handle_nt_syscall(num, a, b, c, d, e, f) as i64;
    }

    // SAIOS-native syscalls live above 0x8000_0000 so they cannot collide
    // with the compatibility number range. Routing them through a second dispatch layer keeps
    // the boundary between the two syscall sets explicit and lets the SAIOS
    // set grow without mixing native-only calls into the compatibility table.
    if num >= SAIOS_SYSCALL_BASE {
        return saios_dispatch(num, a, b, c, d, e, f);
    }
    match num {
        // File I/O
        0 => sys_read(a, b, c),
        1 => sys_write(a, b, c),
        2 => sys_open(a, b, c),
        3 => sys_close(a),
        4 => sys_stat(a, b),
        5 => sys_fstat(a, b),
        6 => sys_lstat(a, b),
        8 => sys_lseek(a, b, c),
        9 => handlers::sys_mmap(a, b, c, d, e, f),
        10 => sys_mprotect(a, b, c),
        11 => sys_munmap(a, b),
        12 => sys_brk(a),
        13 => sys_rt_sigaction(a, b, c, d),
        14 => sys_rt_sigprocmask(a, b, c, d),
        15 => sys_rt_sigreturn(),
        16 => sys_ioctl(a, b, c),
        17 => sys_pread64(a, b, c, d),
        18 => sys_pwrite64(a, b, c, d),
        19 => sys_readv(a, b, c),
        20 => sys_writev(a, b, c),
        21 => sys_access(a, b),
        22 => sys_pipe(a),
        23 => ENOSYS, // select unsupported
        24 => {
            crate::process::scheduler::yield_now();
            0
        }
        25 => ENOSYS, // mremap unsupported until flags/overlap semantics are implemented
        28 => sys_madvise(a, b, c),
        32 => sys_dup(a),
        33 => sys_dup2(a, b),
        35 => sys_nanosleep(a, b),
        37 => sys_alarm(a),
        38 => sys_setitimer(a, b, c),
        39 => sys_getpid(),
        41 => sys_socket(a, b, c),
        42 => sys_connect(a, b, c),
        43 => sys_accept(a, b, c),
        44 => sys_sendto(a, b, c, d, e, f),
        45 => sys_recvfrom(a, b, c, d, e, f),
        46 => sys_sendmsg(a, b, c),
        47 => sys_recvmsg(a, b, c),
        48 => sys_shutdown(a, b),
        49 => sys_bind(a, b, c),
        50 => sys_listen(a, b),
        51 => sys_getsockname(a, b, c),
        52 => sys_getpeername(a, b, c),
        53 => sys_socketpair(a, b, c, d),
        54 => sys_setsockopt(a, b, c, d, e),
        55 => sys_getsockopt(a, b, c, d, e),
        56 => sys_clone(a, b, c, d, e),
        57 => sys_fork(),
        58 => sys_vfork(),
        59 => sys_execve(a, b, c),
        60 => sys_exit(a as i64),
        61 => sys_wait4(a, b, c, d),
        62 => sys_kill(a, b),
        63 => sys_uname(a),
        72 => sys_fcntl(a, b, c),
        73 => ENOSYS, // flock unsupported
        74 => sys_fsync(a),
        75 => sys_fdatasync(a),
        76 => sys_truncate(a, b),
        77 => sys_ftruncate(a, b),
        78 => sys_getdents(a, b, c),
        79 => sys_getcwd(a, b),
        80 => sys_chdir(a),
        81 => sys_fchdir(a),
        82 => sys_rename(a, b),
        83 => sys_mkdir(a, b),
        84 => sys_rmdir(a),
        85 => sys_creat(a, b),
        86 => sys_link(a, b),
        87 => sys_unlink(a),
        88 => sys_symlink(a, b),
        89 => sys_readlink(a, b, c),
        90 => sys_chmod(a, b),
        91 => sys_fchmod(a, b),
        92 => sys_chown(a, b, c),
        93 => sys_fchown(a, b, c),
        94 => sys_lchown(a, b, c),
        95 => sys_umask(a),
        96 => sys_gettimeofday(a, b),
        97 => sys_getrlimit(a, b),
        98 => sys_getrusage(a, b),
        99 => sys_sysinfo(a),
        100 => sys_times(a),
        102 => sys_getuid(),
        104 => sys_getgid(),
        105 => sys_setuid(a),
        106 => sys_setgid(a),
        107 => sys_geteuid(),
        108 => sys_getegid(),
        109 => sys_setpgid(a, b),
        110 => sys_getppid(),
        111 => sys_getpgrp(),
        112 => sys_setsid(),
        113 => sys_setreuid(a, b),
        114 => sys_setregid(a, b),
        115 => sys_getgroups(a, b),
        116 => sys_setgroups(a, b),
        117 => sys_setresuid(a, b, c),
        118 => sys_getresuid(a, b, c),
        119 => sys_setresgid(a, b, c),
        120 => sys_getresgid(a, b, c),
        121 => sys_getpgid(a),
        122 => sys_setfsuid(a),
        123 => sys_setfsgid(a),
        124 => sys_getsid(a),
        131 => ENOSYS, // sigaltstack unsupported
        137 => sys_statfs(a, b),
        138 => sys_fstatfs(a, b),
        140 => ENOSYS, // getpriority unsupported
        158 => sys_arch_prctl(a, b),
        160 => ENOSYS,            // setrlimit unsupported
        161 => ENOSYS,            // chroot unsupported
        162 => sys_sync_device(), // sync
        163 => ENOSYS,            // acct unsupported
        186 => sys_gettid(),
        202 => sys_futex(a, b, c, d, e, f),
        218 => sys_set_tid_address(a),
        220 => ENOSYS, // semtimedop unsupported
        228 => sys_clock_gettime(a, b),
        229 => sys_clock_getres(a, b),
        230 => sys_clock_nanosleep(a, b, c, d),
        231 => sys_exit_group(a as i64),
        232 => ENOSYS, // epoll_wait unsupported
        233 => ENOSYS, // epoll_ctl unsupported
        234 => sys_tgkill(a, b, c),
        235 => ENOSYS, // utimes unsupported
        257 => sys_openat(a, b, c, d),
        258 => sys_mkdirat(a, b, c),
        259 => sys_mknodat(a, b, c, d),
        260 => sys_fchownat(a, b, c, d, e),
        261 => ENOSYS, // futimesat unsupported
        262 => sys_newfstatat(a, b, c, d),
        263 => sys_unlinkat(a, b, c),
        264 => sys_renameat(a, b, c, d),
        265 => ENOSYS, // linkat unsupported
        266 => ENOSYS, // symlinkat unsupported
        267 => ENOSYS, // readlinkat unsupported
        268 => sys_fchmodat(a, b, c, d),
        269 => sys_faccessat(a, b, c, d),
        270 => ENOSYS, // pselect6 unsupported
        271 => ENOSYS, // ppoll unsupported
        280 => ENOSYS, // utimensat unsupported
        281 => ENOSYS, // epoll_pwait unsupported
        283 => ENOSYS, // timerfd_create unsupported
        284 => ENOSYS, // eventfd unsupported
        // posix_openpt (grantpt/unlockpt handled in libc, slave opened via open)
        // We handle this via O_RDWR on /dev/ptmx
        285 => ENOSYS, // fallocate unsupported
        288 => ENOSYS, // accept4 unsupported until flags are honored
        290 => ENOSYS, // eventfd2 unsupported
        291 => ENOSYS, // epoll_create1 unsupported
        292 => sys_dup3(a, b, c),
        293 => sys_pipe2(a, b),
        295 => ENOSYS, // preadv unsupported
        302 => ENOSYS, // prlimit64 unsupported
        303 => ENOSYS, // name_to_handle_at unsupported
        314 => ENOSYS, // sched_setattr unsupported
        316 => sys_renameat2(a, b, c, d, e),
        318 => sys_getrandom(a, b, c),
        319 => ENOSYS, // memfd_create unsupported
        322 => sys_execveat(a, b, c, d, e),
        332 => ENOSYS, // statx unsupported
        334 => ENOSYS, // rseq unsupported
        _ => {
            crate::serial_println!("[syscall] unimplemented #{}", num);
            -38 // ENOSYS
        }
    }
}

// -- SAIOS-native syscall range (above 0x8000_0000) -------------------------
//
// We deliberately use a high-bit prefix so the Linux compat range (0..0x8000_0000)
// stays untouched and the SAIOS-native set is unambiguously its own layer.  The
// first few slots are reserved for the small kernel-services ABI that the
// validation suite and any future SAIOS-native ELF relies on.

/// Dispatch a SAIOS-native syscall.  The Linux layer (above) returns -ENOSYS
/// for any number outside 0..334; this layer handles the SAIOS-native numbers
/// in 0x8000_0000..0x8000_FFFF.  Future SAIOS-native services (signal hooks,
/// AI APIs, BHB, etc.) extend this match.
fn saios_dispatch(num: u64, a: u64, b: u64, c: u64, _d: u64, _e: u64, _f: u64) -> i64 {
    match num {
        SAIOS_SYS_WRITE => handlers::sys_saios_write(a, b, c),
        SAIOS_SYS_PUTC => handlers::sys_saios_putc(a, b),
        SAIOS_SYS_GETPID => handlers::sys_getpid(),
        SAIOS_SYS_YIELD => {
            crate::process::scheduler::yield_now();
            0
        }
        SAIOS_SYS_EXIT => handlers::sys_exit(a as i64),
        SAIOS_SYS_TIME => handlers::sys_time(a),
        SAIOS_SYS_INTERNAL_SHELL => handlers::sys_internal_shell(),
        _ => {
            crate::serial_println!("[syscall] SAIOS-native #{} unimplemented", num);
            -38 // ENOSYS
        }
    }
}
