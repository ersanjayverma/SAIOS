//! Threading support — clone(CLONE_THREAD) for SAIOS user threads.
//!
//! A thread shares the address space, file descriptor table, and signal
//! handlers with its parent process. Each thread has its own:
//!   - Kernel stack
//!   - Saved registers (TLS via FS.base, RSP, RIP)
//!   - TID (thread ID) — distinct from PID for the first thread

use super::table::TABLE;
use super::{KERNEL_STACK_SIZE, ProcessState};
use crate::dynlink;
use crate::ipc::futex;
use crate::vfs::file::FdTable;
use alloc::string::String;

// clone() flags
pub const CLONE_VM: u64 = 0x00000100;
pub const CLONE_FS: u64 = 0x00000200;
pub const CLONE_FILES: u64 = 0x00000400;
pub const CLONE_SIGHAND: u64 = 0x00000800;
pub const CLONE_THREAD: u64 = 0x00010000;
pub const CLONE_SETTLS: u64 = 0x00080000;
pub const CLONE_PARENT_SETTID: u64 = 0x00100000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x00200000;
pub const CLONE_CHILD_SETTID: u64 = 0x01000000;

/// Implement clone() for thread creation (CLONE_VM | CLONE_THREAD | CLONE_SIGHAND).
pub fn do_clone(
    flags: u64,
    child_stack: u64,
    parent_tid: u64,
    child_tid_ptr: u64,
    tls: u64,
    // Saved user registers at clone() call site
    user_rip: u64,
    user_rflags: u64,
) -> i64 {
    let is_thread = flags & CLONE_THREAD != 0;

    let parent_pid = TABLE.lock().current_pid();

    let name = {
        let table = TABLE.lock();
        table
            .procs
            .get(&parent_pid)
            .map(|p| alloc::format!("{}:thread", p.name))
            .unwrap_or_else(|| String::from("thread"))
    };

    let mut child = crate::process_contract::ProcessContract::create(
        crate::process_contract::ProcessCreationRequest {
            name,
            parent_pid,
            kind: crate::process_contract::ProcessCreationKind::UserThread,
            tag: "clone_child",
        },
    );
    let tid = child.pid;
    child.parent_pid = parent_pid;

    {
        let table = TABLE.lock();
        if let Some(p) = table.procs.get(&parent_pid) {
            crate::process_contract::ProcessContract::inherit_parent_metadata(
                &mut child,
                p,
                flags & CLONE_FILES != 0,
                true,
                "clone_inherit",
            );
        }
    }

    let child_rsp = if child_stack != 0 {
        child_stack
    } else {
        TABLE
            .lock()
            .procs
            .get(&parent_pid)
            .map(|p| p.rsp)
            .unwrap_or(0)
    };

    let tls_base = if flags & CLONE_SETTLS != 0 && tls != 0 {
        Some(tls)
    } else {
        None
    };
    crate::process_contract::ProcessContract::finalize_user_thread_context(
        &mut child,
        user_rip,
        child_rsp,
        user_rflags,
        tls_base,
        "clone_context",
    );

    // Set up TLS if requested (CLONE_SETTLS for pthread_create)
    if flags & CLONE_SETTLS != 0 && tls != 0 {
        // Set FS.base via MSR for the new thread
        unsafe {
            crate::arch::process::set_fs_base(tls);
        }
    }

    // Write TIDs
    if flags & CLONE_PARENT_SETTID != 0 && parent_tid != 0 {
        unsafe {
            core::ptr::write_volatile(parent_tid as *mut u32, tid);
        }
    }
    // Store child_tid_ptr for CLONE_CHILD_CLEARTID
    child.clear_child_tid = child_tid_ptr;

    if flags & CLONE_CHILD_SETTID != 0 && child_tid_ptr != 0 {
        // Will be written when child first runs
        child.fork_r12 = child_tid_ptr; // store for later
    }

    let kstack_top = child.kernel_stack_top();
    crate::syscall::set_kernel_stack(kstack_top);

    crate::println!("[thread] new thread tid={} stack={:#x}", tid, child.rsp);
    crate::process_contract::ProcessContract::validate_creation_ready_or_panic(
        crate::process_contract::ProcessCreationKind::UserThread,
        &child,
        "clone_child_ready",
    );
    crate::process_contract::ProcessContract::admit_runnable(
        child,
        "clone_child",
        "process::thread::sys_clone",
    );

    // Restore parent's kernel stack
    if let Some(p) = TABLE.lock().procs.get(&parent_pid) {
        crate::syscall::set_kernel_stack(p.kernel_stack_top());
    }

    tid as i64 // parent gets child TID
}

/// Called when a thread exits — clean up thread resources.
pub fn thread_exit(tid: u32, exit_code: i64) {
    // For CLONE_CHILD_CLEARTID: write 0 to clear_child_tid and futex-wake
    let clear_child_tid = {
        let table = TABLE.lock();
        table.procs.get(&tid).map(|p| p.clear_child_tid)
    };

    if let Some(clear_tid_addr) = clear_child_tid
        && clear_tid_addr != 0
    {
        // Write 0 to clear_child_tid
        unsafe {
            core::ptr::write_volatile(clear_tid_addr as *mut u32, 0);
        }
        // Wake any futex waiters
        futex::futex_wake_all(clear_tid_addr);
    }

    let was_waiter_woken = crate::process_contract::ProcessContract::request_exit(
        crate::process_contract::ProcessExitRequest {
            pid: tid,
            code: exit_code,
            reason: crate::process_contract::ProcessExitReason::ThreadExit,
            tag: "thread_exit",
        },
    )
    .is_some_and(|disposition| disposition.woke_waiters != 0);

    // Scheduler-driven exit: use the non-returning handoff so the exiting stack
    // is never saved as a resumable context.
    if was_waiter_woken {
        crate::process::scheduler::schedule_handoff_from("thread_exit_woke_waiter");
    } else {
        crate::process::scheduler::schedule_handoff_from("thread_exit");
    }
}
