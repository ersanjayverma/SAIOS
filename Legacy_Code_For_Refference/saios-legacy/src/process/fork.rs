//! fork() — creates a child process that is an exact copy of the parent.
//!
//! # Phase 8: per-process address spaces
//! The child is given its own PML4 (sharing the kernel via PML4[0]). Writable
//! user pages are shared copy-on-write, while immutable pages are mapped into
//! the child directly. A write fault allocates a private replacement page.

use super::table::TABLE;
use super::{KERNEL_STACK_SIZE, Process, ProcessState};
use alloc::string::String;
use core::sync::atomic::{AtomicU32, Ordering};

static EXECVE_FORK_CHILD_TRACE_PID: AtomicU32 = AtomicU32::new(0);

pub fn take_execve_child_first_syscall_trace(pid: u32) -> bool {
    EXECVE_FORK_CHILD_TRACE_PID
        .compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// Fork the current process. Returns child PID to caller (parent path).
/// The child is added to the run queue and will return 0 from its own fork syscall.
#[allow(clippy::too_many_arguments)]
pub fn do_fork(
    // Saved user registers at the syscall site — we need to set up the child
    // so that when it's scheduled it returns 0 from syscall.
    user_rip: u64,
    user_rsp: u64,
    user_rflags: u64,
    user_rdi: u64,
    user_rsi: u64,
    user_rdx: u64,
    user_r8: u64,
    user_r9: u64,
    user_r10: u64,
    user_rbx: u64,
    user_rbp: u64,
    user_r12: u64,
    user_r13: u64,
    user_r14: u64,
    user_r15: u64,
) -> i64 {
    // Read ALL parent fields under a SINGLE lock acquisition.  Taking the lock
    // multiple times would let the scheduler switch the "current" process
    // between reads, mixing fields from two different processes into the child.
    let (parent_pid, parent_pml4, name) = {
        let table = TABLE.lock();
        match table.current_ref() {
            Some(p) => (p.pid, p.address_space_pml4(), String::from(p.name.as_str())),
            None => return -1, // no current process
        }
    };

    // Build the child PCB
    let mut child = crate::process_contract::ProcessContract::create(
        crate::process_contract::ProcessCreationRequest {
            name,
            parent_pid,
            kind: crate::process_contract::ProcessCreationKind::ForkChild,
            tag: "fork_child",
        },
    );
    let child_pid = child.pid;
    {
        let table = TABLE.lock();
        let Some(parent) = table.procs.get(&parent_pid) else {
            return -1;
        };
        crate::process_contract::ProcessContract::inherit_parent_metadata(
            &mut child,
            parent,
            true,
            false,
            "fork_inherit",
        );
    }

    // Give the child its own address space and clone the parent's user pages
    // copy-on-write so fork does not eagerly copy every page up front.
    if let Err(e) = clone_address_space(parent_pml4, &mut child) {
        crate::println!("[fork] address-space clone failed: {}", e);
        child.destroy_address_space();
        return -1;
    }

    crate::process_contract::ProcessContract::finalize_fork_register_image(
        &mut child,
        crate::process_contract::ForkRegisterImage {
            rip: user_rip,
            rsp: user_rsp,
            rflags: user_rflags,
            rdi: user_rdi,
            rsi: user_rsi,
            rdx: user_rdx,
            r8: user_r8,
            r9: user_r9,
            r10: user_r10,
            rbx: user_rbx,
            rbp: user_rbp,
            r12: user_r12,
            r13: user_r13,
            r14: user_r14,
            r15: user_r15,
        },
        "fork_register_image",
    );

    if let Err(e) = make_child_return_stack_writable(&child) {
        crate::println!("[fork] child return-stack COW split failed: {}", e);
        child.destroy_address_space();
        return -1;
    }

    if crate::diag::diag_proc_on() {
        crate::serial_println!(
            "[fork] parent trap rip={:#x} rsp={:#x} rax=child({}) rdi={:#x} rsi={:#x} rdx={:#x} r8={:#x} r9={:#x} r10={:#x} rbx={:#x} rbp={:#x} r12={:#x} r13={:#x} r14={:#x} r15={:#x} cr3={:#x}",
            user_rip,
            user_rsp,
            child_pid,
            user_rdi,
            user_rsi,
            user_rdx,
            user_r8,
            user_r9,
            user_r10,
            user_rbx,
            user_rbp,
            user_r12,
            user_r13,
            user_r14,
            user_r15,
            parent_pml4,
        );
        crate::serial_println!(
            "[fork] child trap  rip={:#x} rsp={:#x} rax={} rdi={:#x} rsi={:#x} rdx={:#x} r8={:#x} r9={:#x} r10={:#x} rbx={:#x} rbp={:#x} r12={:#x} r13={:#x} r14={:#x} r15={:#x} cr3={:#x}",
            child.rip,
            child.rsp,
            child.fork_rax,
            child.fork_rdi,
            child.fork_rsi,
            child.fork_rdx,
            child.fork_r8,
            child.fork_r9,
            child.fork_r10,
            child.fork_rbx,
            child.fork_rbp,
            child.fork_r12,
            child.fork_r13,
            child.fork_r14,
            child.fork_r15,
            child.address_space_pml4(),
        );
    }

    // Point child's kernel stack RSP to a fresh trampoline that will
    // sysretq back to user space with RAX=0
    let kstack_top = child.kernel_stack_top();
    setup_fork_return_frame(kstack_top, &mut child);
    if child.name == "execve_driver" {
        EXECVE_FORK_CHILD_TRACE_PID.store(child_pid, Ordering::SeqCst);
        crate::serial_println!(
            "[execve-fork-child] pid={} parent={} rip={:#x} rsp={:#x} kstack_top={:#x} kernel_rsp={:#x} pml4={:#x} gs_active={}",
            child_pid,
            parent_pid,
            child.rip,
            child.rsp,
            kstack_top,
            child.kernel_rsp,
            child.address_space_pml4(),
            crate::arch::syscall::kernel_gs_active()
        );
    }

    crate::process_contract::ProcessContract::validate_fork_child_or_panic(
        parent_pid,
        &child,
        "fork_child_validate",
    );
    crate::process_contract::ProcessContract::validate_creation_ready_or_panic(
        crate::process_contract::ProcessCreationKind::ForkChild,
        &child,
        "fork_child_ready",
    );

    crate::process_contract::ProcessContract::admit_runnable(
        child,
        "fork_child",
        "process::fork::do_fork",
    );
    crate::observability_contract::ObservabilityContract::kds_event_for(
        crate::kds::KdsSubsystem::Process,
        crate::kds::KdsEventType::Fork,
        crate::kds::KdsSeverity::Info,
        parent_pid,
        parent_pid,
        [parent_pid as u64, child_pid as u64, user_rip, user_rsp],
    );

    // Restore parent's kernel stack pointer
    if let Some(p) = TABLE.lock().current_ref() {
        crate::syscall::set_kernel_stack(p.kernel_stack_top());
    }

    child_pid as i64 // parent returns child PID
}

/// Give `child` its own PML4 and clone the parent's user pages into it using
/// copy-on-write for writable mappings. Walks the page tables directly because
/// the user region is far too large to scan linearly by virtual address.
fn clone_address_space(parent_pml4: u64, child: &mut Process) -> Result<(), &'static str> {
    let src_pml4 = if parent_pml4 != 0 {
        parent_pml4
    } else {
        crate::memory::paging::active_pml4()
    };
    let child_pml4 = child.ensure_address_space()?;
    crate::memory_contract::MemoryContract::fork_cow(
        crate::address_space_contract::AddressSpaceHandle {
            id: src_pml4,
            pml4: src_pml4,
            owner_pid: child.parent_pid,
        },
        crate::address_space_contract::AddressSpaceHandle {
            id: child_pml4,
            pml4: child_pml4,
            owner_pid: child.pid,
        },
    )
}

fn make_child_return_stack_writable(child: &Process) -> Result<(), &'static str> {
    let pml4 = child.address_space_pml4();
    if pml4 == 0 || child.rsp == 0 {
        return Ok(());
    }

    let pages = [
        child.rsp & !0xFFF,
        child.rsp.wrapping_sub(8) & !0xFFF,
        child.rsp.wrapping_sub(128) & !0xFFF,
    ];
    let mut last_page = u64::MAX;
    for page in pages {
        if page == last_page {
            continue;
        }
        last_page = page;
        let Some((_, flags)) = crate::memory::paging::translate_entry_in(pml4, page) else {
            continue;
        };
        if flags & crate::memory::paging::PTE_COW != 0 {
            crate::memory_contract::MemoryContract::resolve_cow_fault(
                crate::address_space_contract::AddressSpaceHandle {
                    id: pml4,
                    pml4,
                    owner_pid: child.pid,
                },
                page,
            )?;
        }
    }
    Ok(())
}

/// Push a minimal SYSRETQ frame on the child's kernel stack so the scheduler
/// can jump to it and it will sysretq to user space with RAX=0.
fn setup_fork_return_frame(kstack_top: u64, child: &mut Process) {
    let top = kstack_top & !0xF;
    let sp = (top - 8 * 8) as *mut u64;
    unsafe {
        *sp.add(0) = 0x2;
        *sp.add(1) = 0;
        *sp.add(2) = 0;
        *sp.add(3) = 0;
        *sp.add(4) = 0;
        *sp.add(5) = 0;
        *sp.add(6) = 0;
        *sp.add(7) = fork_child_trampoline as *const () as usize as u64;
    }
    child.kernel_rsp = sp as u64;
}

extern "C" fn fork_child_trampoline() -> ! {
    crate::process::scheduler::finish_switch();
    let _ = crate::process::refresh_current_from_table();

    let (
        pid,
        rip,
        rsp,
        rflags,
        fork_rax,
        fork_rdi,
        fork_rsi,
        fork_rdx,
        fork_r8,
        fork_r9,
        fork_r10,
        fork_rbx,
        fork_rbp,
        fork_r12,
        fork_r13,
        fork_r14,
        fork_r15,
        pml4,
    ) = {
        let table = TABLE.lock();
        let proc = table
            .current_ref()
            .expect("fork_child_trampoline: no current child process");
        (
            proc.pid,
            proc.rip,
            proc.rsp,
            proc.rflags,
            proc.fork_rax,
            proc.fork_rdi,
            proc.fork_rsi,
            proc.fork_rdx,
            proc.fork_r8,
            proc.fork_r9,
            proc.fork_r10,
            proc.fork_rbx,
            proc.fork_rbp,
            proc.fork_r12,
            proc.fork_r13,
            proc.fork_r14,
            proc.fork_r15,
            proc.address_space_pml4(),
        )
    };

    crate::execution_contract::ExecutionContract::activate_process_address_space(
        pid,
        pml4,
        crate::execution_contract::ExecutionTransition::Fork,
        "fork_child_trampoline",
    );
    if pid == EXECVE_FORK_CHILD_TRACE_PID.load(Ordering::SeqCst) {
        crate::serial_println!(
            "[execve-fork-return] pid={} path=unified rip={:#x} rsp={:#x} pml4={:#x} gs_active={}",
            pid,
            rip,
            rsp,
            pml4,
            crate::arch::syscall::kernel_gs_active()
        );
    }
    if crate::arch::syscall::kernel_gs_active() {
        crate::process::jump_to_user_with_registers_from_syscall(
            rip, rsp, rflags, fork_rax, fork_rdi, fork_rsi, fork_rdx, fork_r8, fork_r9, fork_r10,
            fork_rbx, fork_rbp, fork_r12, fork_r13, fork_r14, fork_r15,
        )
    } else {
        crate::process::jump_to_user_with_registers(
            rip, rsp, rflags, fork_rax, fork_rdi, fork_rsi, fork_rdx, fork_r8, fork_r9, fork_r10,
            fork_rbx, fork_rbp, fork_r12, fork_r13, fork_r14, fork_r15,
        )
    }
}
