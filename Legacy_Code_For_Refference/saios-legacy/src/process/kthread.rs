//! Kernel threads — schedulable execution contexts that run in ring 0 sharing
//! the kernel address space (no CR3 switch).  They reuse the same Process table
//! and round-robin scheduler as user processes, so the existing timer-driven
//! `scheduler::tick()` preempts them.
//!
//! A new kernel thread's kernel stack is crafted so that the very first
//! `switch_context` into it pops 6 zeroed callee-saved slots and `ret`s into
//! `kthread_trampoline` (arch/x86_64/process/context_switch.s), which enables interrupts and calls the
//! entry fn (with %rbx = entry, %r12 = arg pre-loaded from the stack).

use super::table::TABLE;
use alloc::string::String;

/// Spawn a kernel thread that runs `entry`.  Returns its PID/TID.
pub fn spawn(name: &str, entry: extern "C" fn()) -> u32 {
    spawn_opts(name, entry, false)
}

/// Spawn a kernel thread pinned to the boot CPU (for threads that touch
/// singleton hardware not yet serialised for SMP — keyboard, NIC).
pub fn spawn_pinned(name: &str, entry: extern "C" fn()) -> u32 {
    spawn_opts(name, entry, true)
}

fn spawn_opts(name: &str, entry: extern "C" fn(), boot_cpu_affine: bool) -> u32 {
    let trace_shell = name == "shell";
    let mut p = crate::process_contract::ProcessContract::create(
        crate::process_contract::ProcessCreationRequest {
            name: String::from(name),
            parent_pid: 0,
            kind: crate::process_contract::ProcessCreationKind::KernelThread,
            tag: "kthread_spawn",
        },
    );
    let pid = p.pid;
    if trace_shell {
        crate::serial_println!(
            "[shell-spawn] created pid={} boot_cpu_affine={} entry={:#x}",
            pid,
            boot_cpu_affine,
            entry as usize
        );
    }
    crate::process_contract::ProcessContract::prepare_kernel_context(
        &mut p,
        boot_cpu_affine,
        "kthread_context",
    );

    // Craft the initial kernel-stack frame consumed by switch_context:
    //   [rflags][rbp][rbx][r12][r13][r14][r15][ret_addr]
    // switch_context pops rflags (popfq), then rbp,rbx,r12,r13,r14,r15, then
    // `ret`s into ret_addr.  RFLAGS keeps IF clear until the trampoline runs
    // finish_switch(); the trampoline enables interrupts before calling entry.
    // Align so the trampoline's `call` site is 16-byte aligned (SysV ABI).
    let top = p.kernel_stack_top() & !0xF;
    let sp = (top - 8 * 8) as *mut u64; // 8 u64 slots = 64 bytes
    unsafe {
        *sp.add(0) = 0x2; // rflags: reserved bit 1, IF cleared for finish_switch
        *sp.add(1) = 0; // rbp
        *sp.add(2) = entry as usize as u64; // rbx -> entry fn
        *sp.add(3) = 0; // r12 -> arg (unused)
        *sp.add(4) = 0; // r13
        *sp.add(5) = 0; // r14
        *sp.add(6) = 0; // r15
        *sp.add(7) = crate::arch::process::kthread_trampoline_addr(); // ret addr
    }
    p.kernel_rsp = sp as u64;
    if trace_shell {
        crate::serial_println!(
            "[shell-spawn] ready pid={} state={:?} boot_cpu_affine={} allowed={:#x} preferred={:?} numa={:?} stack_top={:#x} kernel_rsp={:#x} entry={:#x}",
            pid,
            p.state,
            p.boot_cpu_affine,
            p.scheduling.allowed_cpus,
            p.scheduling.preferred_cpu,
            p.scheduling.numa_node,
            p.kernel_stack_top(),
            p.kernel_rsp,
            entry as usize
        );
    }

    // Insert with interrupts off so the timer's scheduler can't observe a
    // half-updated table.
    if trace_shell {
        crate::serial_println!("[shell-spawn] admitting pid={}", pid);
    }
    crate::arch::without_interrupts(|| {
        crate::process_contract::ProcessContract::validate_creation_ready_or_panic(
            crate::process_contract::ProcessCreationKind::KernelThread,
            &p,
            "kthread_ready",
        );
        crate::process_contract::ProcessContract::admit_runnable(
            p,
            "kthread_spawn",
            "process::kthread::spawn",
        );
    });
    if trace_shell {
        let queued = TABLE
            .try_lock()
            .map(|table| table.scheduler_snapshot().run_queue.contains(&pid))
            .unwrap_or(false);
        crate::serial_println!("[shell-spawn] admitted pid={}", pid);
        crate::serial_println!("[shell-spawn] queued pid={} queued={}", pid, queued);
    }
    pid
}

#[unsafe(no_mangle)]
pub extern "C" fn kthread_finish_switch_current() {
    super::scheduler::finish_switch();
}

/// Register the currently-running boot/kernel context as a schedulable thread,
/// so the scheduler can switch *away* from it (today PID 0 isn't in the table,
/// so `schedule()` always sees "only one process" and never preempts).  Its
/// `kernel_rsp` is filled in by `switch_context` on the first switch-out, and it
/// runs on the real boot stack (its Process's heap stack is unused).
pub fn register_boot_thread() -> u32 {
    crate::serial_println!("[kthread] boot register: create begin");
    let mut p = crate::process_contract::ProcessContract::create(
        crate::process_contract::ProcessCreationRequest {
            name: String::from("kmain"),
            parent_pid: 0,
            kind: crate::process_contract::ProcessCreationKind::KernelThread,
            tag: "boot_thread",
        },
    );
    let pid = p.pid;
    crate::serial_println!("[kthread] boot register: prepare begin pid={}", pid);
    crate::process_contract::ProcessContract::prepare_kernel_context(&mut p, true, "boot_context");
    crate::serial_println!("[kthread] boot register: admit begin pid={}", pid);
    crate::arch::without_interrupts(|| {
        crate::serial_println!("[kthread] boot register: validate ready begin pid={}", pid);
        crate::process_contract::ProcessContract::validate_creation_ready_or_panic(
            crate::process_contract::ProcessCreationKind::KernelThread,
            &p,
            "boot_thread_ready",
        );
        crate::serial_println!("[kthread] boot register: contract admit begin pid={}", pid);
        crate::process_contract::ProcessContract::admit_running_current(p, 0, true, "boot_thread");
        crate::serial_println!(
            "[kthread] boot register: contract admit complete pid={}",
            pid
        );
    });
    crate::serial_println!("[kthread] boot register: complete pid={}", pid);
    pid
}

/// Called from `kthread_trampoline` when a kernel thread's entry fn returns.
/// Removes the thread from the run queue and yields forever.
#[unsafe(no_mangle)]
pub extern "C" fn kthread_exit_current() -> ! {
    crate::arch::without_interrupts(|| {
        let mut t = TABLE.lock();
        let cur = t.current_pid();
        crate::scheduler_contract::SchedulerContract::remove_from_run_queue(
            &mut t,
            cur,
            "kthread_exit_current",
        );
    });
    loop {
        super::scheduler::yield_now();
        crate::arch::halt();
    }
}
