//! Programmable Interval Timer (PIT) based kernel timer.
//!
//! Configures the legacy 8259 PIC and PIT channel 0 to deliver 100 Hz timer
//! interrupts on IRQ0. The interrupt trampoline preserves full CPU register
//! state, calls [`saios_timer_tick`], and the handler acknowledges the
//! interrupt with an EOI.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::time::Duration;

use hal::arch::x86_64::idt;
use hal::arch::x86_64::io::{inb, io_wait, outb};

/// PIT input clock frequency in Hz.
const PIT_INPUT_HZ: u32 = 1_193_182;
/// Desired timer tick frequency in Hz.
const TICK_HZ: u32 = 100;
/// IDT vector used for the timer interrupt (remapped IRQ0).
const TIMER_VECTOR: u8 = 32;
const PIC_MASTER_COMMAND_PORT: u16 = 0x20;
const PIC_MASTER_DATA_PORT: u16 = 0x21;
const PIC_SLAVE_COMMAND_PORT: u16 = 0xA0;
const PIC_SLAVE_DATA_PORT: u16 = 0xA1;
const PIC_END_OF_INTERRUPT: u8 = 0x20;
const PIC_ICW1_INIT_WITH_ICW4: u8 = 0x11;
const PIC_MASTER_VECTOR_OFFSET: u8 = 0x20;
const PIC_SLAVE_VECTOR_OFFSET: u8 = 0x28;
const PIC_MASTER_SLAVE_ON_IRQ2: u8 = 0x04;
const PIC_SLAVE_CASCADE_ID: u8 = 0x02;
const PIC_ICW4_8086_MODE: u8 = 0x01;
const PIC_MASK_ALL_EXCEPT_IRQ0: u8 = 0xFE;
const PIC_IRQ0_MASK: u8 = 0x01;
const PIC_MASK_ALL: u8 = 0xFF;
const PIT_CHANNEL0_DATA_PORT: u16 = 0x40;
const PIT_COMMAND_PORT: u16 = 0x43;
const PIT_CHANNEL0_MODE3_LOHI: u8 = 0x36;
const PIT_DIVISOR_LOW_BYTE_MASK: u16 = 0x00FF;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static TICKS: AtomicU64 = AtomicU64::new(0);

/// Diagnostic watchdog: while a user process is active, periodically report
/// the interrupted RIP so a hang with no fault marker can be told apart from
/// a tight user-mode spin loop (same RIP every sample) vs. genuine forward
/// progress (RIP changes / process exits before the next sample).
const WATCHDOG_SAMPLE_TICKS: u64 = 50; // ~500ms at 100Hz

#[unsafe(no_mangle)]
extern "C" fn saios_timer_tick(interrupted_rip: u64) {
    let tick = TICKS.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    if tick % WATCHDOG_SAMPLE_TICKS == 0
        && let Some(pid) = crate::kernel::fault::active_exec_pid_lockfree()
    {
        crate::console::println!(
            "[watchdog] pid={} interrupted-rip=0x{:x} tick={}",
            pid,
            interrupted_rip,
            tick
        );
    }
    crate::scheduler::on_timer_tick(tick);
    crate::console::on_timer_tick();
    // PIC EOI for IRQ0.
    outb(PIC_MASTER_COMMAND_PORT, PIC_END_OF_INTERRUPT);
}

fn remap_pic() {
    let master_mask = inb(PIC_MASTER_DATA_PORT);
    let slave_mask = inb(PIC_SLAVE_DATA_PORT);

    // Start PIC initialization.
    outb(PIC_MASTER_COMMAND_PORT, PIC_ICW1_INIT_WITH_ICW4);
    io_wait();
    outb(PIC_SLAVE_COMMAND_PORT, PIC_ICW1_INIT_WITH_ICW4);
    io_wait();

    // Vector offsets: 32..39 (master), 40..47 (slave).
    outb(PIC_MASTER_DATA_PORT, PIC_MASTER_VECTOR_OFFSET);
    io_wait();
    outb(PIC_SLAVE_DATA_PORT, PIC_SLAVE_VECTOR_OFFSET);
    io_wait();

    // Master has slave on IRQ2, slave identity = 2.
    outb(PIC_MASTER_DATA_PORT, PIC_MASTER_SLAVE_ON_IRQ2);
    io_wait();
    outb(PIC_SLAVE_DATA_PORT, PIC_SLAVE_CASCADE_ID);
    io_wait();

    // 8086 mode.
    outb(PIC_MASTER_DATA_PORT, PIC_ICW4_8086_MODE);
    io_wait();
    outb(PIC_SLAVE_DATA_PORT, PIC_ICW4_8086_MODE);
    io_wait();

    // Keep only timer IRQ unmasked on master; mask all slave IRQs.
    outb(
        PIC_MASTER_DATA_PORT,
        (master_mask | PIC_MASK_ALL_EXCEPT_IRQ0) & !PIC_IRQ0_MASK,
    );
    outb(PIC_SLAVE_DATA_PORT, PIC_MASK_ALL);

    // Preserve previous masks if needed later; for now timer-only policy.
    let _ = slave_mask;
}

fn init_pit() {
    let divisor = (PIT_INPUT_HZ / TICK_HZ) as u16;

    // Channel 0, lobyte/hibyte, mode 3 (square wave), binary.
    outb(PIT_COMMAND_PORT, PIT_CHANNEL0_MODE3_LOHI);
    outb(
        PIT_CHANNEL0_DATA_PORT,
        (divisor & PIT_DIVISOR_LOW_BYTE_MASK) as u8,
    );
    outb(PIT_CHANNEL0_DATA_PORT, (divisor >> 8) as u8);
}

/// Initializes the PIC and PIT and registers the timer interrupt handler.
pub fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    remap_pic();
    init_pit();
    idt::register_raw(TIMER_VECTOR, hal::arch::x86_64::seed_support::timer_irq0_stub_addr());
}

/// Returns the number of timer ticks since initialization.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Returns the approximate uptime since initialization.
pub fn uptime() -> Duration {
    Duration::from_millis((ticks() * 1000) / (TICK_HZ as u64))
}

/// Sleeps for approximately `ms` milliseconds.
pub fn sleep(ms: u64) {
    let tick_delta = (ms.saturating_mul(TICK_HZ as u64)).div_ceil(1000);
    crate::scheduler::sleep_ticks(tick_delta);
}
