//! Process management â€” PCB, lifecycle, context.

pub mod elf;
pub mod exec;
pub mod fork;
pub mod kthread;
pub mod kwork;
mod metadata;
pub mod pi;
pub mod scheduler;
pub mod signal;
pub mod table;
pub mod thread;
use alloc::format;
use alloc::string::{String, ToString};
use core::sync::atomic::{AtomicBool, Ordering};
pub use metadata::*;
use spin::{Mutex, MutexGuard};

pub struct PerCpuCurrent([Mutex<Option<Process>>; table::MAX_CPUS]);

impl PerCpuCurrent {
    pub const fn new() -> Self {
        Self([const { Mutex::new(None) }; table::MAX_CPUS])
    }

    pub fn lock(&self) -> MutexGuard<'_, Option<Process>> {
        self.0[table::cpu_idx()].lock()
    }
}

pub struct PerCpuUserModeActive([AtomicBool; table::MAX_CPUS]);

impl PerCpuUserModeActive {
    pub const fn new() -> Self {
        Self([const { AtomicBool::new(false) }; table::MAX_CPUS])
    }

    pub fn load(&self, order: Ordering) -> bool {
        self.0[table::cpu_idx()].load(order)
    }

    pub fn store(&self, value: bool, order: Ordering) {
        self.0[table::cpu_idx()].store(value, order);
    }
}

pub static CURRENT: PerCpuCurrent = PerCpuCurrent::new();
pub static USER_MODE_ACTIVE: PerCpuUserModeActive = PerCpuUserModeActive::new();
static NEXT_PID: Mutex<u32> = Mutex::new(1);

pub fn alloc_pid() -> u32 {
    let mut p = NEXT_PID.lock();
    let pid = *p;
    *p += 1;
    pid
}

/// Spawn a process from a VFS path (ELF binary).
pub fn spawn(path: &str) -> Result<u32, &'static str> {
    spawn_with_args_env(path, &[], &[])
}

/// Spawn a process from a VFS path (ELF binary) with explicit argv/envp.
pub fn spawn_with_args_env(
    path: &str,
    argv: &[String],
    envp: &[String],
) -> Result<u32, &'static str> {
    let image = match crate::vfs_contract::VfsContract::exec_image(path) {
        Ok(image) => {
            crate::serial_println!(
                "[spawn] vfs_exec ok path='{}' type={:?} size={} bytes={} magic={:02x}{:02x}{:02x}{:02x}",
                path,
                image.file_type,
                image.size,
                image.data.len(),
                image.data.first().copied().unwrap_or(0),
                image.data.get(1).copied().unwrap_or(0),
                image.data.get(2).copied().unwrap_or(0),
                image.data.get(3).copied().unwrap_or(0)
            );
            image
        }
        Err(error) => {
            crate::serial_println!(
                "[spawn] vfs_exec err path='{}' errno={} error={:?}",
                path,
                error.to_errno(),
                error
            );
            return Err("process: executable load failed");
        }
    };

    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let parent_pid = {
        let table = crate::process::table::TABLE.lock();
        table.current_ref().map(|parent| parent.pid).unwrap_or(0)
    };
    let mut proc = crate::process_contract::ProcessContract::create(
        crate::process_contract::ProcessCreationRequest {
            name: name.clone(),
            parent_pid,
            kind: crate::process_contract::ProcessCreationKind::UserProcess,
            tag: "spawn",
        },
    );
    let pid = proc.pid;
    if crate::diag::diag_proc_on() {
        crate::serial_println!("[proc] create pid={} name='{}'", pid, name);
    }
    proc.namespace_view = crate::vfs::namespace::NamespaceView::Linux;
    if parent_pid != 0 {
        let table = crate::process::table::TABLE.lock();
        if let Some(parent) = table.procs.get(&parent_pid) {
            crate::process_contract::ProcessContract::inherit_parent_metadata(
                &mut proc,
                parent,
                false,
                false,
                "spawn_inherit",
            );
        }
    }
    proc.boot_cpu_affine = false;
    proc.scheduling = SchedulingPolicy::unrestricted();

    // Give the process its own address space before loading the image so the
    // ELF segments and user stack land in its private PML4, not the shared one.
    proc.ensure_address_space()?;

    setup_user_stack(&mut proc)?;

    let default_argv;
    let exec_argv = if argv.is_empty() {
        default_argv = alloc::vec![path.to_string()];
        default_argv.as_slice()
    } else {
        argv
    };

    let (entry, rsp) = match exec::do_exec(&mut proc, &image.data, exec_argv, envp) {
        Ok(context) => context,
        Err(error) => {
            crate::serial_println!(
                "[spawn] elf_load err path='{}' pid={} reason={}",
                path,
                pid,
                error
            );
            return Err(error);
        }
    };
    crate::process_contract::ProcessContract::finalize_user_process_image(
        &mut proc,
        entry,
        rsp,
        "spawn_image",
    );

    setup_user_entry_frame(&mut proc);

    if crate::diag::diag_proc_on() {
        crate::serial_println!(
            "[proc] start  pid={} name='{}' entry={:#x}",
            pid,
            proc.name,
            entry
        );
    }
    crate::process_contract::ProcessContract::validate_creation_ready_or_panic(
        crate::process_contract::ProcessCreationKind::UserProcess,
        &proc,
        "spawn_ready",
    );
    track_runnable_process(proc);
    Ok(pid)
}

fn setup_user_entry_frame(proc: &mut Process) {
    let top = proc.kernel_stack_top() & !0xF;
    let sp = (top - 8 * 8) as *mut u64;
    unsafe {
        *sp.add(0) = 0x2;
        *sp.add(1) = 0;
        *sp.add(2) = 0;
        *sp.add(3) = 0;
        *sp.add(4) = 0;
        *sp.add(5) = 0;
        *sp.add(6) = 0;
        *sp.add(7) = user_process_trampoline as *const () as usize as u64;
    }
    proc.kernel_rsp = sp as u64;
}

extern "C" fn user_process_trampoline() -> ! {
    crate::process::scheduler::finish_switch();
    let _ = crate::process::refresh_current_from_table();

    let (pid, rip, rsp, rflags, pml4) = {
        let table = crate::process::table::TABLE.lock();
        let proc = table
            .current_ref()
            .expect("user_process_trampoline: no current process");
        (
            proc.pid,
            proc.rip,
            proc.rsp,
            proc.rflags,
            proc.address_space_pml4(),
        )
    };

    crate::execution_contract::ExecutionContract::activate_process_address_space(
        pid,
        pml4,
        crate::execution_contract::ExecutionTransition::IretqReturn,
        "user_process_trampoline",
    );
    if crate::arch::syscall::kernel_gs_active() {
        crate::process::jump_to_user_from_syscall(rip, rsp, rflags)
    } else {
        crate::process::jump_to_user(rip, rsp, rflags)
    }
}

fn shadow_from_process(proc: &Process) -> Process {
    let mut shadow = Process::new(proc.pid, proc.name.clone());
    shadow.parent_pid = proc.parent_pid;
    shadow.state = proc.state.clone();
    shadow.cwd = proc.cwd.clone();
    shadow.namespace_view = proc.namespace_view;
    shadow.mount_namespace = proc.mount_namespace.clone();
    shadow.uid = proc.uid;
    shadow.gid = proc.gid;
    shadow.euid = proc.euid;
    shadow.egid = proc.egid;
    shadow.suid = proc.suid;
    shadow.sgid = proc.sgid;
    shadow.rip = proc.rip;
    shadow.rsp = proc.rsp;
    shadow.rflags = proc.rflags;
    shadow.program_entry = proc.program_entry;
    shadow.fork_rax = proc.fork_rax;
    shadow.fork_rdi = proc.fork_rdi;
    shadow.fork_rsi = proc.fork_rsi;
    shadow.fork_rdx = proc.fork_rdx;
    shadow.fork_r8 = proc.fork_r8;
    shadow.fork_r9 = proc.fork_r9;
    shadow.fork_r10 = proc.fork_r10;
    shadow.fork_rbx = proc.fork_rbx;
    shadow.fork_rbp = proc.fork_rbp;
    shadow.fork_r12 = proc.fork_r12;
    shadow.fork_r13 = proc.fork_r13;
    shadow.fork_r14 = proc.fork_r14;
    shadow.fork_r15 = proc.fork_r15;
    shadow.brk = proc.brk;
    shadow.mmap_base = proc.mmap_base;
    shadow.install_address_space(proc.address_space);
    shadow.owns_address_space = proc.owns_address_space;
    shadow.boot_cpu_affine = proc.boot_cpu_affine;
    shadow.scheduling = proc.scheduling;
    shadow.stack_base = proc.stack_base;
    shadow.stack_size = proc.stack_size;
    shadow.tls_info = proc.tls_info.clone();
    shadow.interpreter = proc.interpreter.clone();
    shadow.is_windows_process = proc.is_windows_process;
    shadow.peb_addr = proc.peb_addr;
    shadow.teb_addr = proc.teb_addr;
    shadow.fs_base = proc.fs_base.clone();
    shadow.fd_table = proc.fd_table.clone();
    shadow.signals = proc.signals.clone();
    shadow.session_id = proc.session_id;
    shadow.pgid = proc.pgid;
    shadow.exit_code = proc.exit_code;
    shadow.clear_child_tid = proc.clear_child_tid;
    shadow
}

fn sync_process_shadow(dst: &mut Process, src: &Process) {
    dst.parent_pid = src.parent_pid;
    dst.state = src.state.clone();
    dst.name = src.name.clone();
    dst.cwd = src.cwd.clone();
    dst.namespace_view = src.namespace_view;
    dst.mount_namespace = src.mount_namespace.clone();
    dst.uid = src.uid;
    dst.gid = src.gid;
    dst.euid = src.euid;
    dst.egid = src.egid;
    dst.suid = src.suid;
    dst.sgid = src.sgid;
    dst.rip = src.rip;
    dst.rsp = src.rsp;
    dst.rflags = src.rflags;
    dst.program_entry = src.program_entry;
    dst.fork_rax = src.fork_rax;
    dst.fork_rdi = src.fork_rdi;
    dst.fork_rsi = src.fork_rsi;
    dst.fork_rdx = src.fork_rdx;
    dst.fork_r8 = src.fork_r8;
    dst.fork_r9 = src.fork_r9;
    dst.fork_r10 = src.fork_r10;
    dst.fork_rbx = src.fork_rbx;
    dst.fork_rbp = src.fork_rbp;
    dst.fork_r12 = src.fork_r12;
    dst.fork_r13 = src.fork_r13;
    dst.fork_r14 = src.fork_r14;
    dst.fork_r15 = src.fork_r15;
    dst.brk = src.brk;
    dst.mmap_base = src.mmap_base;
    dst.install_address_space(src.address_space);
    dst.owns_address_space = src.owns_address_space;
    dst.stack_base = src.stack_base;
    dst.stack_size = src.stack_size;
    dst.tls_info = src.tls_info.clone();
    dst.interpreter = src.interpreter.clone();
    dst.is_windows_process = src.is_windows_process;
    dst.peb_addr = src.peb_addr;
    dst.teb_addr = src.teb_addr;
    dst.fs_base = src.fs_base.clone();
    dst.fd_table = src.fd_table.clone();
    dst.signals = src.signals.clone();
    dst.session_id = src.session_id;
    dst.pgid = src.pgid;
    dst.exit_code = src.exit_code;
    dst.clear_child_tid = src.clear_child_tid;
}

#[cfg(debug_assertions)]
fn assert_shadow_matches_table(context: &str) {
    let shadow = CURRENT.lock();
    let Some(shadow) = shadow.as_ref() else {
        return;
    };

    let table = crate::process::table::TABLE.lock();
    let current_pid = table.current_pid();
    assert_eq!(
        current_pid, shadow.pid,
        "{context}: CURRENT pid {} diverged from TABLE current pid {}",
        shadow.pid, current_pid
    );
    let canonical = table
        .procs
        .get(&shadow.pid)
        .unwrap_or_else(|| panic!("{context}: CURRENT pid {} missing from TABLE", shadow.pid));

    assert_eq!(shadow.state, canonical.state, "{context}: state diverged");
    assert_eq!(
        shadow.parent_pid, canonical.parent_pid,
        "{context}: parent_pid diverged"
    );
    assert_eq!(shadow.rip, canonical.rip, "{context}: rip diverged");
    assert_eq!(shadow.rsp, canonical.rsp, "{context}: rsp diverged");
    assert_eq!(
        shadow.rflags, canonical.rflags,
        "{context}: rflags diverged"
    );
    assert_eq!(shadow.cwd, canonical.cwd, "{context}: cwd diverged");
    assert_eq!(shadow.brk, canonical.brk, "{context}: brk diverged");
    assert_eq!(
        shadow.mmap_base, canonical.mmap_base,
        "{context}: mmap_base diverged"
    );
    assert_eq!(
        shadow.address_space_pml4(),
        canonical.address_space_pml4(),
        "{context}: pml4 diverged"
    );
    assert_eq!(
        shadow.owns_address_space, canonical.owns_address_space,
        "{context}: owns_address_space diverged"
    );
    assert_eq!(
        shadow.stack_base, canonical.stack_base,
        "{context}: stack_base diverged"
    );
    assert_eq!(
        shadow.stack_size, canonical.stack_size,
        "{context}: stack_size diverged"
    );
    assert_eq!(
        shadow.fs_base, canonical.fs_base,
        "{context}: fs_base diverged"
    );
    assert_eq!(
        shadow.clear_child_tid, canonical.clear_child_tid,
        "{context}: clear_child_tid diverged"
    );
    assert_eq!(
        shadow.exit_code, canonical.exit_code,
        "{context}: exit_code diverged"
    );
    assert_eq!(
        shadow.signals.handlers, canonical.signals.handlers,
        "{context}: signal handlers diverged"
    );
    assert_eq!(
        shadow.signals.pending, canonical.signals.pending,
        "{context}: signal pending diverged"
    );
    assert_eq!(
        shadow.signals.blocked, canonical.signals.blocked,
        "{context}: signal blocked diverged"
    );
    assert_eq!(shadow.suid, canonical.suid, "{context}: suid diverged");
    assert_eq!(shadow.sgid, canonical.sgid, "{context}: sgid diverged");
    assert_eq!(
        shadow.session_id, canonical.session_id,
        "{context}: session_id diverged"
    );
    assert_eq!(shadow.pgid, canonical.pgid, "{context}: pgid diverged");
}

#[cfg(not(debug_assertions))]
fn assert_shadow_matches_table(_context: &str) {}

pub fn track_current_process(proc: Process) {
    crate::process_contract::ProcessContract::admit_detached(proc, "track_current_process");
}

pub fn track_runnable_process(proc: Process) {
    crate::process_contract::ProcessContract::admit_runnable(
        proc,
        "spawn_runnable",
        "process::track_runnable_process",
    );
}

pub fn current_pid() -> Option<u32> {
    let pid = crate::process::table::TABLE.lock().current_pid();
    if pid == 0 { None } else { Some(pid) }
}

pub fn refresh_current_from_pid(pid: u32) -> bool {
    let shadow = {
        let table = crate::process::table::TABLE.lock();
        table.procs.get(&pid).map(shadow_from_process)
    };
    let Some(shadow) = shadow else {
        return false;
    };
    *CURRENT.lock() = Some(shadow);
    assert_shadow_matches_table("refresh_current_from_pid");
    true
}

pub fn refresh_current_from_table() -> bool {
    let pid = crate::process::table::TABLE.lock().current_pid();
    if pid == 0 {
        return false;
    }
    refresh_current_from_pid(pid)
}

pub fn clear_current_shadow() {
    *CURRENT.lock() = None;
}

pub fn with_current_process<R>(f: impl FnOnce(&Process) -> R) -> Option<R> {
    let table = crate::process::table::TABLE.lock();
    let proc = table.current_ref()?;
    Some(f(proc))
}

pub fn current_is_windows_process() -> bool {
    with_current_process(|proc| proc.is_windows_process).unwrap_or(false)
}

pub fn with_current_process_mut<R>(f: impl FnOnce(&mut Process) -> R) -> Option<R> {
    let pid = current_pid()?;
    let result = with_process_mut_by_pid(pid, f)?;
    let _ = refresh_current_from_pid(pid);
    Some(result)
}

pub fn with_process_mut_by_pid<R>(pid: u32, f: impl FnOnce(&mut Process) -> R) -> Option<R> {
    let result = {
        let mut table = crate::process::table::TABLE.lock();
        let proc = table.procs.get_mut(&pid)?;
        f(proc)
    };

    let should_refresh = CURRENT.lock().as_ref().map(|p| p.pid) == Some(pid);
    if should_refresh {
        let _ = refresh_current_from_pid(pid);
    }
    Some(result)
}

pub fn block_current() {
    assert_shadow_matches_table("block_current before schedule");
    {
        let mut table = crate::process::table::TABLE.lock();
        if crate::process_contract::ProcessContract::block_current(
            &mut table,
            "process block_current",
        )
        .is_none()
        {
            return;
        }
    }
    crate::process::scheduler::schedule_blocking_from("process_block_current");
    let _ = refresh_current_from_table();
    assert_shadow_matches_table("block_current after schedule");
}

pub fn validate_shadow(context: &str) {
    assert_shadow_matches_table(context);
}

pub fn sanitize_user_rflags(rflags: u64) -> u64 {
    const RESERVED_HIGH: u64 = 0xFFFF_FFFF_FF00_0000;
    const TRAP_FLAG: u64 = 1 << 8;
    const INTERRUPT_FLAG: u64 = 1 << 9;
    const IOPL: u64 = 0b11 << 12;
    const NESTED_TASK: u64 = 1 << 14;
    const RESUME_FLAG: u64 = 1 << 16;
    const VIRTUAL_8086: u64 = 1 << 17;

    (rflags | 0x2 | INTERRUPT_FLAG)
        & !(RESERVED_HIGH | TRAP_FLAG | IOPL | NESTED_TASK | RESUME_FLAG | VIRTUAL_8086)
}

/// Initial stack size for user processes.
const USER_STACK_INIT_PAGES: u64 = 256; // 1 MiB initial allocation

fn setup_user_stack(proc: &mut Process) -> Result<(), &'static str> {
    let phys = crate::memory_contract::MemoryContract::alloc_process_frames(
        USER_STACK_INIT_PAGES as usize,
        crate::memory_contract::PageOwner::Process(proc.pid),
        "user_stack_setup",
    )
    .ok_or("process: OOM for user stack")?;
    let base = USER_STACK_TOP - USER_STACK_INIT_PAGES * 0x1000;
    let target = if proc.address_space_pml4() != 0 {
        proc.address_space_pml4()
    } else {
        crate::memory::paging::active_pml4()
    };
    if crate::address_space_contract::AddressSpaceContract::map_user_frames_in(
        crate::address_space_contract::AddressSpaceHandle {
            id: target,
            pml4: target,
            owner_pid: proc.pid,
        },
        base,
        phys,
        USER_STACK_INIT_PAGES as usize,
    )
    .is_err()
    {
        crate::memory_contract::MemoryContract::free_frames(
            phys,
            USER_STACK_INIT_PAGES as usize,
            "user_stack_setup_failed",
        );
        return Err("process: OOM for user stack (mapping failed)");
    }
    proc.rsp = USER_STACK_TOP - 128; // ABI red zone
    proc.stack_base = base;
    proc.stack_size = USER_STACK_INIT_PAGES * 0x1000;
    crate::serial_println!(
        "[stack] bottom={:#x} top={:#x} pages={}",
        proc.stack_base,
        USER_STACK_TOP,
        USER_STACK_INIT_PAGES
    );
    Ok(())
}

/// Grow the user stack by one page (4KB) and map it.
/// Called when a page fault occurs in the stack region.
/// Returns true if stack was grown, false if growth limit reached.
pub fn grow_user_stack(proc: &mut Process) -> Result<bool, &'static str> {
    // Check if fault address is below current stack allocation (stack grows down)
    let cr2 = crate::arch::fault_address();
    let max_stack_size = USER_STACK_SIZE as u64;
    let stack_limit = USER_STACK_TOP - max_stack_size;

    // For a downward-growing stack (from USER_STACK_TOP toward lower addresses):
    // - proc.stack_base is the lowest mapped address
    // - A fault at cr2 < proc.stack_base means we need to grow
    if cr2 < proc.stack_base {
        // This is a stack growth fault - proceed with growth
    } else if cr2 >= proc.stack_base && cr2 < USER_STACK_TOP {
        // Already mapped this region, this should not happen for a valid stack fault
        return Ok(false);
    } else {
        // Fault is outside the stack region entirely
        return Ok(false);
    }

    if cr2 < stack_limit || proc.stack_size >= max_stack_size {
        return Err("process: stack limit reached");
    }

    let target = if proc.address_space_pml4() != 0 {
        proc.address_space_pml4()
    } else {
        crate::memory::paging::active_pml4()
    };

    while cr2 < proc.stack_base {
        if proc.stack_size >= max_stack_size {
            return Err("process: stack limit reached");
        }

        let phys = crate::memory_contract::MemoryContract::alloc_process_frames(
            1,
            crate::memory_contract::PageOwner::Process(proc.pid),
            "user_stack_growth",
        )
        .ok_or("process: OOM for stack growth")?;
        let new_base = proc.stack_base - 0x1000;
        if crate::address_space_contract::AddressSpaceContract::map_user_frames_in(
            crate::address_space_contract::AddressSpaceHandle {
                id: target,
                pml4: target,
                owner_pid: proc.pid,
            },
            new_base,
            phys,
            1,
        )
        .is_err()
        {
            crate::memory_contract::MemoryContract::free_frame(phys, "user_stack_growth_failed");
            return Err("process: OOM for stack growth (mapping failed)");
        }

        proc.stack_base = new_base;
        proc.stack_size += 0x1000;
    }

    Ok(true)
}

// Kernel setjmp/longjmp (arch/x86_64/process/context_switch.s) — kept for potential future use.
// The buffer must be 9 u64 = 72 bytes to hold: rbx, rbp, r12-15, rflags, rsp, rip.
// These are no longer used by the scheduler-owned process lifecycle.

/// Enter ring 3 with the CURRENT process's saved context, without arming a new
/// return point.  Used by execve/sigreturn, which re-enter userspace from
/// inside a syscall.
pub fn resume_user() -> ! {
    let (pid, rip, rsp, rflags, pml4) = {
        let table = crate::process::table::TABLE.lock();
        let p = table.current_ref().expect("resume_user: no process");
        (p.pid, p.rip, p.rsp, p.rflags, p.address_space_pml4())
    };
    // Activate the process's private address space (kernel stays mapped via the
    // shared PML4[0]). No-op if it has none / is already active.
    crate::execution_contract::ExecutionContract::activate_process_address_space(
        pid,
        pml4,
        crate::execution_contract::ExecutionTransition::IretqReturn,
        "resume_user",
    );
    jump_to_user(rip, rsp, rflags)
}

/// Resume userspace from inside a syscall handler through the single iretq
/// return shape. The arch helper writes active GS=user and KERNEL_GS=per-CPU.
pub fn resume_user_from_syscall() -> ! {
    let (pid, rip, rsp, rflags, pml4) = {
        let table = crate::process::table::TABLE.lock();
        let p = table
            .current_ref()
            .expect("resume_user_from_syscall: no process");
        (p.pid, p.rip, p.rsp, p.rflags, p.address_space_pml4())
    };
    crate::execution_contract::ExecutionContract::activate_process_address_space(
        pid,
        pml4,
        crate::execution_contract::ExecutionTransition::SyscallExit,
        "resume_user_from_syscall",
    );
    jump_to_user_from_syscall(rip, rsp, rflags)
}

/// Public alias for use by scheduler
pub fn jump_to_user(rip: u64, rsp: u64, rflags: u64) -> ! {
    let rflags = sanitize_user_rflags(rflags);
    let (pid, fs_base, gs_base, kstack) = {
        let table = crate::process::table::TABLE.lock();
        let p = table.current_ref().expect("jump_to_user: no process");
        (
            p.pid,
            p.fs_base.fs_base,
            p.fs_base.gs_base,
            p.kernel_stack_top(),
        )
    };
    if kstack != 0 {
        crate::syscall::set_kernel_stack(kstack);
    }
    if crate::diag::diag_proc_on() {
        crate::serial_println!("[resume] pid={} rip={:#x} rsp={:#x}", pid, rip, rsp);
    }
    validate_user_return_or_panic(
        crate::execution_contract::UserReturnOrigin::Direct,
        rip,
        rsp,
        rflags,
        fs_base,
        gs_base,
    );

    crate::arch::process::jump_to_userspace(rip, rsp, rflags, fs_base, gs_base)
}

pub fn jump_to_user_from_syscall(rip: u64, rsp: u64, rflags: u64) -> ! {
    let rflags = sanitize_user_rflags(rflags);
    let (pid, fs_base, gs_base, kstack) = {
        let table = crate::process::table::TABLE.lock();
        let p = table
            .current_ref()
            .expect("jump_to_user_from_syscall: no process");
        (
            p.pid,
            p.fs_base.fs_base,
            p.fs_base.gs_base,
            p.kernel_stack_top(),
        )
    };
    if kstack != 0 {
        crate::syscall::set_kernel_stack(kstack);
    }
    crate::execution_contract::ExecutionContract::dump_user_return(
        "jump_to_user_from_syscall",
        pid,
        rip,
        rsp,
        rflags,
        fs_base,
        gs_base,
    );
    validate_user_return_or_panic(
        crate::execution_contract::UserReturnOrigin::Syscall,
        rip,
        rsp,
        rflags,
        fs_base,
        gs_base,
    );
    crate::arch::process::jump_to_userspace_from_syscall(rip, rsp, rflags, fs_base, gs_base)
}

#[allow(clippy::too_many_arguments)]
pub fn jump_to_user_with_registers(
    rip: u64,
    rsp: u64,
    rflags: u64,
    rax: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    rbx: u64,
    rbp: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
) -> ! {
    let rflags = sanitize_user_rflags(rflags);
    let (pid, fs_base, gs_base, kstack) = {
        let table = crate::process::table::TABLE.lock();
        let p = table
            .current_ref()
            .expect("jump_to_user_with_registers: no process");
        (
            p.pid,
            p.fs_base.fs_base,
            p.fs_base.gs_base,
            p.kernel_stack_top(),
        )
    };
    if kstack != 0 {
        crate::syscall::set_kernel_stack(kstack);
    }
    if crate::diag::diag_proc_on() {
        crate::serial_println!("[resume] pid={} rip={:#x} rsp={:#x}", pid, rip, rsp);
    }
    validate_user_return_or_panic(
        crate::execution_contract::UserReturnOrigin::Syscall,
        rip,
        rsp,
        rflags,
        fs_base,
        gs_base,
    );

    crate::arch::process::jump_to_userspace_with_registers(
        rip, rsp, rflags, rax, rdi, rsi, rdx, r8, r9, r10, rbx, rbp, r12, r13, r14, r15, fs_base,
        gs_base,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn jump_to_user_with_registers_from_syscall(
    rip: u64,
    rsp: u64,
    rflags: u64,
    rax: u64,
    rdi: u64,
    rsi: u64,
    rdx: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    rbx: u64,
    rbp: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
) -> ! {
    let rflags = sanitize_user_rflags(rflags);
    let (pid, fs_base, gs_base, kstack) = {
        let table = crate::process::table::TABLE.lock();
        let p = table
            .current_ref()
            .expect("jump_to_user_with_registers_from_syscall: no process");
        (
            p.pid,
            p.fs_base.fs_base,
            p.fs_base.gs_base,
            p.kernel_stack_top(),
        )
    };
    if kstack != 0 {
        crate::syscall::set_kernel_stack(kstack);
    }
    crate::execution_contract::ExecutionContract::dump_user_return(
        "jump_to_user_with_registers_from_syscall",
        pid,
        rip,
        rsp,
        rflags,
        fs_base,
        gs_base,
    );
    validate_user_return_or_panic(
        crate::execution_contract::UserReturnOrigin::ForkChild,
        rip,
        rsp,
        rflags,
        fs_base,
        gs_base,
    );
    crate::arch::process::jump_to_userspace_with_registers_from_syscall(
        rip, rsp, rflags, rax, rdi, rsi, rdx, r8, r9, r10, rbx, rbp, r12, r13, r14, r15, fs_base,
        gs_base,
    )
}

fn validate_user_return_or_panic(
    origin: crate::execution_contract::UserReturnOrigin,
    rip: u64,
    rsp: u64,
    rflags: u64,
    fs_base: u64,
    gs_base: u64,
) {
    let context = crate::execution_contract::UserContext {
        rip,
        rsp,
        rflags,
        fs_base,
        gs_base,
    };
    if let Err(reason) =
        crate::execution_contract::ExecutionContract::validate_user_return(origin, &context)
    {
        crate::execution_contract::ExecutionContract::dump_user_return(
            "validate_user_return",
            crate::process::current_pid().unwrap_or(0),
            rip,
            rsp,
            rflags,
            fs_base,
            gs_base,
        );
        panic!("[execution-contract] user return violation: {}", reason);
    }
}

/// Wait for a child process to exit and return its exit code.
/// Uses wait4 syscall internally. Returns the child PID on success, negative error code on error.
pub fn waitpid(pid: u32, status_ptr: u64) -> i64 {
    crate::syscall::handlers::sys_wait4(pid as u64, status_ptr, 0, 0)
}
