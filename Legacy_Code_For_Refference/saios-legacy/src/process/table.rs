//! Global process table — tracks all living and zombie processes.

use super::{Process, ProcessState};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::vec::Vec;
use spin::Mutex;

pub static TABLE: Mutex<ProcessTable> = Mutex::new(ProcessTable::new());

/// Lock TABLE with lock-order instrumentation.
/// Validates that ProcessTable priority is not violated.
#[inline]
pub fn table_lock() -> spin::MutexGuard<'static, ProcessTable> {
    crate::reliability::lock_order::lock_acquire(
        crate::reliability::lock_order::LockClass::ProcessTable,
    );
    TABLE.lock()
}

/// Try-lock TABLE with lock-order instrumentation.
/// Returns None without recording if lock is unavailable.
#[inline]
pub fn table_try_lock() -> Option<spin::MutexGuard<'static, ProcessTable>> {
    let guard = TABLE.try_lock()?;
    crate::reliability::lock_order::lock_acquire(
        crate::reliability::lock_order::LockClass::ProcessTable,
    );
    Some(guard)
}

/// Release notification for lock-order tracking.
/// Call when the TABLE MutexGuard is about to be dropped.
#[inline]
pub fn table_unlock() {
    crate::reliability::lock_order::lock_release(
        crate::reliability::lock_order::LockClass::ProcessTable,
    );
}

/// Best-effort state query for the process currently on `cpu`.
/// Returns `None` if the table is locked (avoids deadlock from IRQ context).
pub fn current_process_state(cpu: usize) -> Option<ProcessState> {
    let t = TABLE.try_lock()?;
    let pid = t.current[cpu];
    if pid == 0 {
        return None;
    }
    t.procs.get(&pid).map(|p| p.state.clone())
}

pub(crate) fn trace_pid(_pid: u32) -> bool {
    crate::diag::diag_proc_on()
}

/// Maximum CPUs the per-CPU scheduler tracks.
pub const MAX_CPUS: usize = 64;

/// Index of the calling CPU (its local-APIC ID, clamped).  The LAPIC ID register
/// is readable on the BSP from early boot (default base), so this is valid even
/// before SMP bringup — it returns 0 on the BSP.
#[inline]
pub fn cpu_idx() -> usize {
    (crate::smp::lapic_id() as usize).min(MAX_CPUS - 1)
}

/// Structured zombie entry replacing bare (pid, parent_pid, exit_code) tuple.
/// F-TABLE-05 constitutional fix: full exit metadata for wait4/waitpid callers.
#[derive(Clone, Debug)]
pub struct ZombieEntry {
    pub pid: u32,
    pub parent_pid: u32,
    pub exit_code: i64,
    pub exit_signal: u32,
    pub cpu: u8,
}

pub struct ProcessTable {
    pub procs: BTreeMap<u32, Process>,
    pub zombies: Vec<ZombieEntry>,
    /// GLOBAL scheduler-owned FIFO protected by TABLE. Runnable PIDs are NOT
    /// assigned to any CPU; a thread is in exactly one place: `current[cpu]` for
    /// one CPU, this queue, or blocked/zombie.
    pub(super) run_queue: Vec<u32>,
    pub(super) current: [u32; MAX_CPUS], // PID running on each CPU (0 = none)
    pub(super) idle: [u32; MAX_CPUS],    // per-CPU idle PID (fallback when queue empty)
    pub(super) prev: [u32; MAX_CPUS],    // PID switched away from (for finish_switch)
}

#[derive(Clone)]
pub struct SchedulerSnapshot {
    pub run_queue: Vec<u32>,
    pub current: [u32; MAX_CPUS],
    pub idle: [u32; MAX_CPUS],
    pub prev: [u32; MAX_CPUS],
}

impl ProcessTable {
    pub const fn new() -> Self {
        Self {
            procs: BTreeMap::new(),
            zombies: Vec::new(),
            run_queue: Vec::new(),
            current: [0; MAX_CPUS],
            idle: [0; MAX_CPUS],
            prev: [0; MAX_CPUS],
        }
    }

    pub fn insert(&mut self, proc: Process) {
        self.insert_with_reason(proc, "insert", "ProcessTable::insert");
    }

    pub fn insert_with_reason(
        &mut self,
        proc: Process,
        reason: &'static str,
        caller: &'static str,
    ) {
        let pid = proc.pid;
        self.procs.insert(pid, proc);
        // Constitutional admission gate: New → Ready (DOC-07).
        // Process was created in New state; admit it to the run queue as Ready.
        if let Some(p) = self.procs.get_mut(&pid)
            && p.state == ProcessState::New
        {
            p.ready_since_ns = crate::time::uptime_ns().max(1);
            p.state = ProcessState::Ready;
        }
        self.run_queue.push(pid);
        crate::serial_println!("[scheduler] enqueue pid={} queue_len={} reason={} caller={}",
            pid, self.run_queue.len(), reason, caller);
        let _ = (reason, caller);
        if trace_pid(pid) {
            crate::println!("insert pid={}", pid);
        }
        if crate::diag::diag_sched_on() {
            crate::serial_println!(
                "[sched] enqueue pid={} queue_len={} proc_count={}",
                pid,
                self.run_queue.len(),
                self.procs.len()
            );
            self.dump_queue("after enqueue");
        }
    }

    /// Insert a process into the table without making it runnable.
    /// Used for user-mode processes spawned via process::spawn().
    pub fn insert_detached(&mut self, proc: Process) {
        let pid = proc.pid;
        self.procs.insert(pid, proc);
        if trace_pid(pid) {
            crate::println!("insert pid={}", pid);
        }
        if crate::diag::diag_sched_on() {
            crate::serial_println!(
                "[sched] track pid={} detached proc_count={}",
                pid,
                self.procs.len()
            );
        }
    }

    /// All live PIDs (for /proc enumeration).
    pub fn pids(&self) -> alloc::vec::Vec<u32> {
        self.procs.keys().copied().collect()
    }
    /// (name) of a process by PID, if it exists.
    pub fn name_of(&self, pid: u32) -> Option<alloc::string::String> {
        self.procs.get(&pid).map(|p| p.name.clone())
    }

    pub fn scheduler_snapshot(&self) -> SchedulerSnapshot {
        SchedulerSnapshot {
            run_queue: self.run_queue.clone(),
            current: self.current,
            idle: self.idle,
            prev: self.prev,
        }
    }

    pub fn current_on_cpu(&self, cpu: usize) -> Option<u32> {
        self.current.get(cpu).copied().filter(|pid| *pid != 0)
    }

    pub fn pid_has_switch_publication_pending(&self, pid: u32) -> bool {
        self.current.contains(&pid) || self.prev.contains(&pid)
    }

    pub(crate) fn remove_from_run_queue(&mut self, pid: u32) {
        self.run_queue.retain(|&queued| queued != pid);
    }

    pub(crate) fn enqueue_recovered_if_absent(&mut self, pid: u32) {
        if !self.run_queue.contains(&pid) {
            self.run_queue.push(pid);
        }
    }

    pub fn restore_running_after_blocked_schedule(&mut self, pid: u32) -> Option<u64> {
        let cpu = cpu_idx();
        let prev = self.current[cpu];
        self.run_queue.retain(|&queued| queued != pid);
        if self
            .procs
            .get(&pid)
            .is_some_and(|proc| proc.state == ProcessState::Blocked)
        {
            self.set_state(pid, ProcessState::Ready);
        }
        self.set_state(pid, ProcessState::Running);
        // Use contract path for CPU ownership (F-TABLE-04 fix).
        self.claim_contract_current(cpu, pid, prev);
        let kernel_stack_top = self.procs.get(&pid)?.kernel_stack_top();
        crate::scheduler_contract::SchedulerContract::validate_table_or_panic(
            self,
            "restore running after blocked schedule",
        );
        Some(kernel_stack_top)
    }

    /// Remove a fault-terminated process from the table and run queue.
    /// Returns the process if it was found and removed.
    pub fn remove_faulted(&mut self, pid: u32) -> Option<Process> {
        if crate::diag::diag_proc_on() {
            crate::serial_println!("[fault] removing pid={} from scheduler", pid);
            self.dump_queue("remove_faulted before");
        }
        // Remove from run queue if present
        self.run_queue.retain(|&p| p != pid);
        // Transition through Zombie before removal (F-TABLE-03 constitutional fix).
        if let Some(proc) = self.procs.get_mut(&pid)
            && !matches!(proc.state, ProcessState::Zombie | ProcessState::Dead)
        {
            let parent = proc.parent_pid;
            proc.state = ProcessState::Zombie;
            proc.exit_code = -11; // SIGSEGV equivalent
            self.zombies.push(ZombieEntry {
                pid,
                parent_pid: parent,
                exit_code: -11,
                exit_signal: 11, // SIGSEGV
                cpu: cpu_idx() as u8,
            });
            crate::kds::kds_event(
                crate::kds::KdsSubsystem::Process,
                crate::kds::KdsEventType::TaskExit,
                crate::kds::KdsSeverity::Warn,
                [pid as u64, parent as u64, 11, 0],
            );
        }
        let proc = self.procs.remove(&pid);
        if trace_pid(pid) {
            crate::println!("[task] pid={} -> Dead (faulted)", pid);
            crate::println!("remove pid={}", pid);
        }
        if crate::diag::diag_proc_on() {
            crate::serial_println!(
                "[fault] removed pid={} queue_len={} proc_count={}",
                pid,
                self.run_queue.len(),
                self.procs.len()
            );
            self.dump_queue("remove_faulted after");
        }
        proc
    }

    /// PID currently running on the calling CPU.
    pub fn current_pid(&self) -> u32 {
        self.current[cpu_idx()]
    }
    /// Set the PID running on the calling CPU.
    pub fn set_current(&mut self, pid: u32) {
        self.current[cpu_idx()] = pid;
    }

    pub(crate) fn register_contract_running_current(
        &mut self,
        cpu: usize,
        pid: u32,
        make_idle: bool,
    ) {
        let mut displaced = [0u32; MAX_CPUS];
        let mut displaced_len = 0usize;
        for (&other_pid, proc) in self.procs.iter_mut() {
            if other_pid != pid && proc.is_on_cpu() && proc.cpu_owner() == Some(cpu) {
                proc.set_contract_cpu_owner(None, false);
                if displaced_len < displaced.len() {
                    displaced[displaced_len] = other_pid;
                    displaced_len += 1;
                }
            }
        }

        self.current[cpu] = pid;
        if make_idle {
            self.idle[cpu] = pid;
        }
        if let Some(proc) = self.procs.get_mut(&pid) {
            proc.set_contract_cpu_owner(Some(cpu), true);
        }

        for &displaced_pid in displaced[..displaced_len].iter() {
            if self
                .procs
                .get(&displaced_pid)
                .is_some_and(|proc| proc.state == ProcessState::Running)
            {
                self.set_state(displaced_pid, ProcessState::Ready);
            }
            if self
                .procs
                .get(&displaced_pid)
                .is_some_and(|proc| proc.state == ProcessState::Ready)
                && !self.run_queue.contains(&displaced_pid)
            {
                self.run_queue.push(displaced_pid);
            }
        }
    }

    pub(crate) fn claim_contract_current(&mut self, cpu: usize, next: u32, prev: u32) {
        self.current[cpu] = next;
        self.prev[cpu] = prev;
        // Constitutional Invariant 2: on_cpu means exactly one current[cpu] slot.
        // Clear prev's on_cpu immediately — it is no longer in any current slot.
        if prev != 0
            && let Some(proc) = self.procs.get_mut(&prev)
        {
            proc.set_contract_cpu_owner(None, false);
        }
        if let Some(proc) = self.procs.get_mut(&next) {
            proc.set_contract_cpu_owner(Some(cpu), true);
        }
    }

    pub(crate) fn clear_contract_cpu_owner(&mut self, pid: u32) {
        if let Some(proc) = self.procs.get_mut(&pid) {
            proc.set_contract_cpu_owner(None, false);
        }
    }

    pub fn current_mut(&mut self) -> Option<&mut Process> {
        let pid = self.current[cpu_idx()];
        self.procs.get_mut(&pid)
    }

    pub fn current_ref(&self) -> Option<&Process> {
        self.procs.get(&self.current[cpu_idx()])
    }

    pub fn set_state(&mut self, pid: u32, next: ProcessState) -> bool {
        let Some(proc) = self.procs.get_mut(&pid) else {
            return false;
        };
        crate::process_contract::ProcessContract::validate_existing_transition_or_panic(
            pid,
            &proc.state,
            &next,
            "set_state",
        );
        if trace_pid(pid) && proc.state != next {
            crate::println!("[task] pid={} {:?} -> {:?}", pid, proc.state, next);
        }
        if proc.state != next {
            let event_type = match next {
                ProcessState::Blocked => crate::kds::KdsEventType::TaskBlock,
                ProcessState::Ready => crate::kds::KdsEventType::TaskUnblock,
                ProcessState::Zombie | ProcessState::Dead => crate::kds::KdsEventType::TaskExit,
                ProcessState::Running => crate::kds::KdsEventType::ContextSwitch,
                ProcessState::New => crate::kds::KdsEventType::TaskCreate,
            };
            crate::observability_contract::ObservabilityContract::kds_event_for(
                crate::kds::KdsSubsystem::Process,
                event_type,
                crate::kds::KdsSeverity::Trace,
                pid,
                pid,
                [pid as u64, 0, 0, 0],
            );
        }
        // Starvation aging: stamp when entering Ready, clear when leaving.
        if next == ProcessState::Ready && proc.state != ProcessState::Ready {
            proc.ready_since_ns = crate::time::uptime_ns().max(1);
        } else if next != ProcessState::Ready {
            proc.ready_since_ns = 0;
        }
        proc.state = next;
        true
    }

    pub fn mark_exiting(&mut self, pid: u32, exit_code: i64, tag: &'static str) -> bool {
        let Some(proc) = self.procs.get_mut(&pid) else {
            return false;
        };
        proc.exit_code = exit_code;
        crate::process_contract::ProcessContract::validate_existing_transition_or_panic(
            pid,
            &proc.state,
            &ProcessState::Zombie,
            tag,
        );
        if trace_pid(pid) && proc.state != ProcessState::Zombie {
            crate::println!("[task] pid={} {:?} -> Zombie", pid, proc.state);
        }
        crate::serial_println!(
            "[zombie] mark pid={} parent={} from={:?} exit_code={} tag={}",
            pid,
            proc.parent_pid,
            proc.state,
            exit_code,
            tag
        );
        crate::observability_contract::ObservabilityContract::kds_event_for(
            crate::kds::KdsSubsystem::Process,
            crate::kds::KdsEventType::TaskExit,
            crate::kds::KdsSeverity::Info,
            pid,
            pid,
            [
                pid as u64,
                exit_code as u64,
                tag.as_ptr() as u64,
                tag.len() as u64,
            ],
        );
        proc.state = ProcessState::Zombie;
        true
    }

    pub fn block_current(&mut self) -> Option<u32> {
        let pid = self.current_pid();
        let old_state = self.procs.get(&pid)?.state.clone();
        debug_assert!(
            matches!(old_state, ProcessState::Running | ProcessState::Ready),
            "block_current: pid {} entered blocking from unexpected state {:?}",
            pid,
            old_state
        );
        self.set_state(pid, ProcessState::Blocked);
        self.run_queue.retain(|&queued| queued != pid);
        Some(pid)
    }

    pub fn wake_pid(&mut self, pid: u32) -> bool {
        let Some(proc) = self.procs.get(&pid) else {
            return false;
        };
        if proc.state != ProcessState::Blocked {
            debug_assert!(
                proc.state == ProcessState::Blocked,
                "wake_pid: pid {} was woken from unexpected state {:?}",
                pid,
                proc.state
            );
            return false;
        }
        let on_cpu = proc.is_on_cpu();
        let stale_on_cpu = on_cpu && !self.current.contains(&pid);
        self.set_state(pid, ProcessState::Ready);
        if stale_on_cpu && let Some(proc) = self.procs.get_mut(&pid) {
            proc.set_contract_cpu_owner(None, false);
        }
        if (!on_cpu || stale_on_cpu) && !self.run_queue.contains(&pid) {
            self.run_queue.push(pid);
            crate::serial_println!("[wake-pid] enqueue pid={} queue_len={} on_cpu={} stale_on_cpu={}",
                pid, self.run_queue.len(), on_cpu, stale_on_cpu);
        } else {
            crate::serial_println!("[wake-pid] enqueue-skip pid={} on_cpu={} stale_on_cpu={} already_queued={}",
                pid, on_cpu, stale_on_cpu, self.run_queue.contains(&pid));
        }
        debug_assert!(
            on_cpu == stale_on_cpu || !self.run_queue.contains(&pid),
            "wake_pid: on-CPU pid {} must not be queued runnable",
            pid
        );
        true
    }

pub fn pick_next(&mut self) -> Option<u32> {
    let cpu = cpu_idx();
    let placement =
        crate::scheduler_contract::SchedulerContract::placement_for_cpu(cpu);

    if crate::diag::diag_sched_on() {
        crate::serial_println!(
            "[schedule] runnable cpu={} numa={:?}:",
            placement.cpu,
            placement.numa_node
        );

        for &pid in &self.run_queue {
            if let Some(proc) = self.procs.get(&pid) {
                let score =
                    crate::scheduler_contract::SchedulerContract::placement_score(
                        proc,
                        placement,
                    );

                crate::serial_println!(
                    "[schedule]   pid={} state={:?} on_cpu={} cpu={:?} current_slot={} allowed={:#x} preferred={:?} numa={:?} score={:?}",
                    pid,
                    proc.state,
                    proc.is_on_cpu(),
                    proc.cpu_owner(),
                    self.current.contains(&pid),
                    proc.scheduling.allowed_cpus,
                    proc.scheduling.preferred_cpu,
                    proc.scheduling.numa_node,
                    score
                );
            } else {
                crate::serial_println!(
                    "[schedule]   pid={} missing",
                    pid
                );
            }
        }
    }

    let now_ns = crate::time::uptime_ns();

    const STARVATION_THRESHOLD_NS: u64 = 10_000_000_000; // 10 seconds
    const STARVATION_BOOST: u8 = 100;

    let mut best_local: Option<(usize, u32, u8)> = None;
    let mut best_global: Option<(usize, u32, u8)> = None;

    for (idx, &pid) in self.run_queue.iter().enumerate() {
        let Some(proc) = self.procs.get(&pid) else {
            continue;
        };

        if proc.state != ProcessState::Ready {
            continue;
        }

        if proc.is_on_cpu() {
            continue;
        }

        let mut score =
            match crate::scheduler_contract::SchedulerContract::placement_score(
                proc,
                placement,
            ) {
                Some(score) => score,
                None => continue,
            };

        let wait_ns =
            now_ns.saturating_sub(proc.ready_since_ns);

        if wait_ns >= STARVATION_THRESHOLD_NS {
            score += STARVATION_BOOST;
        }

        let prefers_this_cpu =
            proc.scheduling.preferred_cpu == Some(cpu);

        if prefers_this_cpu {
            match best_local {
                Some((_, _, best_score)) if score <= best_score => {}
                _ => best_local = Some((idx, pid, score)),
            }
        }

        match best_global {
            Some((_, _, best_score)) if score <= best_score => {}
            _ => best_global = Some((idx, pid, score)),
        }
    }

    let selected = best_local.or(best_global);

    let Some((idx, pid, _)) = selected else {
        return None;
    };

    let pid = self.run_queue.swap_remove(idx);

    if let Some(proc) = self.procs.get_mut(&pid) {
        proc.ready_since_ns = now_ns;
    }

    if crate::diag::diag_sched_on() {
        crate::serial_println!(
            "[scheduler] dequeue pid={} cpu={} queue_len={}",
            pid,
            cpu,
            self.run_queue.len()
        );
    }

    Some(pid)
}

    /// Check if a process is eligible for pick_next selection.
    #[inline]
    fn is_pick_eligible(&self, proc: &Process, pid: u32) -> bool {
        matches!(proc.state, ProcessState::Ready | ProcessState::Running)
            && !proc.is_on_cpu()
            && !self.current.contains(&pid)
    }

    /// Human-readable reason why a process would be rejected by pick_next.
    #[inline]
    fn pick_reject_reason(
        &self,
        proc: &Process,
        pid: u32,
        placement: crate::scheduler_contract::SchedulerPlacement,
    ) -> Option<&'static str> {
        if !matches!(proc.state, ProcessState::Ready | ProcessState::Running) {
            return Some("state_not_runnable");
        }
        if proc.is_on_cpu() {
            return Some("on_cpu");
        }
        if self.current.contains(&pid) {
            return Some("current_slot");
        }
        if let Err(reason) =
            crate::scheduler_contract::SchedulerContract::can_run_on_cpu(proc, placement)
        {
            return Some(reason);
        }
        None
    }

    /// Apply starvation aging and priority inheritance boosts to a score.
    #[inline]
    fn apply_boosts(&self, proc: &Process, pid: u32, mut score: u8, now_ns: u64) -> u8 {
        const STARVATION_THRESHOLD_NS: u64 = 10_000_000_000;
        const STARVATION_BOOST: u8 = 100;
        if proc.ready_since_ns > 0
            && now_ns.saturating_sub(proc.ready_since_ns) > STARVATION_THRESHOLD_NS
        {
            score = score.saturating_add(STARVATION_BOOST);
            let wait_ns = now_ns.saturating_sub(proc.ready_since_ns);
            crate::kds::kds_event(
                crate::kds::KdsSubsystem::Scheduler,
                crate::kds::KdsEventType::SchedulerStarvation,
                crate::kds::KdsSeverity::Warn,
                [pid as u64, wait_ns, STARVATION_BOOST as u64, 0],
            );
        }
        score.saturating_add(proc.pi_boost)
    }

    /// Emit diagnostic serial output after dequeue.
    #[inline]
    fn diag_dequeue(&self, pid: u32) {
        if crate::diag::diag_sched_on() {
            crate::serial_println!(
                "[sched] dequeue pid={} queue_len={} proc_count={}",
                pid,
                self.run_queue.len(),
                self.procs.len()
            );
            self.dump_queue("after dequeue");
        }
    }

    /// Dump current queue contents for debugging.
    pub fn dump_queue(&self, tag: &str) {
        let mut q = alloc::vec::Vec::new();
        for &pid in &self.run_queue {
            if let Some(name) = self.procs.get(&pid).map(|p| p.name.clone()) {
                q.push(format!("{}({})", pid, name));
            } else {
                q.push(format!("{}(?)", pid));
            }
        }
        crate::serial_println!("[sched] queue[{}]: [{}]", tag, q.join(", "));
    }

    /// Log scheduler ownership invariants without panicking.
    ///
    /// At any stable point there should be exactly one Running/on_cpu task for
    /// every online scheduler CPU, and every online `current[cpu]` slot should
    /// name a unique Running/on_cpu process. This diagnostic is intentionally
    /// non-fatal so a broken boot can still leave a serial log.
    pub fn log_invariants(&self, tag: &str) {
        let online_mask = crate::smp::online_mask();
        let online_count = crate::smp::cpu_count() as usize;
        let mut current_pids = Vec::new();
        let mut current_nonzero = 0usize;
        let mut current_missing = 0usize;
        let mut current_not_running = 0usize;
        let mut current_not_on_cpu = 0usize;

        for cpu in 0..MAX_CPUS {
            if online_mask & (1u64 << cpu) == 0 {
                continue;
            }
            let pid = self.current[cpu];
            if pid == 0 {
                current_missing += 1;
                crate::serial_println!("[sched-invariant] {} VIOLATION cpu{} current=0", tag, cpu);
                continue;
            }
            current_nonzero += 1;
            current_pids.push(pid);
            match self.procs.get(&pid) {
                Some(proc) => {
                    if proc.state != ProcessState::Running {
                        current_not_running += 1;
                        crate::serial_println!(
                            "[sched-invariant] {} VIOLATION cpu{} current pid={} state={:?}",
                            tag,
                            cpu,
                            pid,
                            proc.state
                        );
                    }
                    if !proc.is_on_cpu() {
                        current_not_on_cpu += 1;
                        crate::serial_println!(
                            "[sched-invariant] {} VIOLATION cpu{} current pid={} on_cpu=false",
                            tag,
                            cpu,
                            pid
                        );
                    }
                }
                None => {
                    current_missing += 1;
                    crate::serial_println!(
                        "[sched-invariant] {} VIOLATION cpu{} current pid={} missing",
                        tag,
                        cpu,
                        pid
                    );
                }
            }
        }

        let mut duplicate_current = 0usize;
        for i in 0..current_pids.len() {
            if current_pids[..i].contains(&current_pids[i]) {
                duplicate_current += 1;
                crate::serial_println!(
                    "[sched-invariant] {} VIOLATION duplicate current pid={}",
                    tag,
                    current_pids[i]
                );
            }
        }

        let running_count = self
            .procs
            .values()
            .filter(|proc| proc.state == ProcessState::Running)
            .count();
        let on_cpu_count = self.procs.values().filter(|proc| proc.is_on_cpu()).count();
        let non_current_on_cpu = self
            .procs
            .values()
            .filter(|proc| proc.is_on_cpu() && !current_pids.contains(&proc.pid))
            .count();
        for proc in self
            .procs
            .values()
            .filter(|proc| proc.is_on_cpu() && !current_pids.contains(&proc.pid))
        {
            crate::serial_println!(
                "[sched-invariant] {} VIOLATION non-current on_cpu pid={} name={} cpu={:?} state={:?}",
                tag,
                proc.pid,
                proc.name,
                proc.cpu_owner(),
                proc.state
            );
        }
        let queued_current = self
            .run_queue
            .iter()
            .filter(|pid| current_pids.contains(pid))
            .count();
        let queued_on_cpu = self
            .run_queue
            .iter()
            .filter(|pid| self.procs.get(pid).is_some_and(|proc| proc.is_on_cpu()))
            .count();

        crate::serial_println!(
            "[sched-invariant] {} online={} mask={:#x} current_nonzero={} running={} on_cpu={} non_current_on_cpu={} queued_current={} queued_on_cpu={}",
            tag,
            online_count,
            online_mask,
            current_nonzero,
            running_count,
            on_cpu_count,
            non_current_on_cpu,
            queued_current,
            queued_on_cpu
        );

        if running_count != online_count {
            crate::serial_println!(
                "[sched-invariant] {} VIOLATION running_count={} online_count={}",
                tag,
                running_count,
                online_count
            );
        }
        if on_cpu_count != online_count {
            crate::serial_println!(
                "[sched-invariant] {} VIOLATION on_cpu_count={} online_count={}",
                tag,
                on_cpu_count,
                online_count
            );
        }
        if current_nonzero != online_count || current_missing != 0 {
            crate::serial_println!(
                "[sched-invariant] {} VIOLATION current_nonzero={} current_missing={} online_count={}",
                tag,
                current_nonzero,
                current_missing,
                online_count
            );
        }
        if current_not_running != 0
            || current_not_on_cpu != 0
            || duplicate_current != 0
            || non_current_on_cpu != 0
            || queued_current != 0
            || queued_on_cpu != 0
        {
            crate::serial_println!(
                "[sched-invariant] {} VIOLATION detail current_not_running={} current_not_on_cpu={} duplicate_current={} non_current_on_cpu={} queued_current={} queued_on_cpu={}",
                tag,
                current_not_running,
                current_not_on_cpu,
                duplicate_current,
                non_current_on_cpu,
                queued_current,
                queued_on_cpu
            );
        }

        if running_count != online_count
            || on_cpu_count != online_count
            || current_nonzero != online_count
            || current_missing != 0
            || current_not_running != 0
            || current_not_on_cpu != 0
            || duplicate_current != 0
            || non_current_on_cpu != 0
            || queued_current != 0
            || queued_on_cpu != 0
        {
            panic!(
                "[sched-invariant] {} fatal scheduler ownership violation",
                tag
            );
        }
    }

    /// Pop the first zombie whose parent matches `parent_pid`.
    pub fn pop_zombie(&mut self, parent_pid: u32, want_pid: u32) -> Option<(u32, i64)> {
        let idx = self.zombies.iter().position(|z| {
            z.parent_pid == parent_pid
                && (want_pid == 0 || want_pid == z.pid || want_pid as i64 == -1)
        })?;
        let z = self.zombies.remove(idx);
        if crate::diag::diag_proc_on() {
            crate::println!(
                "zombie-pop: parent={} child={} remaining_zombies={}",
                parent_pid,
                z.pid,
                self.zombies.len()
            );
        }
        Some((z.pid, z.exit_code))
    }

    pub fn find_waitable_child(&self, parent_pid: u32, want_pid: u32) -> Option<(u32, &Process)> {
        self.procs
            .iter()
            .find(|&(pid, proc)| {
                proc.parent_pid == parent_pid
                    && (want_pid == 0 || want_pid == *pid || want_pid as i32 == -1)
            })
            .map(|(&pid, proc)| (pid, proc))
    }

    pub fn find_waitable_zombie(&self, parent_pid: u32, want_pid: u32) -> Option<(u32, i64)> {
        self.zombies
            .iter()
            .find(|z| {
                z.parent_pid == parent_pid
                    && (want_pid == 0 || want_pid == z.pid || want_pid as i32 == -1)
            })
            .map(|z| (z.pid, z.exit_code))
    }

    pub fn pids_in_process_group(&self, pgid: u32) -> Vec<u32> {
        self.procs
            .values()
            .filter(|proc| proc.pgid == pgid)
            .map(|proc| proc.pid)
            .collect()
    }
}

fn state_transition_allowed(from: &ProcessState, to: &ProcessState) -> bool {
    if from == to {
        return true;
    }
    matches!(
        (from, to),
        (ProcessState::Ready, ProcessState::Running)
            | (ProcessState::Running, ProcessState::Ready)
            | (ProcessState::Running, ProcessState::Blocked)
            | (ProcessState::Ready, ProcessState::Blocked)
            | (ProcessState::Blocked, ProcessState::Ready)
            | (ProcessState::Running, ProcessState::Zombie)
            | (ProcessState::Zombie, ProcessState::Dead)
    )
}
