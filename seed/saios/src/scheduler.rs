use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::global_asm;

use hal::arch::x86_64::interrupt;
use hal::arch::x86_64::sync::StaticCell;

pub type ThreadId = u64;
pub type VirtAddr = u64;

type ThreadEntry = fn();

const STACK_SIZE: usize = 64 * 1024;
const DEFAULT_QUANTUM_TICKS: u64 = 10;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Sleeping,
    Blocked,
    Dead,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
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
pub struct Thread {
    pub id: ThreadId,
    pub state: ThreadState,
    pub context: CpuContext,
    pub stack_top: VirtAddr,
}

#[derive(Debug, Copy, Clone)]
pub struct ThreadInfo {
    pub id: ThreadId,
    pub state: ThreadState,
}

struct ThreadRecord {
    thread: Thread,
    entry: ThreadEntry,
    _stack: Box<[u8]>,
}

struct Scheduler {
    initialized: bool,
    threads: Vec<ThreadRecord>,
    run_queue: VecDeque<usize>,
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
        });

        let idx = self.threads.len() - 1;
        self.run_queue.push_back(idx);
        thread.id
    }
}

static SCHEDULER: StaticCell<Option<Scheduler>> = StaticCell::new(None);

global_asm!(
    ".global saios_context_switch",
    "saios_context_switch:",
    "mov [rdi + 0x00], rsp",
    "mov [rdi + 0x08], rbx",
    "mov [rdi + 0x10], rbp",
    "mov [rdi + 0x18], r12",
    "mov [rdi + 0x20], r13",
    "mov [rdi + 0x28], r14",
    "mov [rdi + 0x30], r15",
    "mov rsp, [rsi + 0x00]",
    "mov rbx, [rsi + 0x08]",
    "mov rbp, [rsi + 0x10]",
    "mov r12, [rsi + 0x18]",
    "mov r13, [rsi + 0x20]",
    "mov r14, [rsi + 0x28]",
    "mov r15, [rsi + 0x30]",
    "ret",
);

unsafe extern "C" {
    fn saios_context_switch(old: *mut CpuContext, new: *const CpuContext);
}

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
    while let Some(idx) = scheduler.run_queue.pop_front() {
        if scheduler.threads[idx].thread.state == ThreadState::Ready {
            return idx;
        }
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

    if scheduler.threads[old_idx].thread.state == ThreadState::Running && old_idx != scheduler.idle {
        scheduler.threads[old_idx].thread.state = ThreadState::Ready;
        scheduler.run_queue.push_back(old_idx);
    }

    scheduler.threads[next_idx].thread.state = ThreadState::Running;
    scheduler.current = next_idx;
    scheduler.needs_reschedule = false;
    scheduler.ticks_since_switch = 0;

    let old_ctx = &mut scheduler.threads[old_idx].thread.context as *mut CpuContext;
    let new_ctx = &scheduler.threads[next_idx].thread.context as *const CpuContext;

    unsafe {
        saios_context_switch(old_ctx, new_ctx);
    }
}

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

pub fn spawn(entry: ThreadEntry) -> ThreadId {
    interrupt::without_interrupts(|| {
        let scheduler = scheduler_mut().expect("scheduler not initialized");
        scheduler.spawn_internal(entry)
    })
}

pub fn on_timer_tick() {
    interrupt::without_interrupts(|| {
        let scheduler = match scheduler_mut() {
            Some(s) if s.initialized => s,
            _ => return,
        };

        scheduler.ticks_since_switch = scheduler.ticks_since_switch.saturating_add(1);
        if scheduler.ticks_since_switch >= scheduler.quantum_ticks {
            scheduler.needs_reschedule = true;
        }
    });
}

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

pub fn yield_now() {
    interrupt::without_interrupts(do_schedule);
}

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
