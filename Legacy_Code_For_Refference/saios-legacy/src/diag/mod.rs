//! SAIOS kernel diagnostics.
//!
//! This module is the home for everything that helps answer "why did the
//! kernel freeze / crash / misbehave" after the fact:
//!
//!   * [`heartbeat`]  — 1 Hz liveness counter from the PIT timer IRQ.
//!   * [`watchdog`]   — detects when no forward progress has been made for
//!     5-10 s and dumps kernel state before the panic handler runs.
//!   * [`fault`]      — register/context dump used by the `#PF` / `#GP` /
//!     `#UD` / `#DF` handlers in `src/interrupts.rs`.
//!
//! All three are intentionally small, additive, and off the hot path:
//! the heartbeat stays silent unless queried, the watchdog fires only on
//! real stalls, and the fault dump runs only at the exception
//! site (which is by definition a crash).  Toggling scheduler / process
//! prints is gated by [`DIAG_FLAGS`] (a single atomic u32 set from the
//! `diag sched on|off` and `diag proc on|off` shell commands).
//!
//! The module is wired into the boot sequence in
//! [`crate::main::kernel_main`] right after `interrupts::init_idt()`,
//! so the heartbeat and watchdog start counting from the first PIT tick.

pub mod fault;
pub mod heartbeat;
pub mod watchdog;

use core::sync::atomic::{AtomicU32, Ordering};

/// Bit flags for the runtime-toggleable diagnostic prints.  Set from the
/// `diag sched on|off` and `diag proc on|off` shell commands; read from
/// the scheduler and process lifecycle paths.  Default: all off (quiet
/// boot; user turns them on after they suspect a problem).
const DIAG_SCHED_BIT: u32 = 1 << 0; // [sched] pid A -> pid B on every context switch
const DIAG_PROC_BIT: u32 = 1 << 1; // [proc] create / start / exit / destroy lines

pub static DIAG_FLAGS: AtomicU32 = AtomicU32::new(0);

#[inline]
pub fn diag_sched_on() -> bool {
    DIAG_FLAGS.load(Ordering::Relaxed) & DIAG_SCHED_BIT != 0
}
#[inline]
pub fn diag_proc_on() -> bool {
    DIAG_FLAGS.load(Ordering::Relaxed) & DIAG_PROC_BIT != 0
}

/// Set or clear a single flag (shell command `diag sched on` etc).
pub fn set_flag(bit: u32, on: bool) {
    if on {
        DIAG_FLAGS.fetch_or(bit, Ordering::Relaxed);
    } else {
        DIAG_FLAGS.fetch_and(!bit, Ordering::Relaxed);
    }
}

pub const fn diag_sched_bit() -> u32 {
    DIAG_SCHED_BIT
}
pub const fn diag_proc_bit() -> u32 {
    DIAG_PROC_BIT
}

/// Wire the diagnostic module into the kernel.  Called from
/// [`crate::main::kernel_main`] after `interrupts::init_idt()`.  Currently
/// just turns on the heartbeat counter; the watchdog and fault-dump code
/// are on by default (zero-cost until a fault happens).
pub fn init() {
    heartbeat::init();
    watchdog::init();
    crate::println!(
        "[diag] watchdog ({} s) + fault dump ready",
        watchdog::TIMEOUT_SECS
    );
}
