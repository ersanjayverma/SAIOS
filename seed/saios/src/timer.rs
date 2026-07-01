use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::time::Duration;

use hal::arch::x86_64::idt;
use hal::arch::x86_64::io::{inb, io_wait, outb};

const PIT_INPUT_HZ: u32 = 1_193_182;
const TICK_HZ: u32 = 100;
const TIMER_VECTOR: u8 = 32;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static TICKS: AtomicU64 = AtomicU64::new(0);

global_asm!(
    ".global saios_timer_irq0_stub",
    "saios_timer_irq0_stub:",
    "push rax",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "call saios_timer_tick",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rax",
    "iretq",
);

unsafe extern "C" {
    fn saios_timer_irq0_stub();
}

#[unsafe(no_mangle)]
extern "C" fn saios_timer_tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::scheduler::on_timer_tick();
    // PIC EOI for IRQ0.
    outb(0x20, 0x20);
}

fn remap_pic() {
    let master_mask = inb(0x21);
    let slave_mask = inb(0xA1);

    // Start PIC initialization.
    outb(0x20, 0x11);
    io_wait();
    outb(0xA0, 0x11);
    io_wait();

    // Vector offsets: 32..39 (master), 40..47 (slave).
    outb(0x21, 0x20);
    io_wait();
    outb(0xA1, 0x28);
    io_wait();

    // Master has slave on IRQ2, slave identity = 2.
    outb(0x21, 0x04);
    io_wait();
    outb(0xA1, 0x02);
    io_wait();

    // 8086 mode.
    outb(0x21, 0x01);
    io_wait();
    outb(0xA1, 0x01);
    io_wait();

    // Keep only timer IRQ unmasked on master; mask all slave IRQs.
    outb(0x21, (master_mask | 0xFE) & !0x01);
    outb(0xA1, 0xFF);

    // Preserve previous masks if needed later; for now timer-only policy.
    let _ = slave_mask;
}

fn init_pit() {
    let divisor = (PIT_INPUT_HZ / TICK_HZ) as u16;

    // Channel 0, lobyte/hibyte, mode 3 (square wave), binary.
    outb(0x43, 0x36);
    outb(0x40, (divisor & 0xFF) as u8);
    outb(0x40, (divisor >> 8) as u8);
}

pub fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    remap_pic();
    init_pit();
    idt::register_raw(TIMER_VECTOR, saios_timer_irq0_stub as *const () as usize);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn uptime() -> Duration {
    Duration::from_millis((ticks() * 1000) / (TICK_HZ as u64))
}

pub fn sleep(ms: u64) {
    let tick_delta = (ms.saturating_mul(TICK_HZ as u64)).div_ceil(1000);
    let target = ticks().saturating_add(tick_delta);

    while ticks() < target {
        crate::scheduler::maybe_preempt();
        core::hint::spin_loop();
    }
}
