//! Cooperative/preemptive thread scheduler.
//!
//! Maintains a set of kernel threads, a run queue and a simple round-robin
//! policy driven by timer ticks. Context switch is performed in assembly by
//! [`switch_context`].  Each user-mode capable thread owns a dedicated kernel
//! transition stack; `TSS.RSP0` and `SAIOS_SYSCALL_RSP0` are updated on every
//! context switch so ring3→ring0 transitions always land on the correct stack.

use crate::kernel::constants::{KERNEL_THREAD_STACK_SIZE, USER_PROCESS_KERNEL_STACK_SIZE};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::interrupt;
use hal::arch::x86_64::seed_support::FaultRecoveryContext;
use hal::arch::x86_64::syscall::UserSyscallFrame;
use hal::arch::x86_64::sync::StaticCell;

const STACK_SIZE: usize = KERNEL_THREAD_STACK_SIZE;
const DEFAULT_QUANTUM_TICKS: u64 = 10;

#[path = "scheduler/tests.rs"]
pub mod tests;

/// Unique identifier for a scheduler thread.
pub type ThreadId = u64;
/// Virtual address type.
pub type VirtAddr = u64;

type ThreadEntry = fn();

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
/// Execution state of a scheduler thread.
pub enum ThreadState {
    Ready,
    Running,
    Sleeping,
    Blocked,
    Dead,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
/// Callee-saved CPU state used for context switches.
pub struct CpuContext {
    pub rsp: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

#[derive(Debug, Copy, Clone)]
/// A kernel thread.
pub struct Thread {
    /// Thread identifier.
    pub id: ThreadId,
    /// Current execution state.
    pub state: ThreadState,
    /// Saved CPU context.
    pub context: CpuContext,
    /// Top of the thread's stack.
    pub stack_top: VirtAddr,
}

#[derive(Debug, Copy, Clone)]
/// Lightweight snapshot of a thread for introspection.
pub struct ThreadInfo {
    /// Thread identifier.
    pub id: ThreadId,
    /// Current execution state.
    pub state: ThreadState,
}

struct ThreadRecord {
    thread: Thread,
    entry: ThreadEntry,
    _stack: Box<[u8]>,
    /// Dedicated kernel transition stack for ring3→ring0 (syscall/interrupt).
    /// `None` for pure kernel threads that never enter ring 3.
    #[allow(dead_code)]  // owned for its lifetime; drop frees the memory
    user_kernel_stack: Option<Box<[u8]>>,
    /// Top of `user_kernel_stack`; written to TSS.RSP0 and SAIOS_SYSCALL_RSP0
    /// when this thread is scheduled. 0 = no user-mode stack.
    user_kernel_rsp0: u64,
    /// Non-None when this thread is `Blocked` waiting for a specific pid.
    waiting_for_pid: Option<u64>,
    /// Saved fault-recovery statics for per-thread user-mode execution context.
    fault_recovery: FaultRecoveryContext,
    /// `ACTIVE_EXEC_PID` at the time this thread was last context-switched out.
    /// Restored when the thread is switched back in so syscalls can resolve
    /// the correct process pid.
    saved_active_exec_pid: Option<u64>,
    /// CR3 active when this thread was last context-switched out.
    /// User-exec threads can run under isolated roots; kernel-only threads
    /// must restore the kernel root before resuming.
    saved_cr3: u64,
}

struct Scheduler {
    initialized: bool,
    threads: Vec<ThreadRecord>,
    run_queue: VecDeque<usize>,
    sleep_queue: Vec<(u64, usize)>,
    current: usize,
    idle: usize,
    next_id: ThreadId,
    quantum_ticks: u64,
    ticks_since_switch: u64,
    needs_reschedule: bool,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            initialized: false,
            threads: Vec::new(),
            run_queue: VecDeque::new(),
            sleep_queue: Vec::new(),
            current: 0,
            idle: 0,
            next_id: 0,
            quantum_ticks: DEFAULT_QUANTUM_TICKS,
            ticks_since_switch: 0,
            needs_reschedule: false,
        }
    }

    fn alloc_id(&mut self) -> ThreadId {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    fn spawn_internal(&mut self, entry: ThreadEntry) -> ThreadId {
        self.spawn_internal_with_user_stack(entry, None, 0)
    }

    fn spawn_internal_with_user_stack(
        &mut self,
        entry: ThreadEntry,
        user_stack: Option<Box<[u8]>>,
        user_rsp0: u64,
    ) -> ThreadId {
        let mut stack = vec![0u8; STACK_SIZE].into_boxed_slice();
        let stack_base = stack.as_mut_ptr() as usize;
        let stack_top = stack_base + STACK_SIZE;

        let mut rsp = stack_top & !0xF;
        rsp -= core::mem::size_of::<usize>();
        unsafe {
            *(rsp as *mut usize) = thread_trampoline as *const () as usize;
        }

        let thread = Thread {
            id: self.alloc_id(),
            state: ThreadState::Ready,
            context: CpuContext {
                rsp: rsp as u64,
                ..CpuContext::default()
            },
            stack_top: stack_top as u64,
        };

        self.threads.push(ThreadRecord {
            thread,
            entry,
            _stack: stack,
            user_kernel_stack: user_stack,
            user_kernel_rsp0: user_rsp0,
            waiting_for_pid: None,
            fault_recovery: FaultRecoveryContext::default(),
            saved_active_exec_pid: None,
            saved_cr3: crate::vmm::stats().cr3 & crate::vmm::ADDR_MASK,
        });

        let idx = self.threads.len() - 1;
        self.run_queue.push_back(idx);
        thread.id
    }

    fn queue_sleep(&mut self, idx: usize, wake_tick: u64) {
        self.sleep_queue
            .retain(|(_, queued_idx)| *queued_idx != idx);

        let pos = self
            .sleep_queue
            .iter()
            .position(|(tick, _)| *tick > wake_tick)
            .unwrap_or(self.sleep_queue.len());
        self.sleep_queue.insert(pos, (wake_tick, idx));
    }

    fn wake_due(&mut self, current_tick: u64) {
        while let Some((wake_tick, idx)) = self.sleep_queue.first().copied() {
            if wake_tick > current_tick {
                break;
            }

            self.sleep_queue.remove(0);

            if self.threads[idx].thread.state == ThreadState::Sleeping {
                self.threads[idx].thread.state = ThreadState::Ready;
                self.run_queue.push_back(idx);
            }
        }
    }
}

static SCHEDULER: StaticCell<Option<Scheduler>> = StaticCell::new(None);
static USER_SESSION_STARTED: AtomicBool = AtomicBool::new(false);

/// Data passed from `spawn_user_child_thread` to `child_exec_entry`.
/// A single global suffices because the parent blocks until the child exits
/// (cooperative single-CPU model).
struct PendingChildSpawn {
    child_pid:        u64,
    #[allow(dead_code)]  // reserved for future multi-session diagnostics
    parent_thread_id: ThreadId,
    child_frame:      UserSyscallFrame,
    child_fs_base:    Option<u64>,
}
static PENDING_CHILD_SPAWN: StaticCell<Option<PendingChildSpawn>> = StaticCell::new(None);

fn scheduler_mut() -> Option<&'static mut Scheduler> {
    unsafe { (&mut *SCHEDULER.get()).as_mut() }
}

fn scheduler_ref() -> Option<&'static Scheduler> {
    unsafe { (&*SCHEDULER.get()).as_ref() }
}

fn idle_entry() {
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}

fn thread_trampoline() -> ! {
    let entry = interrupt::without_interrupts(|| {
        let scheduler = scheduler_mut().expect("scheduler not initialized");
        scheduler.threads[scheduler.current].entry
    });

    entry();

    interrupt::without_interrupts(|| {
        let scheduler = scheduler_mut().expect("scheduler not initialized");
        scheduler.threads[scheduler.current].thread.state = ThreadState::Dead;
    });

    loop {
        yield_now();
    }
}

fn pick_next(scheduler: &mut Scheduler) -> usize {
    let mut idle_candidate: Option<usize> = None;

    while let Some(idx) = scheduler.run_queue.pop_front() {
        if scheduler.threads[idx].thread.state != ThreadState::Ready {
            continue;
        }

        if idx == scheduler.idle {
            idle_candidate = Some(idx);
            continue;
        }

        if let Some(idle_idx) = idle_candidate.take() {
            scheduler.run_queue.push_back(idle_idx);
        }

        return idx;
    }

    if let Some(idle_idx) = idle_candidate
        && scheduler.threads[idle_idx].thread.state == ThreadState::Ready
    {
        return idle_idx;
    }

    scheduler.idle
}

fn do_schedule() {
    let scheduler = match scheduler_mut() {
        Some(s) if s.initialized => s,
        _ => return,
    };

    let old_idx = scheduler.current;
    let next_idx = pick_next(scheduler);

    if old_idx == next_idx {
        scheduler.needs_reschedule = false;
        scheduler.ticks_since_switch = 0;
        return;
    }

    // Save the outgoing thread's user-mode fault recovery context and the
    // currently-active exec pid so they can be restored on the next switch
    // back to this thread.
    scheduler.threads[old_idx].fault_recovery =
        hal::arch::x86_64::seed_support::save_fault_recovery_context();
    scheduler.threads[old_idx].saved_active_exec_pid =
        crate::kernel::fault::active_exec_pid();
    scheduler.threads[old_idx].saved_cr3 =
        hal::arch::paging::read_cr3() & crate::vmm::ADDR_MASK;

    if scheduler.threads[old_idx].thread.state == ThreadState::Running && old_idx != scheduler.idle
    {
        scheduler.threads[old_idx].thread.state = ThreadState::Ready;
        scheduler.run_queue.push_back(old_idx);
    }

    scheduler.threads[next_idx].thread.state = ThreadState::Running;
    scheduler.current = next_idx;
    scheduler.needs_reschedule = false;
    scheduler.ticks_since_switch = 0;

    // Update the ring3→ring0 transition stack for the incoming thread.
    let new_rsp0 = scheduler.threads[next_idx].user_kernel_rsp0;
    if new_rsp0 != 0 {
        hal::arch::x86_64::tss::set_rsp0(new_rsp0);
        hal::arch::x86_64::syscall::set_kernel_rsp0(new_rsp0);
    }

    // Restore the incoming thread's fault recovery context and exec pid.
    let new_recovery = scheduler.threads[next_idx].fault_recovery;
    let new_exec_pid = scheduler.threads[next_idx].saved_active_exec_pid;
    let kernel_cr3 = crate::vmm::stats().cr3 & crate::vmm::ADDR_MASK;
    let new_cr3 = match scheduler.threads[next_idx].saved_cr3 {
        0 => kernel_cr3,
        cr3 => cr3 & crate::vmm::ADDR_MASK,
    };

    let old_ctx = &mut scheduler.threads[old_idx].thread.context as *mut CpuContext;
    let new_ctx = &scheduler.threads[next_idx].thread.context as *const CpuContext;

    // Restore the incoming thread's CR3, recovery context, and exec pid BEFORE
    // the context switch so they are in effect as soon as that thread resumes.
    if new_cr3 != 0 && (hal::arch::paging::read_cr3() & crate::vmm::ADDR_MASK) != new_cr3 {
        unsafe { hal::arch::paging::write_cr3(new_cr3) };
    }
    unsafe {
        hal::arch::x86_64::seed_support::restore_fault_recovery_context(&new_recovery);
    }
    match new_exec_pid {
        Some(pid) => crate::kernel::fault::begin_user_exec(pid),
        None      => crate::kernel::fault::end_user_exec(),
    }

    unsafe {
        hal::arch::x86_64::seed_support::context_switch(
            old_ctx.cast::<u8>(),
            new_ctx.cast::<u8>(),
        );
    }
}

/// Initializes the scheduler and creates the bootstrap and idle threads.
pub fn init() {
    interrupt::without_interrupts(|| {
        let slot = unsafe { &mut *SCHEDULER.get() };
        if slot.is_some() {
            return;
        }

        let mut scheduler = Scheduler::new();

        let bootstrap = Thread {
            id: scheduler.alloc_id(),
            state: ThreadState::Running,
            context: CpuContext::default(),
            stack_top: 0,
        };
        scheduler.threads.push(ThreadRecord {
            thread: bootstrap,
            entry: || {},
            _stack: Box::from([]),
            user_kernel_stack: None,
            user_kernel_rsp0: 0,
            waiting_for_pid: None,
            fault_recovery: FaultRecoveryContext::default(),
            saved_active_exec_pid: None,
            saved_cr3: hal::arch::paging::read_cr3() & crate::vmm::ADDR_MASK,
        });
        scheduler.current = 0;

        let idle_id = scheduler.spawn_internal(idle_entry);
        let idle_idx = scheduler
            .threads
            .iter()
            .position(|t| t.thread.id == idle_id)
            .expect("idle thread missing");
        scheduler.idle = idle_idx;

        scheduler.initialized = true;
        *slot = Some(scheduler);
    });
}

/// Spawns a new kernel thread running `entry`.
pub fn spawn(entry: ThreadEntry) -> ThreadId {
    interrupt::without_interrupts(|| {
        let scheduler = scheduler_mut().expect("scheduler not initialized");
        scheduler.spawn_internal(entry)
    })
}

/// Returns the current thread's identifier, if the scheduler is active.
pub fn current_thread_id() -> Option<ThreadId> {
    interrupt::without_interrupts(|| {
        scheduler_ref().map(|s| s.threads[s.current].thread.id)
    })
}

/// Returns the user-mode kernel transition stack top for the current thread,
/// or the global default if the current thread has none.
pub fn current_user_rsp0() -> u64 {
    let rsp0 = interrupt::without_interrupts(|| {
        scheduler_ref().and_then(|s| {
            let rsp0 = s.threads[s.current].user_kernel_rsp0;
            if rsp0 != 0 { Some(rsp0) } else { None }
        })
    });
    rsp0.unwrap_or_else(hal::arch::x86_64::seed_support::user_transition_kernel_rsp0)
}

/// Record `rsp0` as the current thread's user-mode kernel transition stack
/// top so that future `do_schedule` calls restore it correctly.
pub fn set_current_user_rsp0(rsp0: u64) {
    interrupt::without_interrupts(|| {
        if let Some(s) = scheduler_mut() {
            s.threads[s.current].user_kernel_rsp0 = rsp0;
        }
    });
}

/// Block the current thread until the process with `pid` exits.
/// Returns immediately once `unblock_waiters_for_pid(pid)` is called.
pub fn block_current_waiting_for_pid(pid: u64) {
    interrupt::without_interrupts(|| {
        let scheduler = match scheduler_mut() {
            Some(s) if s.initialized => s,
            _ => return,
        };
        let current_idx = scheduler.current;
        if scheduler.threads[current_idx].thread.state == ThreadState::Running {
            scheduler.threads[current_idx].thread.state = ThreadState::Blocked;
            scheduler.threads[current_idx].waiting_for_pid = Some(pid);
        }
    });
    yield_now();
}

/// Wake all threads that are `Blocked` waiting for `pid`.
pub fn unblock_waiters_for_pid(pid: u64) {
    interrupt::without_interrupts(|| {
        let scheduler = match scheduler_mut() {
            Some(s) if s.initialized => s,
            _ => return,
        };
        for i in 0..scheduler.threads.len() {
            let waiting_for_pid = scheduler.threads[i].waiting_for_pid;
            if (waiting_for_pid == Some(pid) || waiting_for_pid == Some(0))
                && scheduler.threads[i].thread.state == ThreadState::Blocked
            {
                scheduler.threads[i].thread.state = ThreadState::Ready;
                scheduler.threads[i].waiting_for_pid = None;
                scheduler.run_queue.push_back(i);
            }
        }
    });
}

/// Entry function for a child-exec kernel thread spawned by `fork`.
/// Reads startup data from `PENDING_CHILD_SPAWN`, enters the child's
/// ring-3 context, and unblocks the parent when done.
fn child_exec_entry() {
    // Read and clear the pending spawn data atomically (no real concurrency,
    // but keeps the pattern clear).
    let data = interrupt::without_interrupts(|| {
        unsafe { (*PENDING_CHILD_SPAWN.get()).take() }
    })
    .expect("child_exec_entry: no pending spawn data");

    let child_pid = data.child_pid;
    let frame     = data.child_frame;

    // Allocate a dedicated kernel transition stack for the child's ring-0
    // syscall handling.  Without this, `saios_syscall_entry` resets RSP to
    // K_child_top on every syscall, and the resulting call chain (e.g. the
    // execve → ELF-loader path) overwrites the `hal_enter_user_mode_from_frame`
    // recovery frame that was saved in K_child, causing a corrupted `ret`
    // target when `resume_from_user_fault` fires.
    let mut child_kstack = vec![0u8; USER_PROCESS_KERNEL_STACK_SIZE].into_boxed_slice();
    let child_rsp0 = child_kstack.as_mut_ptr() as u64
        + USER_PROCESS_KERNEL_STACK_SIZE as u64;

    let saved_rsp0 = hal::arch::x86_64::syscall::SAIOS_SYSCALL_RSP0
        .load(core::sync::atomic::Ordering::Acquire);
    hal::arch::x86_64::tss::set_rsp0(child_rsp0);
    hal::arch::x86_64::syscall::set_kernel_rsp0(child_rsp0);
    set_current_user_rsp0(child_rsp0);

    // Enter ring 3 as the child (fork returns 0 in rax).
    crate::kernel::fault::begin_user_exec(child_pid);
    let saved_fs_base = hal::arch::x86_64::msr::rdmsr(crate::kernel::syscall::IA32_FS_BASE);
    if let Some(fs_base) = data.child_fs_base {
        hal::arch::x86_64::msr::wrmsr(crate::kernel::syscall::IA32_FS_BASE, fs_base);
    }
    let _returned = unsafe {
        hal::arch::x86_64::seed_support::enter_user_mode_from_frame(&frame)
    };
    hal::arch::x86_64::msr::wrmsr(crate::kernel::syscall::IA32_FS_BASE, saved_fs_base);
    crate::kernel::fault::end_user_exec();

    // Restore the outer kernel transition stack before any further kernel work.
    hal::arch::x86_64::tss::set_rsp0(saved_rsp0);
    hal::arch::x86_64::syscall::set_kernel_rsp0(saved_rsp0);
    set_current_user_rsp0(saved_rsp0);
    drop(child_kstack);

    // The child has exited (exit syscall already marked it via exit_quiet /
    // linux_exit_now).  Ensure the process record is exited just in case.
    if crate::kernel::process::record(child_pid)
        .map(|r| r.state != crate::kernel::process::ProcessState::Exited)
        .unwrap_or(false)
    {
        let _ = crate::kernel::process::exit_quiet(child_pid, -1);
    }

    // Unblock the parent thread that was blocked waiting for this pid.
    unblock_waiters_for_pid(child_pid);
}

/// Spawn a new kernel thread to execute the child side of a `fork`.
/// The parent (`parent_thread_id`) must call `block_current_waiting_for_pid`
/// immediately after.  Returns the new kernel thread's id.
pub fn spawn_user_child_thread(
    child_pid:        u64,
    parent_thread_id: ThreadId,
    child_frame:      UserSyscallFrame,
    child_fs_base:    Option<u64>,
) -> ThreadId {
    interrupt::without_interrupts(|| {
        // Store spawn data for child_exec_entry to read.
        unsafe {
            *PENDING_CHILD_SPAWN.get() = Some(PendingChildSpawn {
                child_pid,
                parent_thread_id,
                child_frame,
                child_fs_base,
            });
        }

        let scheduler = scheduler_mut().expect("scheduler not initialized");

        // Allocate a dedicated kernel transition stack for the child.
        let mut user_stack = vec![0u8; USER_PROCESS_KERNEL_STACK_SIZE].into_boxed_slice();
        let user_rsp0 = user_stack.as_mut_ptr() as u64
            + USER_PROCESS_KERNEL_STACK_SIZE as u64;
        let thread_id = scheduler.spawn_internal_with_user_stack(
            child_exec_entry,
            Some(user_stack),
            user_rsp0,
        );
        // `spawn_internal_with_user_stack` seeds `saved_cr3` with the plain
        // kernel root (`vmm::stats().cr3`), correct for an ordinary kernel
        // thread but wrong here: a forked child must resume sharing the
        // parent's address space (isolated per-process root included), not
        // the kernel's own root. Without this override, `do_schedule()`
        // switches the child onto the kernel root on its first-ever
        // schedule, so its first instruction fetch after vfork/fork returns
        // executes user code under a table where that range is mapped
        // supervisor-only -- a `#PF` (present + user + instruction-fetch)
        // that looks exactly like a bad ELF mapping but is really just
        // resuming under the wrong CR3 entirely.
        let parent_cr3 = hal::arch::paging::read_cr3() & crate::vmm::ADDR_MASK;
        if let Some(record) = scheduler.threads.last_mut() {
            record.saved_active_exec_pid = Some(child_pid);
            record.saved_cr3 = parent_cr3;
        }
        thread_id
    })
}

fn ensure_init_script() {
    let _ = crate::saifs::mkdir("/system");
    let _ = crate::saifs::mkdir("/sbin");
    let _ = crate::saifs::touch("/system/init");
    let _ = crate::saifs::touch("/sbin/init");
    let script = b"# SAIOS init script\nsetenv HOSTNAME saios\nalias ll ls\n";
    let _ = crate::vfs::write_path("/system/init", script);
    let _ = crate::vfs::write_path("/sbin/init", script);
    hal::arch::x86_64::console::_print(format_args!(
        "kernel: init script written to /system/init and /sbin/init\n"
    ));
}

/// Prepares the default userland filesystem/session environment.
pub fn prepare_default_user_session() -> Result<(), &'static str> {
    crate::console::clear();
    crate::console::println!("{}", crate::version::PRODUCT_BANNER);
    crate::console::println!("UEFI Boot");
    crate::console::println!("Initializing user session...");
    crate::console::println!("UTF framebuffer: Cafe Ω α あ ┌─┐ █");
    crate::console::newline();

    crate::object_manager::init();
    crate::saifs::init();
    hal::arch::x86_64::console::_print(format_args!(
        "kernel: saifs initialized\n"
    ));
    crate::kernel::package_image::mount_default()?;
    hal::arch::x86_64::console::_print(format_args!(
        "kernel: default package image mounted\n"
    ));

    ensure_init_script();
    crate::kernel::object::init();
    hal::arch::x86_64::console::_print(format_args!(
        "kernel: object registry initialized\n"
    ));
    Ok(())
}

fn default_user_session_entry() {
    crate::kernel::init_runtime::boot_to_login_shell();
}

/// Starts the default user shell as a scheduler-owned user session.
pub fn start_default_user_session() -> Result<(), &'static str> {
    if USER_SESSION_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }

    let _ = spawn(default_user_session_entry);
    yield_now();
    Ok(())
}

/// Called by the timer interrupt handler to advance scheduling state.
pub fn on_timer_tick(current_tick: u64) {
    interrupt::without_interrupts(|| {
        let scheduler = match scheduler_mut() {
            Some(s) if s.initialized => s,
            _ => return,
        };

        scheduler.wake_due(current_tick);

        scheduler.ticks_since_switch = scheduler.ticks_since_switch.saturating_add(1);
        if scheduler.ticks_since_switch >= scheduler.quantum_ticks {
            scheduler.needs_reschedule = true;
        }
    });
}

/// Yields the CPU if the current thread's quantum has expired.
pub fn maybe_preempt() {
    let should_reschedule = interrupt::without_interrupts(|| {
        let scheduler = match scheduler_ref() {
            Some(s) if s.initialized => s,
            _ => return false,
        };
        scheduler.needs_reschedule
    });

    if should_reschedule {
        yield_now();
    }
}

/// Voluntarily yields the CPU to the next runnable thread.
pub fn yield_now() {
    interrupt::without_interrupts(do_schedule);
}

/// Blocks the current thread until `target_tick` is reached.
pub fn yield_until_tick(target_tick: u64) {
    loop {
        let should_block = interrupt::without_interrupts(|| {
            let scheduler = match scheduler_mut() {
                Some(s) if s.initialized => s,
                _ => return false,
            };

            if scheduler.current == scheduler.idle {
                return false;
            }

            if crate::timer::ticks() >= target_tick {
                return false;
            }

            let current_idx = scheduler.current;
            if scheduler.threads[current_idx].thread.state != ThreadState::Running {
                return false;
            }

            scheduler.threads[current_idx].thread.state = ThreadState::Sleeping;
            scheduler.queue_sleep(current_idx, target_tick);
            true
        });

        if !should_block {
            break;
        }

        yield_now();

        if crate::timer::ticks() >= target_tick {
            break;
        }
    }
}

/// Sleeps the current thread for `tick_delta` scheduler ticks.
pub fn sleep_ticks(tick_delta: u64) {
    if tick_delta == 0 {
        return;
    }

    let target = crate::timer::ticks().saturating_add(tick_delta);

    let initialized =
        interrupt::without_interrupts(|| matches!(scheduler_ref(), Some(s) if s.initialized));

    if !initialized {
        while crate::timer::ticks() < target {
            core::hint::spin_loop();
        }
        return;
    }

    yield_until_tick(target);
}

/// Returns a snapshot of all threads known to the scheduler.
pub fn threads() -> Vec<ThreadInfo> {
    interrupt::without_interrupts(|| {
        let scheduler = match scheduler_ref() {
            Some(s) if s.initialized => s,
            _ => return Vec::new(),
        };

        scheduler
            .threads
            .iter()
            .map(|t| ThreadInfo {
                id: t.thread.id,
                state: t.thread.state,
            })
            .collect()
    })
}

/// Verifies the scheduler and returns a report.
pub fn verify() -> crate::kernel::testing::report::VerifyReport {
    let snapshot = threads();
    let mut checks = Vec::new();

    checks.push(if !snapshot.is_empty() {
        crate::kernel::testing::report::VerifyCheck::pass("Thread table", "thread records present")
    } else {
        crate::kernel::testing::report::VerifyCheck::fail("Thread table", "no threads registered")
    });

    let running = snapshot
        .iter()
        .filter(|t| t.state == ThreadState::Running)
        .count();
    checks.push(if running == 1 {
        crate::kernel::testing::report::VerifyCheck::pass(
            "Running thread",
            "exactly one running thread",
        )
    } else {
        crate::kernel::testing::report::VerifyCheck::fail(
            "Running thread",
            "invalid running thread count",
        )
    });

    let mut unique = true;
    for i in 0..snapshot.len() {
        for j in (i + 1)..snapshot.len() {
            if snapshot[i].id == snapshot[j].id {
                unique = false;
            }
        }
    }

    checks.push(if unique {
        crate::kernel::testing::report::VerifyCheck::pass("Thread ids", "all thread ids are unique")
    } else {
        crate::kernel::testing::report::VerifyCheck::fail("Thread ids", "duplicate thread id found")
    });

    crate::kernel::testing::report::VerifyReport {
        target: "scheduler",
        checks,
    }
}
