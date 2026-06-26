//! x86_64 interrupt infrastructure: IDT, PIC/PIT setup, and exception/IRQ handlers.

use crate::gdt;
use crate::ipc::signal as ipc_signal;
use crate::println;
use crate::process::Process;
use crate::process::USER_STACK_SIZE;
use crate::process::USER_STACK_TOP;
use crate::process::signal;
use crate::tty;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

// -- Diagnostic IRQ counters (check these when input freezes) --------------
// Read via `irqinfo` shell command.  If KB_IRQS / MOUSE_IRQS keep climbing
// while input is frozen → IRQs are alive, bug is in event loop.
// If they stop → interrupts disabled or IRQ masked.
pub static KB_IRQS: AtomicU64 = AtomicU64::new(0);
pub static MOUSE_IRQS: AtomicU64 = AtomicU64::new(0);

pub struct PerCpuIrqCounter([AtomicU64; crate::process::table::MAX_CPUS]);

impl PerCpuIrqCounter {
    pub const fn new() -> Self {
        Self([const { AtomicU64::new(0) }; crate::process::table::MAX_CPUS])
    }

    pub fn fetch_add(&self, value: u64, order: Ordering) -> u64 {
        self.0[crate::process::table::cpu_idx()].fetch_add(value, order)
    }

    pub fn load(&self, order: Ordering) -> u64 {
        self.0.iter().map(|counter| counter.load(order)).sum()
    }
}

pub static TIMER_IRQS: PerCpuIrqCounter = PerCpuIrqCounter::new();

/// Run a closure with interrupts disabled and warn (via serial) if the
/// interrupts-off window exceeds 2 timer ticks (~110 ms at 18 Hz).
#[macro_export]
macro_rules! without_interrupts_checked {
    ($body:expr) => {{
        let _t0 = $crate::shell::commands::boot_ticks();
        let _r = $crate::arch::without_interrupts(|| $body);
        let _dt = $crate::shell::commands::boot_ticks().wrapping_sub(_t0);
        if _dt > 2 {
            $crate::serial_println!("[diag] LONG CLI {} ticks at {}:{}", _dt, file!(), line!());
        }
        _r
    }};
}

// -- PIC (8259) constants ---------------------------------------------------

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;
const PIC_EOI: u8 = 0x20; // end-of-interrupt

// Hardware IRQ vectors start at 0x20 (above CPU exceptions)
const IRQ_OFFSET: u8 = 0x20;
const IRQ_KEYBOARD: u8 = 1;
const IRQ_MOUSE: u8 = 12;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum InterruptIndex {
    Timer = IRQ_OFFSET,
    Keyboard = IRQ_OFFSET + IRQ_KEYBOARD,
    Mouse = IRQ_OFFSET + IRQ_MOUSE,
}

// -- Keyboard scancode ring buffer -----------------------------------------
// F-INT-06: Fixed-size ring buffer replaces VecDeque (no heap allocation in IRQ path).

const SCANCODE_RING_CAP: usize = 256;

struct ScancodeRing {
    buf: [u8; SCANCODE_RING_CAP],
    head: usize, // next read position
    len: usize,  // number of valid entries
}

impl ScancodeRing {
    const fn new() -> Self {
        Self {
            buf: [0u8; SCANCODE_RING_CAP],
            head: 0,
            len: 0,
        }
    }

    fn push(&mut self, sc: u8) -> bool {
        if self.len >= SCANCODE_RING_CAP {
            return false; // full — drop
        }
        let tail = (self.head + self.len) % SCANCODE_RING_CAP;
        self.buf[tail] = sc;
        self.len += 1;
        true
    }

    fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let val = self.buf[self.head];
        self.head = (self.head + 1) % SCANCODE_RING_CAP;
        self.len -= 1;
        Some(val)
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn clear(&mut self) -> usize {
        let dropped = self.len;
        self.head = 0;
        self.len = 0;
        dropped
    }
}

static SCANCODE_QUEUE: Mutex<ScancodeRing> = Mutex::new(ScancodeRing::new());
static KEYBOARD_WAITERS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

#[derive(Clone, Copy)]
struct SleepWaiter {
    pid: u32,
    wake_tick: u64,
}

static SLEEP_WAITERS: Mutex<Vec<SleepWaiter>> = Mutex::new(Vec::new());

/// Called by the shell poll loop — returns the next raw scancode if one arrived.
///
/// Acquired with interrupts disabled: the keyboard IRQ handler locks the same
/// queue, so if an IRQ landed while this (mainline) code held the lock, the
/// handler would spin forever on it — a hard, unrecoverable freeze that takes
/// down the timer and mouse too.  `without_interrupts` closes that window.
pub fn next_scancode() -> Option<u8> {
    crate::arch::without_interrupts(|| SCANCODE_QUEUE.lock().pop())
}

pub fn has_pending_scancode() -> bool {
    crate::arch::without_interrupts(|| !SCANCODE_QUEUE.lock().is_empty())
}

/// Push a raw scancode into the ring.  Used by the keyboard IRQ handler and by
/// the poll-time hardware drain (which recovers bytes whose IRQ edge was lost).
/// Caller must already hold interrupts disabled.
pub fn push_scancode(sc: u8) {
    let mut q = SCANCODE_QUEUE.lock();
    q.push(sc);
}

/// Clear the entire scancode ring.  Called during keyboard reenable to discard
/// any partial or stale scancodes. F-KBD-05: logs dropped count instead of silent loss.
pub fn clear_scancode_queue() {
    crate::arch::without_interrupts(|| {
        let dropped = SCANCODE_QUEUE.lock().clear();
        if dropped > 0 {
            crate::serial_println!("[kbd] clear_scancode_queue: dropped {} scancodes", dropped);
        }
    });
}

fn register_keyboard_waiter(pid: u32) {
    crate::arch::without_interrupts(|| {
        let mut waiters = KEYBOARD_WAITERS.lock();
        if !waiters.contains(&pid) {
            waiters.push(pid);
        }
    });
}

fn unregister_keyboard_waiter(pid: u32) {
    crate::arch::without_interrupts(|| {
        KEYBOARD_WAITERS
            .lock()
            .retain(|&waiter_pid| waiter_pid != pid);
    });
}

fn has_keyboard_waiters() -> bool {
    crate::arch::without_interrupts(|| !KEYBOARD_WAITERS.lock().is_empty())
}

fn register_sleep_waiter(pid: u32, wake_tick: u64) {
    crate::arch::without_interrupts(|| {
        let mut waiters = SLEEP_WAITERS.lock();
        if let Some(waiter) = waiters.iter_mut().find(|waiter| waiter.pid == pid) {
            waiter.wake_tick = wake_tick;
        } else {
            waiters.push(SleepWaiter { pid, wake_tick });
        }
    });
}

fn unregister_sleep_waiter(pid: u32) {
    crate::arch::without_interrupts(|| {
        SLEEP_WAITERS.lock().retain(|waiter| waiter.pid != pid);
    });
}

fn wake_keyboard_waiters() {
    crate::arch::without_interrupts(|| {
        let waiters = KEYBOARD_WAITERS.lock();
        if waiters.is_empty() {
            return;
        }
        let Some(mut table) = crate::process::table::TABLE.try_lock() else {
            return;
        };
        crate::serial_println!("[kbd-pipe] waking {} waiter(s)", waiters.len());
        for &pid in waiters.iter() {
            let _ = crate::process_contract::ProcessContract::wake_pid(
                &mut table,
                pid,
                "keyboard waiter wake",
            );
        }
    });
}

fn wake_expired_sleepers(now_tick: u64) {
    let has_expired = {
        let waiters = SLEEP_WAITERS.lock();
        waiters.iter().any(|waiter| waiter.wake_tick <= now_tick)
    };
    if !has_expired {
        return;
    }

    let Some(mut table) = crate::process::table::TABLE.try_lock() else {
        return;
    };

    let mut waiters = SLEEP_WAITERS.lock();
    waiters.retain(|waiter| {
        if waiter.wake_tick <= now_tick {
            let _ = crate::process_contract::ProcessContract::wake_pid(
                &mut table,
                waiter.pid,
                "sleep waiter wake",
            );
            false
        } else {
            true
        }
    });
}

pub fn block_until_tick(wake_tick: u64) {
    if crate::shell::commands::boot_ticks() >= wake_tick {
        return;
    }

    let Some(pid) = crate::process::current_pid() else {
        while crate::shell::commands::boot_ticks() < wake_tick {
            crate::arch::enable_interrupts();
            crate::arch::halt();
        }
        return;
    };

    register_sleep_waiter(pid, wake_tick);
    if crate::shell::commands::boot_ticks() >= wake_tick {
        unregister_sleep_waiter(pid);
        return;
    }
    crate::process::block_current();
    unregister_sleep_waiter(pid);
}

pub fn wait_for_keyboard_input_until(deadline_tick: Option<u64>) -> bool {
    drain_ps2_keyboard();
    if has_pending_scancode() {
        return true;
    }

    if deadline_tick.is_none() {
        loop {
            drain_ps2_keyboard();
            if has_pending_scancode() {
                return true;
            }
            crate::arch::enable_interrupts();
            crate::arch::halt();
        }
    }

    if let Some(deadline) = deadline_tick
        && deadline.wrapping_sub(crate::shell::commands::boot_ticks()) <= 1
    {
        while crate::shell::commands::boot_ticks() < deadline {
            drain_ps2_keyboard();
            if has_pending_scancode() {
                return true;
            }
            crate::arch::enable_interrupts();
            crate::arch::nop();
        }
        drain_ps2_keyboard();
        return has_pending_scancode();
    }

    let Some(pid) = crate::process::current_pid() else {
        if let Some(deadline) = deadline_tick {
            while crate::shell::commands::boot_ticks() < deadline && !has_pending_scancode() {
                crate::arch::enable_interrupts();
                crate::arch::halt();
            }
            return has_pending_scancode();
        }
        crate::arch::enable_interrupts();
        crate::arch::halt();
        return has_pending_scancode();
    };

    loop {
        drain_ps2_keyboard();
        if has_pending_scancode() {
            return true;
        }
        if deadline_tick.is_some_and(|deadline| crate::shell::commands::boot_ticks() >= deadline) {
            return false;
        }

        register_keyboard_waiter(pid);
        if let Some(deadline) = deadline_tick {
            register_sleep_waiter(pid, deadline);
        }

        drain_ps2_keyboard();
        let ready = has_pending_scancode();
        let timed_out =
            deadline_tick.is_some_and(|deadline| crate::shell::commands::boot_ticks() >= deadline);
        if ready || timed_out {
            unregister_keyboard_waiter(pid);
            unregister_sleep_waiter(pid);
            return ready;
        }

        crate::process::block_current();
        unregister_keyboard_waiter(pid);
        unregister_sleep_waiter(pid);
    }
}

/// Drain every byte currently in the 8042 keyboard output buffer into the
/// scancode queue.  Guarded on the status register's output-buffer-full (bit 0)
/// and AUX/mouse (bit 5) flags, so it never reads when empty (no bogus 0 bytes)
/// and never steals mouse bytes.  Safe to call from both the keyboard IRQ and
/// the timer IRQ: the OBF guard makes the two paths cooperate instead of
/// double-reading.  This is the heart of "keyboard always live": the timer polls
/// it every tick, so even a lost IRQ1 edge (byte arrived while IRQ was masked)
/// is recovered within ~one tick and input never permanently dies.
pub fn drain_ps2_keyboard() {
    let pushed = crate::arch::without_interrupts(|| {
        let mut status: Port<u8> = Port::new(0x64);
        let mut data: Port<u8> = Port::new(0x60);
        let mut pushed = 0usize;
        for _ in 0..32 {
            let st = unsafe { status.read() };
            if st & 0x01 == 0 {
                break;
            } // output buffer empty
            let byte = unsafe { data.read() };
            // Route by the AUX bit (5): mouse bytes to the mouse assembler, keyboard
            // bytes to the scancode queue.  We must consume EVERY byte — leaving a
            // mouse byte stuck at the head keeps the 8042 output buffer full, which
            // stops the keyboard IRQ from ever re-asserting (keyboard goes dead).
            if st & 0x20 != 0 {
                crate::driver::mouse::handle_byte(byte);
            } else {
                push_scancode(byte);
                pushed += 1;
            }
        }
        pushed
    });
    if pushed != 0 {
        crate::serial_println!(
            "[kbd-pipe] drain pushed={} irq_count={}",
            pushed,
            KB_IRQS.load(Ordering::Relaxed)
        );
    }
    if has_pending_scancode() && has_keyboard_waiters() {
        wake_keyboard_waiters();
    }
}

// -- IDT -------------------------------------------------------------------

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        // CPU exceptions
        idt.divide_error.set_handler_fn(divide_error_handler);
        idt.debug.set_handler_fn(debug_handler);
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.general_protection_fault.set_handler_fn(general_protection_fault_handler);
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        idt.alignment_check.set_handler_fn(alignment_check_handler);
        idt.overflow.set_handler_fn(overflow_handler);

        // Hardware IRQs
        idt[InterruptIndex::Timer    as usize].set_handler_fn(timer_handler);
        idt[InterruptIndex::Keyboard as usize].set_handler_fn(keyboard_handler);
        idt[InterruptIndex::Mouse    as usize].set_handler_fn(mouse_handler);

        // Per-CPU LAPIC timer (APs) + LAPIC spurious vector.
        idt[crate::smp::LAPIC_TIMER_VECTOR as usize].set_handler_fn(lapic_timer_handler);
        idt[crate::smp::LAPIC_SPURIOUS_VECTOR as usize].set_handler_fn(lapic_spurious_handler);

        idt
    };
}

/// Per-CPU LAPIC timer IRQ: full maintenance on BSP, schedule on all CPUs.
/// Constitutional requirement (DOC-09): all timer-driven maintenance must
/// function regardless of which timer source is active on a given CPU.
extern "x86-interrupt" fn lapic_timer_handler(frame: InterruptStackFrame) {
    crate::interrupt_contract::InterruptContract::record_irq_entry(crate::smp::LAPIC_TIMER_VECTOR);
    crate::OBS_COUNTER!(
        crate::kds::KdsSubsystem::Interrupt,
        crate::kds::KdsMetricId::Interrupts,
        1,
    );
    crate::diag::watchdog::note_cpu_heartbeat();
    trace_timer_preempt(&frame);

    let cpu = crate::process::table::cpu_idx();

    // BSP (cpu 0) performs system-wide maintenance that is traditionally
    // PIT-only.  Running it here guarantees maintenance survives even if
    // PIT is ever masked, delayed, or reconfigured.
    if cpu == 0 {
        crate::shell::commands::tick();
        let now_tick = crate::shell::commands::boot_ticks();
        crate::graphics::console::tick_blink(now_tick);
        drain_ps2_keyboard();
        wake_expired_sleepers(now_tick);
        // IRQ storm detection: sample once per second (~18 ticks at PIT rate).
        if now_tick.is_multiple_of(18) {
            crate::interrupt_contract::irq_storm_tick();
        }
    }

    // All CPUs: heartbeat liveness and watchdog forward-progress check.
    crate::diag::heartbeat::tick();
    crate::diag::watchdog::tick();

    // DOC-08 §Failure Modes: "Dead process remaining on-CPU triggers Red Ring
    // on next timer interrupt." Check current process state.
    {
        use crate::process::ProcessState;
        if let Some(state) = crate::process::table::current_process_state(cpu)
            && matches!(state, ProcessState::Dead | ProcessState::Zombie)
        {
            crate::reliability_contract::ReliabilityContract::enter_red_ring(
                crate::reliability_contract::RedRingEvidence {
                    cause: crate::reliability_contract::RedRingCause::DeadOnCpu,
                    evidence_event_id: 0,
                    invariant_id: cpu as u64,
                    detail: 0,
                },
            );
        }
    }

    crate::smp::lapic_eoi();
    if can_preempt_current_cpu() {
        crate::process::scheduler::schedule_from("lapic_timer");
    }
}

/// LAPIC spurious interrupt — just acknowledge.
extern "x86-interrupt" fn lapic_spurious_handler(_frame: InterruptStackFrame) {
    crate::smp::lapic_eoi();
}

extern "x86-interrupt" fn debug_handler(stack_frame: InterruptStackFrame) {
    let cpl = stack_frame.code_segment & 3;
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    let flags = stack_frame.cpu_flags;
    if cpl == 3 {
        let pid = crate::process::current_pid().unwrap_or(0);
        let context = crate::debug_contract::DebugTrapContext {
            pid,
            rip,
            rsp,
            rflags: flags,
        };
        if let Err(reason) = crate::debug_contract::DebugContract::validate_user_debug_trap(context)
        {
            crate::debug_contract::DebugContract::dump_user_debug_trap(context, reason);
            panic!("[debug-contract] {}", reason);
        }
        let _ = crate::process::with_current_process_mut(|proc| {
            proc.rflags = crate::debug_contract::DebugContract::sanitize_user_rflags(proc.rflags);
        });
        crate::serial_println!(
            "[#DB] user first-step pid={} rip={:#x} rsp={:#x} rflags={:#x}",
            pid,
            rip,
            rsp,
            flags
        );
    } else {
        crate::serial_println!(
            "[#DB] kernel debug rip={:#x} rsp={:#x} rflags={:#x}",
            rip,
            rsp,
            flags
        );
    }
    unsafe {
        core::arch::asm!("mov dr7, {}", in(reg) 0u64, options(nostack, preserves_flags));
    }
}

pub fn init_idt() {
    IDT.load();
    init_pics();
    crate::arch::enable_interrupts();
}

/// Load the shared IDT on an application processor (no PIC re-init — the PIC is
/// owned by the BSP).  The IDT is read-only after setup, so sharing it is safe.
pub fn load_idt_on_ap() {
    IDT.load();
}

/// Remap and unmask PIC1/PIC2 so IRQ 0-15 map to vectors 0x20-0x2F.
fn init_pics() {
    unsafe {
        // ICW1: start init sequence, edge-triggered
        Port::<u8>::new(PIC1_CMD).write(0x11);
        Port::<u8>::new(PIC2_CMD).write(0x11);
        // ICW2: vector offsets
        Port::<u8>::new(PIC1_DATA).write(IRQ_OFFSET);
        Port::<u8>::new(PIC2_DATA).write(IRQ_OFFSET + 8);
        // ICW3: cascade wiring
        Port::<u8>::new(PIC1_DATA).write(4); // IRQ2 → slave
        Port::<u8>::new(PIC2_DATA).write(2); // cascade identity
        // ICW4: 8086 mode
        Port::<u8>::new(PIC1_DATA).write(0x01);
        Port::<u8>::new(PIC2_DATA).write(0x01);
        // Program PIT channel 0 for a 100 Hz periodic tick (10 ms) instead of the
        // BIOS default ~18.2 Hz — finer timing/scheduling granularity.  Divisor =
        // 1_193_182 / 100 = 11932.  Command 0x36 = ch0, lobyte/hibyte, mode 3.
        const PIT_DIV: u16 = 11932;
        Port::<u8>::new(0x43).write(0x36);
        Port::<u8>::new(0x40).write((PIT_DIV & 0xFF) as u8);
        Port::<u8>::new(0x40).write((PIT_DIV >> 8) as u8);

        // Unmask: timer (IRQ0), keyboard (IRQ1), cascade (IRQ2), mouse (IRQ12)
        // PIC1 mask: 1 = masked. We unmask 0 and 1 → 0b1111_1100
        Port::<u8>::new(PIC1_DATA).write(0b1111_1100);
        // PIC2 mask: IRQ12 is bit 4 of PIC2 → unmask it → 0b1110_1111
        Port::<u8>::new(PIC2_DATA).write(0b1110_1111);
    }
}

fn eoi(irq: u8) {
    crate::interrupt_contract::InterruptContract::record_irq_eoi(irq);
    if crate::arch::x86_64::ioapic::is_active() {
        // IOAPIC mode: acknowledge via LAPIC EOI register.
        crate::smp::lapic_eoi();
    } else {
        // Legacy PIC mode.
        unsafe {
            if irq >= 8 {
                Port::<u8>::new(PIC2_CMD).write(PIC_EOI);
            }
            Port::<u8>::new(PIC1_CMD).write(PIC_EOI);
        }
    }
}

// -- Interrupt handlers -----------------------------------------------------

/// Handle a user-space exception: dump fault info, terminate the process,
/// and return to shell. Never panics the kernel.
///
/// Called by exception handlers when they detect a CPL=3 (user-mode) fault.
extern "C" fn handle_user_exception(
    label: *const u8, // C string (null-terminated)
    label_len: usize,
    frame: &InterruptStackFrame,
    error_code: u64,
    signal: i64,
    reason: *const u8, // C string (null-terminated)
    reason_len: usize,
) {
    // Convert raw pointers to Rust strings (we know these are null-terminated)
    let label_str =
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(label, label_len)) };
    let reason_str =
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(reason, reason_len)) };
    let cs = frame.code_segment;
    let cpl = cs & 3;
    let rip = frame.instruction_pointer.as_u64();
    let sig_num = signal.unsigned_abs() as u32;
    let fault_kind = match label_str {
        "PF" => crate::interrupt_contract::InterruptKind::PageFault,
        "GP" => crate::interrupt_contract::InterruptKind::GeneralProtectionFault,
        _ => crate::interrupt_contract::InterruptKind::Other(0),
    };

    // The faulting process is the process table's current task for this CPU.
    // CURRENT is only a syscall-facing shadow and can be stale in interrupts.
    let (pid, name, fault_state) = {
        let table = crate::process::table::TABLE.lock();
        table
            .current_ref()
            .map(|p| (p.pid, p.name.clone(), p.state().clone()))
            .unwrap_or((
                0,
                alloc::string::String::from("<kernel>"),
                crate::process::ProcessState::Running,
            ))
    };

    if crate::process::table::trace_pid(pid) {
        if label_str == "PF" {
            crate::println!("[pagefault] pid={}", pid);
        } else if label_str == "GP" {
            crate::println!("[gpfault] pid={}", pid);
        } else {
            crate::println!("[exception] pid={}", pid);
        }
    }

    crate::interrupt_contract::InterruptContract::record_fault_terminate(
        fault_kind,
        [
            rip,
            frame.stack_pointer.as_u64(),
            error_code,
            sig_num as u64,
        ],
    );
    crate::OBS_COUNTER!(
        crate::kds::KdsSubsystem::Interrupt,
        crate::kds::KdsMetricId::Faults,
        1,
    );
    crate::serial_println!("[{}] fault pid={}", label_str, pid);
    crate::serial_println!("[{}] fault state={:?}", label_str, fault_state);
    crate::serial_println!("[{}] pid={} {:?} -> Zombie", label_str, pid, fault_state);

    crate::serial_println!(
        "[{}] {} pid={} name='{}' rip={:#x} err={:#x} cpl={}",
        label_str,
        reason_str,
        pid,
        name,
        rip,
        error_code,
        cpl
    );

    // Print the fault dump for debugging
    crate::diag::fault::dump(label_str, frame, error_code);
    dump_user_control_flow_fault(label_str, frame);

    // Diagnostic info: how did we get here? (syscall vs interrupt context)
    // This helps identify if we're in interrupt context (where terminate/klongjmp is risky)
    let cr3 = {
        use crate::memory::paging::active_pml4;
        active_pml4()
    };
    crate::serial_println!(
        "[{}] terminate source: interrupt (exception handler)",
        label_str
    );
    crate::serial_println!("[{}] current pid: {}", label_str, pid);
    crate::serial_println!("[{}] current CR3: {:#x}", label_str, cr3);
    crate::serial_println!(
        "[{}] IF flag: {}",
        label_str,
        if frame.cpu_flags & 0x200 != 0 {
            "set"
        } else {
            "clear"
        }
    );
    crate::serial_println!("[{}] termination stack frame dump:", label_str);
    crate::serial_println!(
        "[{}]   RSP={:#x} RIP={:#x} RFLAGS={:#x}",
        label_str,
        frame.stack_pointer.as_u64(),
        rip,
        frame.cpu_flags
    );

    // First, try to deliver the signal to a registered handler
    if let Some((f, mask, restorer)) = ipc_signal::has_handler_for_pid(pid, sig_num) {
        // There's a signal handler - try to deliver the signal with a frame
        crate::serial_println!(
            "[{}] signal handler found for sig {} at {:#x}, delivering...",
            label_str,
            sig_num,
            f
        );
        let (cur_rip, cur_rsp, cur_rflags) = (
            frame.instruction_pointer.as_u64(),
            frame.stack_pointer.as_u64(),
            frame.cpu_flags,
        );

        let old_mask = crate::process::with_process_mut_by_pid(pid, |p| {
            let old_mask = p.signals.blocked;
            p.signals.blocked = old_mask | mask | (1u64 << sig_num);
            old_mask
        })
        .unwrap_or(0);
        let (new_rip, new_rsp) =
            signal::deliver(sig_num, f, restorer, old_mask, cur_rip, cur_rsp, cur_rflags);
        if new_rip != cur_rip {
            // Signal was delivered successfully - update process state
            if crate::process::with_process_mut_by_pid(pid, |p| {
                p.rip = new_rip;
                p.rsp = new_rsp;
                crate::serial_println!(
                    "[{}] signal {} delivered to handler {:#x}",
                    label_str,
                    sig_num,
                    f
                );
            })
            .is_some()
            {
                crate::interrupt_contract::InterruptContract::record_fault_recover(
                    fault_kind,
                    [rip, new_rip, new_rsp, sig_num as u64],
                );
                // Signal was delivered, continue running the signal handler.
                // The process will fault or exit normally later, at which point
                // the scheduler will clean it up.
                return;
            }
        }
        let _ = crate::process::with_process_mut_by_pid(pid, |p| {
            p.signals.blocked = old_mask;
        });
        // Failed to deliver, fall through to terminate
        crate::serial_println!(
            "[{}] failed to deliver signal {}, terminating",
            label_str,
            sig_num
        );
    } else {
        crate::serial_println!(
            "[{}] no handler for signal {}, terminating",
            label_str,
            sig_num
        );
    }

    // Kill only the user process, not the kernel
    crate::serial_println!(
        "[{}] Terminating user process (signal={})",
        label_str,
        signal
    );

    // Mark the faulted process as Zombie before scheduling. The process remains
    // in the table until finish_switch() publishes its exit metadata.
    let faulted_pid = if pid != 0 {
        if crate::process_contract::ProcessContract::request_exit(
            crate::process_contract::ProcessExitRequest {
                pid,
                code: signal,
                reason: crate::process_contract::ProcessExitReason::FatalSignal,
                tag: "fault_exit",
            },
        )
        .is_some()
        {
            if crate::diag::diag_proc_on() {
                crate::serial_println!(
                    "[{}] [fault] pid={} state={:?} -> Zombie",
                    label_str,
                    pid,
                    fault_state
                );
            }

            if crate::diag::diag_proc_on() {
                crate::println!(
                    "[{}] [fault] pid={} waiting for scheduler to clean up",
                    label_str,
                    pid
                );
            }
            Some(pid)
        } else {
            None
        }
    } else {
        None
    };

    // EOI the interrupt before returning
    eoi(0); // We don't know the exact IRQ, but this is safe

    // Non-returning exit handoff publishes the zombie before choosing the next process.
    if faulted_pid.is_some() {
        crate::process::scheduler::schedule_handoff_no_save_from("fault_exit");
    }
}

fn dump_user_control_flow_fault(label: &str, frame: &InterruptStackFrame) {
    let rip = frame.instruction_pointer.as_u64();
    let rsp = frame.stack_pointer.as_u64();
    let stack_bottom = USER_STACK_TOP.saturating_sub(USER_STACK_SIZE as u64);
    let rip_in_stack = is_user_stack_address(rip);
    let rsp_in_stack = is_user_stack_address(rsp);
    let (saved_rip, saved_rsp, saved_rflags) = crate::arch::syscall::saved_user_syscall_site();

    crate::serial_println!(
        "[{}] user-cfi rip_in_stack={} rsp_in_stack={} stack=[{:#x}..{:#x})",
        label,
        rip_in_stack,
        rsp_in_stack,
        stack_bottom,
        USER_STACK_TOP
    );
    crate::serial_println!(
        "[{}] user-cfi saved_syscall rip={:#x} rsp={:#x} rflags={:#x}",
        label,
        saved_rip,
        saved_rsp,
        saved_rflags
    );
    if rip_in_stack {
        crate::serial_println!(
            "[{}] user-cfi violation: instruction pointer is executing from user stack",
            label
        );
        dump_user_qwords(label, "rip", rip.saturating_sub(0x20), 8);
    }
    dump_user_qwords(label, "rsp", rsp.saturating_sub(0x20), 12);
}

fn is_user_stack_address(addr: u64) -> bool {
    let stack_bottom = USER_STACK_TOP.saturating_sub(USER_STACK_SIZE as u64);
    addr >= stack_bottom && addr < USER_STACK_TOP
}

fn dump_user_qwords(label: &str, base: &str, start: u64, count: usize) {
    for i in 0..count {
        let addr = start + (i as u64 * 8);
        match read_user_u64_safe(addr) {
            Some(value) => crate::serial_println!(
                "[{}] user-stack {}[{:#x}] = {:#018x}",
                label,
                base,
                addr,
                value
            ),
            None => {
                crate::serial_println!("[{}] user-stack {}[{:#x}] = <unmapped>", label, base, addr)
            }
        }
    }
}

fn read_user_u64_safe(virt: u64) -> Option<u64> {
    let mut bytes = [0u8; 8];
    for (offset, byte) in bytes.iter_mut().enumerate() {
        *byte = read_user_byte_safe(virt + offset as u64)?;
    }
    Some(u64::from_le_bytes(bytes))
}

fn read_user_byte_safe(virt: u64) -> Option<u8> {
    let pml4 = crate::memory::paging::active_pml4();
    let (phys, flags) = crate::memory::paging::translate_entry_in(pml4, virt)?;
    if flags & crate::memory::paging::PTE_PRESENT == 0 {
        return None;
    }
    Some(unsafe { *((phys + (virt & 0xFFF)) as *const u8) })
}

fn should_contain_user_fault(cpl: u64) -> bool {
    if cpl == 3 {
        return true;
    }

    if !crate::process::USER_MODE_ACTIVE.load(Ordering::Relaxed) {
        return false;
    }

    crate::process::table::TABLE
        .try_lock()
        .is_some_and(|table| table.current_ref().is_some())
}

extern "x86-interrupt" fn timer_handler(frame: InterruptStackFrame) {
    crate::interrupt_contract::InterruptContract::record_irq_entry(IRQ_OFFSET);
    TIMER_IRQS.fetch_add(1, Ordering::Relaxed);
    crate::OBS_COUNTER!(
        crate::kds::KdsSubsystem::Interrupt,
        crate::kds::KdsMetricId::Interrupts,
        1,
    );
    crate::diag::watchdog::note_cpu_heartbeat();
    trace_timer_preempt(&frame);
    crate::shell::commands::tick();
    let now_tick = crate::shell::commands::boot_ticks();
    // Flip the blink phase (atomic only — the shell loop does the actual draw).
    crate::graphics::console::tick_blink(now_tick);
    // Safety-net keyboard drain: recovers any byte whose IRQ1 edge was lost
    // (arrived while interrupts were masked during a long command), so the
    // keyboard stays live no matter what is running on the CPU.
    drain_ps2_keyboard();
    wake_expired_sleepers(now_tick);
    // Heartbeat (1 Hz, throttled inside) and forward-progress watchdog.  Both
    // are no-ops in the common case — heartbeat only fires every 100th tick,
    // watchdog only fires on real stalls.  They run before the EOI / context
    // switch so a stall in the scheduler itself is still observable.
    crate::diag::heartbeat::tick();
    crate::diag::watchdog::tick();
    // EOI BEFORE the (possible) context switch: scheduler::tick() may switch to
    // a freshly-spawned kernel thread that resumes in kthread_trampoline and
    // never returns here, so the PIC must already be acknowledged.  The CPU
    // keeps interrupts masked (IF=0) until iretq / the trampoline's sti, so no
    // re-entrancy occurs before the switch completes.
    eoi(0);
    // F-INT-05: Schedule when this CPU has a canonical current task. PIT can
    // fire during early boot before scheduler ownership is published; those
    // ticks still drive time/watchdog work above, but must not preempt from a
    // current_pid=0 context.
    if can_preempt_current_cpu() {
        crate::process::scheduler::tick();
    }
}

fn trace_timer_preempt(frame: &InterruptStackFrame) {
    let Some(table) = crate::process::table::TABLE.try_lock() else {
        return;
    };
    let pid = table.current_pid();
    if crate::diag::diag_proc_on() {
        crate::serial_println!(
            "[preempt] pid={} rip={:#x} rsp={:#x}",
            pid,
            frame.instruction_pointer.as_u64(),
            frame.stack_pointer.as_u64()
        );
    }
}

fn can_preempt_current_cpu() -> bool {
    crate::process::table::TABLE
        .try_lock()
        .is_some_and(|table| table.current_pid() != 0)
}

// IRQ1/IRQ12 share the 8042 data port.  Always route bytes by the controller
// status AUX bit; IRQ lines can be noisy or coalesced, and a blind read in the
// mouse handler can steal a keyboard scancode from port 0x60.
extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    crate::interrupt_contract::InterruptContract::record_irq_entry(IRQ_OFFSET + IRQ_KEYBOARD);
    KB_IRQS.fetch_add(1, Ordering::Relaxed);
    // Drain all pending bytes (OBF-guarded) rather than a single unconditional
    // read: a bare read with no byte ready injects a bogus 0x00, and draining
    // here keeps us in lock-step with the timer's safety-net drain.
    drain_ps2_keyboard();
    eoi(IRQ_KEYBOARD);
}

extern "x86-interrupt" fn mouse_handler(_frame: InterruptStackFrame) {
    crate::interrupt_contract::InterruptContract::record_irq_entry(IRQ_OFFSET + IRQ_MOUSE);
    MOUSE_IRQS.fetch_add(1, Ordering::Relaxed);
    drain_ps2_keyboard();
    eoi(IRQ_MOUSE);
}

/// NMI handler: if Red Ring is active, this CPU halts permanently.
/// Constitutional requirement (SSOT §Red Ring Step 2): NMI broadcast freezes
/// all CPUs before KDS seal.  This handler MUST NOT acquire any lock.
extern "x86-interrupt" fn nmi_handler(_frame: InterruptStackFrame) {
    if crate::reliability_contract::ReliabilityContract::active() {
        // Red Ring triggered on another CPU — halt this one permanently.
        // No lock acquisition, no allocation, no KDS write.
        loop {
            crate::arch::halt();
        }
    }
    // If Red Ring is not active, this is a spurious or debug NMI — ignore.
}

extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    crate::interrupt_contract::InterruptContract::dump_fault_frame(
        crate::interrupt_contract::InterruptKind::Other(0),
        "divide error",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        0,
        0,
    );
    let cs = stack_frame.code_segment;
    let cpl = cs & 3;

    // User fault: kill the process, kernel survives
    if should_contain_user_fault(cpl) {
        handle_user_exception(
            c"DE".as_ptr() as *const u8,
            2,
            &stack_frame,
            0,
            -11,
            c"divide error (SIGFPE)".as_ptr() as *const u8,
            21,
        );
        // handle_user_exception now returns normally; the scheduler will
        // clean up the dying process on the next context switch.
        return;
    }

    // Kernel fault: panic
    crate::diag::fault::dump("DE", &stack_frame, 0);
    panic!("[#DE DIVIDE ERROR]\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    crate::interrupt_contract::InterruptContract::dump_fault_frame(
        crate::interrupt_contract::InterruptKind::Other(6),
        "invalid opcode",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        0,
        0,
    );
    let cs = stack_frame.code_segment;
    let cpl = cs & 3;

    // User fault: kill the process, kernel survives
    if should_contain_user_fault(cpl) {
        handle_user_exception(
            c"UD".as_ptr() as *const u8,
            2,
            &stack_frame,
            0,
            -4,
            c"invalid opcode (SIGILL)".as_ptr() as *const u8,
            23,
        );
        // handle_user_exception now returns normally; the scheduler will
        // clean up the dying process on the next context switch.
        return;
    }

    // Kernel fault: panic
    crate::diag::fault::dump("UD", &stack_frame, 0);
    panic!(
        "[#UD INVALID OPCODE] RIP={:#x}\n{:#?}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame
    );
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let cs = stack_frame.code_segment;
    let cpl = cs & 3;
    let rip = stack_frame.instruction_pointer.as_u64();
    crate::interrupt_contract::InterruptContract::dump_fault_frame(
        crate::interrupt_contract::InterruptKind::GeneralProtectionFault,
        "general protection fault",
        rip,
        stack_frame.stack_pointer.as_u64(),
        error_code,
        0,
    );

    // User fault: kill the process, kernel survives
    if should_contain_user_fault(cpl) {
        handle_user_exception(
            c"GP".as_ptr() as *const u8,
            2,
            &stack_frame,
            error_code,
            -11,
            c"general protection fault (SIGSEGV)".as_ptr() as *const u8,
            35,
        );
        // handle_user_exception now returns normally; the scheduler will
        // clean up the dying process on the next context switch.
        return;
    }

    // Kernel fault: panic
    crate::serial_println!(
        "[#GP] error={:#x} rip={:#x} cs={:#x} cpl={} rflags={:#x}",
        error_code,
        rip,
        cs,
        cpl,
        stack_frame.cpu_flags
    );
    crate::diag::fault::dump("GP", &stack_frame, error_code);

    panic!(
        "[#GP GENERAL PROTECTION FAULT] error_code={:#x} RIP={:#x} CS={:#x} CPL={}\n{:#?}",
        error_code, rip, cs, cpl, stack_frame
    );
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("[EXCEPTION] BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    crate::interrupt_contract::InterruptContract::dump_fault_frame(
        crate::interrupt_contract::InterruptKind::DoubleFault,
        "double fault",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        0,
        0,
    );
    let cpu = crate::process::table::cpu_idx();
    let apic_id = crate::smp::lapic_id();
    let (current, idle, prev) = crate::process::table::TABLE
        .try_lock()
        .map(|table| {
            let snapshot = table.scheduler_snapshot();
            (
                snapshot.current[cpu],
                snapshot.idle[cpu],
                snapshot.prev[cpu],
            )
        })
        .unwrap_or((0, 0, 0));
    let (gs_base, kernel_gs_base) = unsafe {
        (
            crate::arch::process::read_gs_base(),
            crate::arch::process::read_kernel_gs_base(),
        )
    };
    let (entry_gs0, entry_gs8, entry_gs16) = crate::arch::syscall::syscall_entry_probes();
    crate::serial_println!(
        "[DOUBLE FAULT CPU] cpu_id={} apic_id={} gs_base={:#x} kernel_gs_base={:#x} tss_rsp0={:#x} current={} idle={} prev={}",
        cpu,
        apic_id,
        gs_base,
        kernel_gs_base,
        crate::gdt::current_rsp0(),
        current,
        idle,
        prev
    );
    crate::serial_println!(
        "[DOUBLE FAULT SYSCALL ENTRY] gs0={:#x} gs8={:#x} gs16={:#x}",
        entry_gs0,
        entry_gs8,
        entry_gs16
    );
    // #DF is the very last thing before a CPU reset; there is no IDT frame
    // dump possible once the fault handler returns, so we print everything
    // we can and then halt.  (We don't go through diag::fault::dump here
    // because we want the panic to be the final line — no allocation paths.)
    crate::serial_println!(
        "\n[DOUBLE FAULT] RIP={:#x} RSP={:#x} CS={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        stack_frame.code_segment
    );
    // DOC-09: Double fault triggers Red Ring (not just panic).
    crate::reliability_contract::ReliabilityContract::enter_red_ring(
        crate::reliability_contract::RedRingEvidence {
            cause: crate::reliability_contract::RedRingCause::KernelPanic,
            evidence_event_id: 0,
            invariant_id: stack_frame.instruction_pointer.as_u64(),
            detail: stack_frame.stack_pointer.as_u64(),
        },
    );
    panic!("[DOUBLE FAULT]\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let cr2 = crate::arch::fault_address();
    let cs = stack_frame.code_segment;
    let cpl = cs & 3;
    let err_bits = error_code.bits();
    let rip = stack_frame.instruction_pointer.as_u64();
    let rsp = stack_frame.stack_pointer.as_u64();
    crate::interrupt_contract::InterruptContract::dump_fault_frame(
        crate::interrupt_contract::InterruptKind::PageFault,
        "page fault",
        rip,
        rsp,
        err_bits,
        cr2,
    );

    // User fault: handle stack growth or kill the process
    if should_contain_user_fault(cpl) {
        let fault_addr = cr2;

        // Shared writable pages become read-only after fork. A present+write
        // page fault on a COW-marked mapping should clone the page and retry.
        if error_code.contains(PageFaultErrorCode::PROTECTION_VIOLATION)
            && error_code.contains(PageFaultErrorCode::CAUSED_BY_WRITE)
        {
            let (pid, pml4) = {
                crate::process::table::TABLE
                    .try_lock()
                    .map(|table| {
                        let pid = table.current_pid();
                        let pml4 = table
                            .current_ref()
                            .map(|proc| proc.address_space_pml4())
                            .unwrap_or(0);
                        (pid, pml4)
                    })
                    .unwrap_or((0, 0))
            };
            let pml4 = if pml4 != 0 {
                pml4
            } else {
                crate::memory::paging::active_pml4()
            };
            let is_cow_fault = crate::memory::paging::translate_entry_in(pml4, fault_addr)
                .is_some_and(|(_, flags)| flags & crate::memory::paging::PTE_COW != 0);
            if is_cow_fault {
                crate::serial_println!("[cow] write fault addr={:#x}", fault_addr);
            }
            if is_cow_fault
                && let Ok(true) = crate::memory_contract::MemoryContract::resolve_cow_fault(
                    crate::address_space_contract::AddressSpaceHandle {
                        id: pml4,
                        pml4,
                        owner_pid: pid,
                    },
                    fault_addr,
                )
            {
                crate::memory_contract::MemoryContract::record_fault(
                    pml4,
                    fault_addr,
                    error_code.bits(),
                    true,
                    "cow_fault_resolved",
                );
                if crate::diag::diag_proc_on()
                    && let Some((_, flags)) =
                        crate::memory::paging::translate_entry_in(pml4, fault_addr)
                {
                    crate::serial_println!(
                        "[cow] resolved pid={} pml4={:#x} cr3={:#x} flags={:#x}",
                        pid,
                        pml4,
                        crate::memory::paging::active_pml4(),
                        flags
                    );
                }
                return;
            }
        }
        // Check if this is a stack growth fault
        let is_stack_fault = {
            crate::process::table::TABLE
                .try_lock()
                .and_then(|table| {
                    table.current_ref().map(|proc| {
                        // Stack grows downward from USER_STACK_TOP.
                        fault_addr < proc.stack_base
                            && fault_addr >= USER_STACK_TOP - crate::process::USER_STACK_SIZE as u64
                    })
                })
                .unwrap_or(false)
        };

        // Try to grow the stack if it's a stack growth fault
        let was_grown = if is_stack_fault {
            let grown = crate::process::table::TABLE
                .try_lock()
                .and_then(|mut table| {
                    table
                        .current_mut()
                        .map(|proc| match crate::process::grow_user_stack(proc) {
                            Ok(true) => true,
                            Ok(false) | Err(_) => false,
                        })
                })
                .unwrap_or(false);
            if grown {
                let _ = crate::process::refresh_current_from_table();
            }
            grown
        } else {
            false
        };

        if was_grown {
            crate::memory_contract::MemoryContract::record_fault(
                crate::memory::paging::active_pml4(),
                fault_addr,
                error_code.bits(),
                true,
                "stack_growth_fault",
            );
            // Stack was successfully grown, return and retry the instruction
            return;
        }

        crate::memory_contract::MemoryContract::record_fault(
            crate::memory::paging::active_pml4(),
            fault_addr,
            error_code.bits(),
            false,
            "user_page_fault",
        );

        crate::serial_println!(
            "[PF] cr2={:#x} rip={:#x} rsp={:#x} err={:#x}",
            cr2,
            rip,
            rsp,
            err_bits
        );
        // Dump page mappings for diagnostic analysis
        crate::serial_println!("[PF] Page mappings:");
        crate::serial_println!("[PF] CR2 (faulting address):");
        crate::memory::paging::dump_page_mapping(cr2);
        crate::serial_println!("[PF] RIP (code fetch address):");
        crate::memory::paging::dump_page_mapping(rip);
        crate::serial_println!("[PF] RSP (stack pointer):");
        crate::memory::paging::dump_page_mapping(rsp);
        crate::serial_println!("[PF] End mappings");

        handle_user_exception(
            c"PF".as_ptr() as *const u8,
            2,
            &stack_frame,
            err_bits,
            -11,
            c"page fault (SIGSEGV)".as_ptr() as *const u8,
            23,
        );
        // handle_user_exception now returns normally; the scheduler will
        // clean up the dying process on the next context switch.
        return;
    }

    // Kernel fault: panic
    crate::serial_println!(
        "[#PF] cr2={:#x} rip={:#x} cs={:#x} cpl={} error={:?}",
        cr2,
        rip,
        cs,
        cpl,
        error_code
    );
    crate::memory_contract::MemoryContract::record_fault(
        crate::memory::paging::active_pml4(),
        cr2,
        error_code.bits(),
        false,
        "kernel_page_fault",
    );
    crate::diag::fault::dump("PF", &stack_frame, err_bits);

    // Also dump page mappings for kernel faults
    crate::serial_println!("[PF] Page mappings:");
    crate::serial_println!("[PF] CR2 (faulting address):");
    crate::memory::paging::dump_page_mapping(cr2);
    crate::serial_println!("[PF] RIP (code fetch address):");
    crate::memory::paging::dump_page_mapping(rip);
    crate::serial_println!("[PF] RSP (stack pointer):");
    crate::memory::paging::dump_page_mapping(rsp);
    crate::serial_println!("[PF] End mappings");

    println!("[PAGE FAULT] kernel fault: Accessed: {:#x}", cr2);
    println!("Error code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    crate::hlt_loop();
}

/// #SS (Stack Segment Fault) handler - CPL=3 kills process, CPL=0 panics
extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) {
    crate::interrupt_contract::InterruptContract::dump_fault_frame(
        crate::interrupt_contract::InterruptKind::Other(12),
        "stack segment fault",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        0,
        0,
    );
    let cs = stack_frame.code_segment;
    let cpl = cs & 3;

    // User fault: kill the process, kernel survives
    if should_contain_user_fault(cpl) {
        handle_user_exception(
            c"SS".as_ptr() as *const u8,
            2,
            &stack_frame,
            0,
            -11,
            c"stack segment fault (SIGSEGV)".as_ptr() as *const u8,
            31,
        );
        // handle_user_exception now returns normally; the scheduler will
        // clean up the dying process on the next context switch.
        return;
    }

    // Kernel fault: panic
    crate::diag::fault::dump("SS", &stack_frame, 0);
    panic!("[#SS STACK SEGMENT FAULT]\n{:#?}", stack_frame);
}

/// #AC (Alignment Check) handler - CPL=3 kills process, CPL=0 panics
extern "x86-interrupt" fn alignment_check_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::interrupt_contract::InterruptContract::dump_fault_frame(
        crate::interrupt_contract::InterruptKind::Other(17),
        "alignment check",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        error_code,
        0,
    );
    let cs = stack_frame.code_segment;
    let cpl = cs & 3;

    // User fault: kill the process, kernel survives
    if should_contain_user_fault(cpl) {
        handle_user_exception(
            c"AC".as_ptr() as *const u8,
            2,
            &stack_frame,
            error_code,
            -11,
            c"alignment check fault (SIGSEGV)".as_ptr() as *const u8,
            33,
        );
        // handle_user_exception now returns normally; the scheduler will
        // clean up the dying process on the next context switch.
        return;
    }

    // Kernel fault: panic
    crate::diag::fault::dump("AC", &stack_frame, error_code);
    panic!("[#AC ALIGNMENT CHECK FAULT]\n{:#?}", stack_frame);
}

/// #OF (Overflow) handler - CPL=3 kills process, CPL=0 panics
extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    crate::interrupt_contract::InterruptContract::dump_fault_frame(
        crate::interrupt_contract::InterruptKind::Other(4),
        "overflow",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.stack_pointer.as_u64(),
        0,
        0,
    );
    let cs = stack_frame.code_segment;
    let cpl = cs & 3;

    // User fault: kill the process, kernel survives
    if should_contain_user_fault(cpl) {
        handle_user_exception(
            c"OF".as_ptr() as *const u8,
            2,
            &stack_frame,
            0,
            -4,
            c"overflow fault (SIGSEGV)".as_ptr() as *const u8,
            25,
        );
        // handle_user_exception now returns normally; the scheduler will
        // clean up the dying process on the next context switch.
        return;
    }

    // Kernel fault: panic
    crate::diag::fault::dump("OF", &stack_frame, 0);
    panic!("[#OF OVERFLOW]\n{:#?}", stack_frame);
}
