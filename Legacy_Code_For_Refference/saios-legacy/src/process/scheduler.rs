//! Preemptive, per-CPU round-robin scheduler (SMP-capable).
//!
//! Each CPU has its own `current` thread and an idle thread to fall back on.
//! The single shared run queue holds only runnable threads that are NOT
//! currently on any CPU — and crucially, a thread is returned to the queue only
//! AFTER the CPU that switched away from it has saved its stack pointer
//! (`on_cpu` handshake in `finish_switch`).  That invariant is what makes it
//! safe for another core to pick the thread without ever resuming it on a stale
//! stack — the classic SMP context-switch race.
//!
//! Timer preemption: each CPU's timer path calls tick() and schedule() for the
//! calling CPU.

use super::ProcessState;
use super::table::{MAX_CPUS, ProcessTable, TABLE, cpu_idx};
use alloc::format;
use alloc::string::ToString;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicU64, Ordering};

/// Scheduler trace logging flag. When set, logs every context switch.
/// Default: OFF (0) - normal boot has no scheduler spam.
/// Enable with: `diag sched on`
const SCHED_TRACE_BIT: u32 = 1 << 2;

pub fn sched_trace_on() -> bool {
    crate::diag::DIAG_FLAGS.load(core::sync::atomic::Ordering::Relaxed) & SCHED_TRACE_BIT != 0
}

const TIME_SLICE_TICKS: u64 = 15;
static TICKS_REMAINING: [spin::Mutex<u64>; MAX_CPUS] =
    [const { spin::Mutex::new(TIME_SLICE_TICKS) }; MAX_CPUS];
static CPU_ACCOUNT_LAST_NS: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
/// Per-CPU contention counter: how many times try_lock() failed for this CPU.
static SCHEDULE_CONTENTION: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

pub fn stale_on_cpu_repair_count() -> u64 {
    crate::scheduler_contract::SchedulerContract::stale_on_cpu_repair_count()
}

/// Called from a CPU-local timer IRQ; may trigger a context switch on that CPU.
pub fn tick() {
    let mut remaining = TICKS_REMAINING[cpu_idx()].lock();
    if *remaining > 0 {
        *remaining -= 1;
        return;
    }
    *remaining = TIME_SLICE_TICKS;
    drop(remaining);
    schedule_from("timer_tick");
}

/// Yield the CPU to the next runnable thread.
pub fn yield_now() {
    schedule_from("yield_now");
}

/// Yield from thread context where losing the reschedule to TABLE contention is
/// not acceptable. Do not use from IRQ/timer paths.
pub fn yield_now_wait(reason: &'static str) {
    schedule_from_inner(reason, true);
}

/// Pick the next thread for the calling CPU and switch to it.
pub fn schedule() {
    schedule_from("direct");
}

/// Pick the next thread for the calling CPU and switch to it.
pub fn schedule_from(reason: &'static str) {
    schedule_from_inner(reason, false);
}

/// Schedule from thread context after the caller has made itself non-runnable.
/// Unlike timer IRQ scheduling, this must not skip the handoff on transient
/// ProcessTable contention.
pub fn schedule_blocking_from(reason: &'static str) {
    schedule_from_inner(reason, true);
}

/// Pick the next process for non-returning exit/fault handoff paths without
/// saving the current stack as a resumable context.
pub fn schedule_handoff_from(reason: &'static str) -> ! {
    schedule_handoff_no_save_from(reason);
}

fn publish_exiting_current_before_pick(reason: &'static str) -> (u32, Option<ProcessState>) {
    let (prev, prev_state, publication) = {
        let mut t = crate::process::table::table_lock();
        let c = cpu_idx();
        let prev = t.current[c];
        let prev_state = t.procs.get(&prev).map(|proc| proc.state.clone());
        crate::serial_println!(
            "[schedule-handoff] enter reason={} cpu={} prev={} state={:?}",
            reason,
            c,
            prev,
            prev_state
        );
        crate::scheduler_contract::SchedulerContract::recover_stale_on_cpu_ownership(
            &mut t,
            "schedule-handoff-entry",
        );

        let publication = if matches!(prev_state, Some(ProcessState::Zombie)) {
            t.current[c] = 0;
            t.prev[c] = prev;
            t.clear_contract_cpu_owner(prev);
            t.remove_from_run_queue(prev);
            let pub_result = crate::process_contract::ProcessContract::publish_zombie_after_switch(
                &mut t,
                prev,
                "schedule no-save exit publication",
            );
            crate::serial_println!("[zombie] publish-return is_some={}", pub_result.is_some());
            pub_result
        } else {
            None
        };
        drop(t);
        crate::process::table::table_unlock();
        crate::serial_println!("[zombie] process-table-unlocked");
        (prev, prev_state, publication)
    };

    if let Some(publication) = publication {
        crate::serial_println!("[zombie] notify-dispatch parent={} child={}",
            publication.parent_pid, publication.pid);
        let disposition =
            crate::process_contract::ProcessContract::notify_zombie_publication(publication);
        if disposition.woke_waiters == 0 && crate::diag::diag_proc_on() {
            crate::serial_println!(
                "[schedule] exit publication woke no waiters pid={} parent={} reason={}",
                disposition.pid,
                disposition.parent_pid,
                reason
            );
        }
    }

    (prev, prev_state)
}

/// Pick the next process from a fatal interrupt/fault path without saving the
/// current IDT stack as a resumable kernel context.
pub fn schedule_handoff_no_save_from(reason: &'static str) -> ! {
    crate::arch::without_interrupts(|| {
        let (handoff_prev, handoff_prev_state) = publish_exiting_current_before_pick(reason);
        let (to, prev, idle, cpu) = {
            let mut t = crate::process::table::table_lock();
            let c = cpu_idx();
            let current = t.current[c];
            let prev = if handoff_prev != 0 {
                handoff_prev
            } else {
                current
            };
            let prev_state = if handoff_prev != 0 {
                handoff_prev_state.clone()
            } else {
                t.procs.get(&current).map(|proc| proc.state.clone())
            };
            crate::scheduler_contract::SchedulerContract::recover_stale_on_cpu_ownership(
                &mut t,
                "schedule-handoff-pick",
            );

            let next = match crate::scheduler_contract::SchedulerContract::pick_next(
                &mut t,
                "schedule no-save pick",
            ) {
                Some(p) => p,
                None => {
                    let idle = t.idle[c];
                    if crate::diag::diag_sched_on()
                        || matches!(prev_state, Some(ProcessState::Zombie))
                    {
                        crate::serial_println!(
                            "[schedule] no runnable tasks found prev={} state={:?} idle={} reason={}",
                            prev,
                            prev_state,
                            idle,
                            reason
                        );
                    }
                    if idle == 0 || idle == current {
                        crate::hlt_loop();
                    }
                    idle
                }
            };

            if next == current {
                crate::hlt_loop();
            }

            crate::scheduler_contract::SchedulerContract::claim_next_on_cpu(
                &mut t,
                c,
                next,
                current,
                "schedule no-save claim",
            );

            if t.procs
                .get(&next)
                .is_some_and(|p| p.state == ProcessState::Blocked)
            {
                crate::process_contract::ProcessContract::transition_existing_state(
                    &mut t,
                    next,
                    ProcessState::Ready,
                    "schedule_handoff dispatch unblock",
                );
            }
            crate::process_contract::ProcessContract::transition_existing_state(
                &mut t,
                next,
                ProcessState::Running,
                "schedule_handoff dispatch",
            );
            crate::diag::watchdog::note_scheduler_progress(c, next);
            crate::OBS_COUNTER!(
                crate::kds::KdsSubsystem::Scheduler,
                crate::kds::KdsMetricId::ContextSwitches,
                1,
            );

            let (kstack, kernel_rsp, pml4) = t
                .procs
                .get(&next)
                .map(|p| (p.kernel_stack_top(), p.kernel_rsp, p.address_space_pml4()))
                .unwrap_or((0, 0, 0));
            crate::execution_contract::ExecutionContract::install_scheduled_process(
                crate::execution_contract::ExecutionTransition::Schedule,
                next,
                kstack,
                kernel_rsp,
                pml4,
                "schedule no-save install",
            );

            let to = t
                .procs
                .get(&next)
                .map(|p| &p.kernel_rsp as *const u64)
                .unwrap_or_default();
            let idle_pid = t.idle[c];
            drop(t);
            crate::process::table::table_unlock();
            (to, prev, idle_pid, c)
        };

        if to.is_null() {
            crate::hlt_loop();
        }
        account_cpu_time(cpu, prev, idle);
        if !crate::arch::syscall::kernel_gs_active() {
            unsafe {
                crate::arch::process::swapgs();
            }
            crate::arch::syscall::mark_kernel_gs_active(true);
        }
        unsafe {
            crate::arch::process::switch_context_nosave(to);
        }
    });
    crate::hlt_loop();
}

fn schedule_from_inner(reason: &'static str, wait_for_table: bool) {
    // Disable interrupts for the whole decision + switch: a nested timer IRQ
    // mid-switch would re-enter and corrupt the stack swap.
    crate::arch::without_interrupts(|| {
        let (from, to, next, prev, idle, cpu) = {
            let mut t = if wait_for_table {
                crate::process::table::table_lock()
            } else {
                // try_lock (never block): schedule() runs from IRQ context, and if
                // another CPU / this CPU's mainline holds TABLE we just skip this tick.
                let Some(t) = crate::process::table::table_try_lock() else {
                    let cpu = cpu_idx();
                    let count = SCHEDULE_CONTENTION[cpu].fetch_add(1, Ordering::Relaxed) + 1;
                    // Emit ProgressContract signal every 1000 contentions — sustained
                    // contention means the scheduler cannot make forward progress.
                    if count.is_multiple_of(1000) {
                        crate::kds::kds_event(
                            crate::kds::KdsSubsystem::Scheduler,
                            crate::kds::KdsEventType::SchedulerStall,
                            crate::kds::KdsSeverity::Warn,
                            [cpu as u64, count, 0, 0],
                        );
                    }
                    return;
                };
                t
            };
            let c = cpu_idx();
            let prev = t.current[c];
            let prev_state = t.procs.get(&prev).map(|proc| proc.state.clone());
            crate::scheduler_contract::SchedulerContract::recover_stale_on_cpu_ownership(
                &mut t,
                "schedule-entry",
            );
            if matches!(prev_state, Some(ProcessState::Running)) || prev == 0 {
                crate::scheduler_contract::SchedulerContract::validate_table_or_panic(
                    &t,
                    "schedule entry",
                );
            }

            // Choose the next thread; fall back to this CPU's idle thread.
            let next = match crate::scheduler_contract::SchedulerContract::pick_next(
                &mut t,
                "schedule pick",
            ) {
                Some(p) => p,
                None => {
                    let idle = t.idle[c];
                    if crate::diag::diag_sched_on()
                        || matches!(prev_state, Some(ProcessState::Zombie))
                    {
                        crate::serial_println!(
                            "[schedule] no runnable tasks found prev={} state={:?} idle={}",
                            prev,
                            prev_state,
                            idle
                        );
                    }
                    if idle == 0 || idle == prev {
                        drop(t);
                        crate::process::table::table_unlock();
                        return;
                    } // nothing to do — keep running
                    idle
                }
            };
            if next == prev {
                drop(t);
                crate::process::table::table_unlock();
                return;
            }

            // Claim `next` for this CPU before releasing the lock.
            crate::scheduler_contract::SchedulerContract::claim_next_on_cpu(
                &mut t,
                c,
                next,
                prev,
                "schedule claim",
            );

            if sched_trace_on() {
                crate::serial_println!("[sched] prev={} next={}", prev, next);
                if let Some(p) = t.procs.get(&next) {
                    crate::serial_println!("[sched]   next state: {:?}", p.state);
                }
            }

            if t.procs
                .get(&next)
                .is_some_and(|p| p.state == ProcessState::Blocked)
            {
                crate::process_contract::ProcessContract::transition_existing_state(
                    &mut t,
                    next,
                    ProcessState::Ready,
                    "schedule dispatch unblock",
                );
            }
            crate::process_contract::ProcessContract::transition_existing_state(
                &mut t,
                next,
                ProcessState::Running,
                "schedule dispatch",
            );
            crate::diag::watchdog::note_scheduler_progress(c, next);
            crate::OBS_COUNTER!(
                crate::kds::KdsSubsystem::Scheduler,
                crate::kds::KdsMetricId::ContextSwitches,
                1,
            );

            // Kernel-entry stack and CR3 are per CPU and must follow the
            // scheduled process on every core that can run a user address space.
            let (kstack, kernel_rsp, pml4) = t
                .procs
                .get(&next)
                .map(|p| (p.kernel_stack_top(), p.kernel_rsp, p.address_space_pml4()))
                .unwrap_or((0, 0, 0));
            crate::execution_contract::ExecutionContract::install_scheduled_process(
                crate::execution_contract::ExecutionTransition::Schedule,
                next,
                kstack,
                kernel_rsp,
                pml4,
                "schedule install",
            );

            // [sched] pid A -> pid B (gated by `diag sched on`).  Snapshot the
            // names while we still hold the lock; the format call is cheap and
            // the print itself is gated.  We have to call this *before* moving
            // ownership of `prev`/`next` into the from/to pointers below.
            let from_name = t
                .procs
                .get(&prev)
                .map(|p| p.name.as_str())
                .unwrap_or("<none>");
            let to_name = t
                .procs
                .get(&next)
                .map(|p| p.name.as_str())
                .unwrap_or("<none>");
            if crate::diag::diag_sched_on() {
                let from_state = t
                    .procs
                    .get(&prev)
                    .map(|p| format!("{:?}", p.state))
                    .unwrap_or_else(|| "<none>".to_string());
                let to_state = t
                    .procs
                    .get(&next)
                    .map(|p| format!("{:?}", p.state))
                    .unwrap_or_else(|| "<none>".to_string());
                crate::serial_println!(
                    "[sched] cpu{} pid {} ({}) -> pid {} ({})",
                    c,
                    prev,
                    from_name,
                    next,
                    to_name
                );
                crate::serial_println!("[sched]   state: {} -> {}", from_state, to_state);
            }

            let from = t
                .procs
                .get_mut(&prev)
                .map(|p| &mut p.kernel_rsp as *mut u64)
                .unwrap_or(null_mut());
            let to = t
                .procs
                .get(&next)
                .map(|p| &p.kernel_rsp as *const u64)
                .unwrap_or(null());
            let idle_pid = t.idle[c];
            drop(t);
            crate::process::table::table_unlock();
            (from, to, next, prev, idle_pid, c)
        };

        if from.is_null() || to.is_null() {
            if crate::diag::diag_sched_on() {
                crate::serial_println!(
                    "[schedule] return_null_context reason={} cpu={} prev={} next={} from_null={} to_null={}",
                    reason,
                    cpu,
                    prev,
                    next,
                    from.is_null(),
                    to.is_null()
                );
            }
            return;
        }
        let _ = next;
        account_cpu_time(cpu, prev, idle);
        // Switch — returns here only when THIS thread is scheduled again.
        unsafe {
            crate::arch::process::switch_context(from, to);
        }
        finish_switch();
    });
}

fn account_cpu_time(cpu: usize, prev: u32, idle: u32) {
    let now = crate::time::uptime_ns();
    let last = CPU_ACCOUNT_LAST_NS[cpu.min(MAX_CPUS - 1)].swap(now, Ordering::AcqRel);
    if last == 0 || now <= last {
        return;
    }
    let elapsed = now - last;
    // F-SCHED-11: accumulate TSC-based per-process CPU time on context switch.
    if prev != 0
        && prev != idle
        && let Some(mut table) = crate::process::table::TABLE.try_lock()
        && let Some(proc) = table.procs.get_mut(&prev)
    {
        proc.cpu_ns = proc.cpu_ns.wrapping_add(elapsed);
    }
    let accountable = if prev != 0 && prev != idle {
        crate::resource_contract::AccountableEntity::process(prev)
    } else {
        crate::resource_contract::AccountableEntity::KERNEL
    };
    let _ = crate::resource_contract::ResourceContract::charge(
        crate::resource_contract::AttributionChain {
            accountable,
            acting_pid: if prev != 0 { Some(prev) } else { None },
            correlation_id:
                crate::observability_contract::ObservabilityContract::current_correlation_id(),
            evidence_event_id: 0,
        },
        crate::resource_contract::ResourceKind::CpuTimeNs,
        elapsed,
    );
}

/// Runs as the just-resumed thread: release the thread this CPU switched away
/// from back to the run queue, now that its stack pointer has been saved.
pub(crate) fn finish_switch() {
    let prev = {
        let t = TABLE.lock();
        t.prev[cpu_idx()]
    };
    finish_switch_for(prev);
    // F-EXEC-02: Post-switch TSS/RSP0 consistency check.
    assert_tss_consistent();
}

/// Verify TSS.RSP0 matches the current process's kernel stack after a switch.
#[inline]
fn assert_tss_consistent() {
    let rsp0 = crate::gdt::current_rsp0();
    if rsp0 == 0 {
        return; // No user process active yet (boot/idle)
    }
    if let Some(expected) = crate::process::with_current_process(|proc| proc.kernel_stack_top())
        && expected != 0
        && rsp0 != expected
    {
        crate::serial_println!(
            "[exec-contract] TSS.RSP0 mismatch: tss={:#x} expected={:#x} cpu={}",
            rsp0,
            expected,
            cpu_idx()
        );
    }
}

fn finish_switch_for(prev: u32) {
    let mut publish_exit: Option<crate::process_contract::ZombiePublication> = None;
    let mut t = TABLE.lock();
    let c = cpu_idx();
    if prev == 0 {
        crate::scheduler_contract::SchedulerContract::validate_table_or_panic(
            &t,
            "finish_switch prev=0",
        );
        return;
    }
    if t.current[c] == prev {
        t.prev[c] = 0;
        crate::scheduler_contract::SchedulerContract::validate_table_or_panic(
            &t,
            "finish_switch same-current",
        );
        return;
    }
    if prev == t.idle[c] {
        crate::scheduler_contract::SchedulerContract::release_cpu_owner(
            &mut t,
            prev,
            "finish_switch idle release",
        );
        crate::process_contract::ProcessContract::transition_existing_state(
            &mut t,
            prev,
            ProcessState::Ready,
            "finish_switch idle ready",
        );
        t.prev[c] = 0;
        crate::scheduler_contract::SchedulerContract::validate_table_or_panic(
            &t,
            "finish_switch idle cleanup",
        );
        return;
    } // idle threads never queue

    // --- CPU ownership release (scheduler concern) ---
    let state = t.procs.get(&prev).map(|p| p.state.clone());
    let is_zombie_or_dead = matches!(state, Some(ProcessState::Zombie | ProcessState::Dead));
    let requeue = matches!(state, Some(ProcessState::Running | ProcessState::Ready));

    if crate::diag::diag_sched_on() {
        let name = t
            .procs
            .get(&prev)
            .map(|p| p.name.as_str())
            .unwrap_or("<none>");
        let state_str = state
            .as_ref()
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "<removed>".to_string());
        crate::serial_println!(
            "[sched] finish_switch pid={} name='{}' state={} requeue={} skip={}",
            prev,
            name,
            state_str,
            requeue,
            is_zombie_or_dead
        );
        crate::serial_println!(
            "[sched]   queue_len={} proc_count={}",
            t.run_queue.len(),
            t.procs.len()
        );
        t.dump_queue("finish_switch before requeue");
    }

    if !is_zombie_or_dead {
        if requeue {
            crate::process_contract::ProcessContract::transition_existing_state(
                &mut t,
                prev,
                ProcessState::Ready,
                "finish_switch requeue ready",
            );
            crate::scheduler_contract::SchedulerContract::requeue_after_switch(
                &mut t,
                prev,
                "finish_switch requeue",
            );
        }
        crate::scheduler_contract::SchedulerContract::release_cpu_owner(
        &mut t,
        prev,
        "finish_switch release",
    );
    } else {
        // --- Zombie publication (process-lifecycle concern, decoupled) ---
        publish_exit = finish_switch_handle_zombie(&mut t, prev, state.as_ref());
    }
    t.prev[c] = 0;
    if crate::diag::diag_sched_on() {
        t.dump_queue("finish_switch after requeue");
    }
    crate::scheduler_contract::SchedulerContract::validate_table_or_panic(
        &t,
        "finish_switch cleanup",
    );
    drop(t);
    if let Some(publication) = publish_exit {
        let _ = crate::process_contract::ProcessContract::notify_zombie_publication(publication);
    }
    // Bump the watchdog's "last seen forward progress" timestamp.  Every
    // context switch is forward progress by definition.
    crate::diag::watchdog::note_progress();
}

/// F-SCHED-12: Decoupled zombie publication logic from finish_switch scheduling.
/// This handles zombie/dead processes that were switched away from.
fn finish_switch_handle_zombie(
    t: &mut ProcessTable,
    prev: u32,
    state: Option<&ProcessState>,
) -> Option<crate::process_contract::ZombiePublication> {
    if matches!(state, Some(ProcessState::Zombie)) {
        let pub_exit = crate::process_contract::ProcessContract::publish_zombie_after_switch(
            t,
            prev,
            "finish_switch exit publication",
        );
        if crate::diag::diag_sched_on() {
            crate::serial_println!(
                "[sched] finish_switch pid={} fully cleaned up (exited)",
                prev
            );
        }
        pub_exit
    } else {
        if crate::diag::diag_sched_on() {
            crate::serial_println!(
                "[sched] finish_switch pid={} (dead - already removed)",
                prev
            );
        }
        None
    }
}

/// Register the calling application processor's idle thread.  It becomes
/// `current[cpu]` and `idle[cpu]` (the fallback when the run queue is empty) and
/// is NOT placed on the run queue.  Its `kernel_rsp` slot receives the AP's
/// current (trampoline) stack the first time the AP switches to a real thread.
pub fn register_ap_idle() -> u32 {
    use alloc::format;
    let mut p = crate::process_contract::ProcessContract::create(
        crate::process_contract::ProcessCreationRequest {
            name: format!("idle{}", cpu_idx()),
            parent_pid: 0,
            kind: crate::process_contract::ProcessCreationKind::IdleThread,
            tag: "ap_idle",
        },
    );
    let pid = p.pid;
    crate::process_contract::ProcessContract::prepare_idle_context(&mut p, "ap_idle_context");
    crate::arch::without_interrupts(|| {
        let c = cpu_idx();
        crate::process_contract::ProcessContract::validate_creation_ready_or_panic(
            crate::process_contract::ProcessCreationKind::IdleThread,
            &p,
            "ap_idle_ready",
        );
        crate::process_contract::ProcessContract::admit_running_current(p, c, true, "ap_idle");
    });
    pid
}

/// Resume a newly-created process (from fork) by jumping to user space.
/// Called the first time a forked child is scheduled.
pub fn resume_forked_child(pid: u32) {
    let (rip, rsp, rflags, _rax, pml4) = {
        let table = TABLE.lock();
        if let Some(p) = table.procs.get(&pid) {
            (p.rip, p.rsp, p.rflags, p.fork_rax, p.address_space_pml4())
        } else {
            return;
        }
    };

    // Jump to user space with fork return value = 0
    crate::execution_contract::ExecutionContract::activate_process_address_space(
        pid,
        pml4,
        crate::execution_contract::ExecutionTransition::Fork,
        "resume_forked_child",
    );
    crate::process::jump_to_user(rip, rsp, rflags);
}

/// Initialize the scheduler for the first user process.
pub fn start_first_process(pid: u32) {
    let mut table = TABLE.lock();
    table.set_current(pid);
    drop(table);

    let (rip, rsp, rflags, pml4) = {
        let table = TABLE.lock();
        if let Some(p) = table.procs.get(&pid) {
            (p.rip, p.rsp, p.rflags, p.address_space_pml4())
        } else {
            return;
        }
    };

    crate::execution_contract::ExecutionContract::activate_process_address_space(
        pid,
        pml4,
        crate::execution_contract::ExecutionTransition::SwitchTo,
        "start_first_process",
    );
    crate::process::jump_to_user(rip, rsp, rflags);
}
