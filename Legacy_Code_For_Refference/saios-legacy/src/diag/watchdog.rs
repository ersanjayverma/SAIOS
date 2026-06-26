//! Forward-progress watchdog.
//!
//! The watchdog fires from the same 1 Hz path as the heartbeat
//! ([`crate::diag::heartbeat::tick`]).  Every heart-beat it compares
//! the current state of three "alive" counters to their values at the
//! previous heartbeat:
//!
//!   * [`crate::interrupts::TIMER_IRQS`]  — PIT has fired since the
//!     last beat.  If not, the timer IRQ has stopped (PIC mask
//!     failure, infinite `without_interrupts`, etc).
//!   * [`crate::shell::commands::BOOT_TICKS`]  — kernel time has
//!     advanced.  (This is also driven by the PIT, but a hung
//!     `without_interrupts` block that spans multiple PIT ticks
//!     would still bump TIMER_IRQS *once* on entry, so we double-
//!     check with BOOT_TICKS as a wall-clock proxy.)
//!   * run-queue progress — the scheduler picked a different thread
//!     since the last beat.  (Only checked when there is a runnable
//!     thread; the shell and idle threads legitimately do nothing.)
//!
//! If the watchdog decides no progress has been made for
//! [`TIMEOUT_SECS`] seconds, it dumps the current state and enters
//! the panic handler.  This converts a frozen VM into a *diagnosed*
//! frozen VM: the seriallog captures the exact moment things stopped.
//!
//! # Waiting for input
//!
//! Interactive prompts (shell, installer, setup) legitimately wait for
//! keyboard input.  This is not a freeze.  Use `enter_input_wait()` and
//! `leave_input_wait()` to tell the watchdog that the system is waiting
//! for user input.  While in input-wait mode, the watchdog does not
//! check for stalls at all - it only panics when the system is NOT
//! waiting for input.

use crate::diag::heartbeat::{HEARTBEAT_LAST_TICK, LAST_TIMER_IRQS_AT_HEARTBEAT};
use crate::process::table::MAX_CPUS;
use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

/// How many heartbeats (seconds) the watchdog tolerates without
/// forward progress before declaring a freeze and dumping state.
/// 10 s is a generous default: legitimate long syscalls (ext4 mount,
/// TLS handshake, apt fetch) can easily take that long.
pub const TIMEOUT_SECS: u64 = 10;

/// Wait mode: the watchdog's understanding of what the system is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
enum WaitMode {
    /// Normal operation.  Progress checks apply normally.
    Running,
    /// System is waiting for keyboard input (shell prompt, installer prompt).
    /// While in this mode, the watchdog does NOT check for progress at all.
    /// Waiting for user input is legitimate and should not cause a panic.
    WaitingForInput,
}

/// Current wait mode.  Updated by `enter_input_wait()` / `leave_input_wait()`.
static WAIT_MODE: AtomicU8 = AtomicU8::new(WaitMode::Running as u8);

/// Wall-clock proxy (BSP PIT ticks since boot).  Read once a second
/// by the watchdog; we just remember the value at the previous beat
/// and complain if it hasn't moved by [`TIMEOUT_SECS`].
static LAST_BOOT_TICKS_AT_HEARTBEAT: AtomicU64 = AtomicU64::new(0);

/// Last time the watchdog saw forward progress.  Bumped from
/// [`crate::process::scheduler::schedule`] on every context switch
/// and from [`crate::process::terminate`] on every process exit, so
/// any user-space activity resets the timer.  Initial value: 0.
static LAST_PROGRESS_HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static LAST_CPU_HEARTBEAT_TSC: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static LAST_CPU_PROGRESS_TSC: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static LAST_CPU_PID: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
static LAST_SCHEDULER_PROGRESS: AtomicU64 = AtomicU64::new(0);
static LAST_CONTEXT_SWITCHES: AtomicU64 = AtomicU64::new(0);
static LAST_KDS_EVENTS: AtomicU64 = AtomicU64::new(0);
static LAST_KDS_METRICS: AtomicU64 = AtomicU64::new(0);
static LAST_KDS_STATE: AtomicU64 = AtomicU64::new(0);
static LAST_RUN_QUEUE_FINGERPRINT: AtomicU64 = AtomicU64::new(0);

/// One-time init.  Snapshots the initial boot-ticks value.
pub fn init() {
    LAST_BOOT_TICKS_AT_HEARTBEAT.store(crate::shell::commands::boot_ticks(), Ordering::Relaxed);
    let now = crate::time::rdtsc();
    for cpu in 0..MAX_CPUS {
        LAST_CPU_HEARTBEAT_TSC[cpu].store(now, Ordering::Relaxed);
        LAST_CPU_PROGRESS_TSC[cpu].store(now, Ordering::Relaxed);
    }
    LAST_SCHEDULER_PROGRESS.store(0, Ordering::Relaxed);
    LAST_CONTEXT_SWITCHES.store(0, Ordering::Relaxed);
    LAST_KDS_EVENTS.store(0, Ordering::Relaxed);
    LAST_KDS_METRICS.store(0, Ordering::Relaxed);
    LAST_KDS_STATE.store(0, Ordering::Relaxed);
    LAST_RUN_QUEUE_FINGERPRINT.store(0, Ordering::Relaxed);
}

/// Complete watchdog progress baselining after the heap exists.
///
/// `ProgressContract::snapshot()` may inspect scheduler data backed by `Vec`,
/// so it must not run during the pre-heap watchdog init path.
pub fn init_after_heap() {
    store_progress_snapshot(crate::progress_contract::ProgressContract::snapshot());
}

/// Called from every CPU's timer interrupt path.  This is separate from the
/// global 1 Hz heartbeat so AP timer starvation shows up in watchdog dumps.
pub fn note_cpu_heartbeat() {
    let cpu = crate::process::table::cpu_idx();
    LAST_CPU_HEARTBEAT_TSC[cpu].store(crate::time::rdtsc(), Ordering::Relaxed);
    crate::kds::note_cpu_heartbeat(cpu, crate::process::current_pid().unwrap_or(0));
}

/// Called when the scheduler publishes a process as CPU-current.
pub fn note_scheduler_progress(cpu: usize, pid: u32) {
    if cpu >= MAX_CPUS {
        return;
    }
    let now = crate::time::rdtsc();
    LAST_CPU_PROGRESS_TSC[cpu].store(now, Ordering::Relaxed);
    LAST_CPU_PID[cpu].store(pid as u64, Ordering::Relaxed);
    crate::kds::note_scheduler_progress(cpu, pid);
    LAST_PROGRESS_HEARTBEAT.store(
        HEARTBEAT_LAST_TICK.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
}

/// Called by the scheduler on every context switch.  Bumps
/// [`LAST_PROGRESS_HEARTBEAT`] so a busy user-space process keeps
/// the watchdog quiet.
pub fn note_progress() {
    let beat = HEARTBEAT_LAST_TICK.load(Ordering::Relaxed);
    LAST_PROGRESS_HEARTBEAT.store(beat, Ordering::Relaxed);
    if let Some(pid) = crate::process::current_pid() {
        let cpu = crate::process::table::cpu_idx();
        LAST_CPU_PROGRESS_TSC[cpu].store(crate::time::rdtsc(), Ordering::Relaxed);
        LAST_CPU_PID[cpu].store(pid as u64, Ordering::Relaxed);
    }
    if crate::diag::diag_sched_on() {
        crate::serial_println!("[watchdog] progress beat={} source=sched", beat);
    }
}

/// Called by `terminate` when a process exits.  Same as
/// [`note_progress`]: an exit is forward progress.
pub fn note_progress_exit() {
    let beat = HEARTBEAT_LAST_TICK.load(Ordering::Relaxed);
    LAST_PROGRESS_HEARTBEAT.store(beat, Ordering::Relaxed);
    if crate::diag::diag_proc_on() {
        crate::serial_println!("[watchdog] progress beat={} source=exit", beat);
    }
}

/// Dump current progress state for debugging.
pub fn dump_state() {
    let last_progress = LAST_PROGRESS_HEARTBEAT.load(Ordering::Relaxed);
    let now_beat = HEARTBEAT_LAST_TICK.load(Ordering::Relaxed);
    crate::serial_println!(
        "[watchdog] dump: last_progress={} now_beat={}",
        last_progress,
        now_beat
    );
    dump_cpu_progress(crate::time::rdtsc());
    dump_progress_contract();
}

fn stored_progress_snapshot() -> crate::progress_contract::ProgressSnapshot {
    crate::progress_contract::ProgressSnapshot {
        heartbeat: HEARTBEAT_LAST_TICK.load(Ordering::Relaxed),
        timer_irqs: crate::interrupts::TIMER_IRQS.load(Ordering::Relaxed),
        boot_ticks: crate::shell::commands::boot_ticks(),
        scheduler_progress: LAST_SCHEDULER_PROGRESS.load(Ordering::Relaxed),
        context_switches: LAST_CONTEXT_SWITCHES.load(Ordering::Relaxed),
        kds_events: LAST_KDS_EVENTS.load(Ordering::Relaxed),
        kds_metrics: LAST_KDS_METRICS.load(Ordering::Relaxed),
        kds_state: LAST_KDS_STATE.load(Ordering::Relaxed),
        run_queue_fingerprint: LAST_RUN_QUEUE_FINGERPRINT.load(Ordering::Relaxed),
        scheduler_snapshot_available: true,
    }
}

fn store_progress_snapshot(snapshot: crate::progress_contract::ProgressSnapshot) {
    LAST_SCHEDULER_PROGRESS.store(snapshot.scheduler_progress, Ordering::Relaxed);
    LAST_CONTEXT_SWITCHES.store(snapshot.context_switches, Ordering::Relaxed);
    LAST_KDS_EVENTS.store(snapshot.kds_events, Ordering::Relaxed);
    LAST_KDS_METRICS.store(snapshot.kds_metrics, Ordering::Relaxed);
    LAST_KDS_STATE.store(snapshot.kds_state, Ordering::Relaxed);
    if snapshot.scheduler_snapshot_available {
        LAST_RUN_QUEUE_FINGERPRINT.store(snapshot.run_queue_fingerprint, Ordering::Relaxed);
    }
}

fn dump_progress_contract() {
    let snapshot = crate::progress_contract::ProgressContract::snapshot();
    crate::serial_println!(
        "[watchdog] progress-contract heartbeat={} timer_irqs={} boot_ticks={} scheduler_progress={} context_switches={} kds_events={} kds_metrics={} kds_state={} run_queue_fp={:#x} snapshot={}",
        snapshot.heartbeat,
        snapshot.timer_irqs,
        snapshot.boot_ticks,
        snapshot.scheduler_progress,
        snapshot.context_switches,
        snapshot.kds_events,
        snapshot.kds_metrics,
        snapshot.kds_state,
        snapshot.run_queue_fingerprint,
        if snapshot.scheduler_snapshot_available {
            "ok"
        } else {
            "locked"
        }
    );
}

fn dump_cpu_progress(now_tsc: u64) {
    let hz = crate::time::tsc_hz().max(1);
    let online_mask = crate::smp::online_mask();
    for cpu in 0..MAX_CPUS {
        if online_mask & (1u64 << cpu) == 0 {
            continue;
        }
        let heartbeat_tsc = LAST_CPU_HEARTBEAT_TSC[cpu].load(Ordering::Relaxed);
        let progress_tsc = LAST_CPU_PROGRESS_TSC[cpu].load(Ordering::Relaxed);
        let pid = LAST_CPU_PID[cpu].load(Ordering::Relaxed);
        crate::serial_println!(
            "[watchdog] cpu{} heartbeat_age_ms={} progress_age_ms={} last_pid={}",
            cpu,
            now_tsc.saturating_sub(heartbeat_tsc).saturating_mul(1000) / hz,
            now_tsc.saturating_sub(progress_tsc).saturating_mul(1000) / hz,
            pid
        );
    }
}

/// Enter input-wait mode.  Call this when entering a blocking input loop
/// (shell prompt, installer prompt, setup prompt).  While in this mode,
/// the watchdog does NOT check for forward progress - waiting for user
/// input is legitimate and should not cause a panic.
pub fn enter_input_wait() {
    WAIT_MODE.store(WaitMode::WaitingForInput as u8, Ordering::Relaxed);
    let beat = HEARTBEAT_LAST_TICK.load(Ordering::Relaxed);
    LAST_PROGRESS_HEARTBEAT.store(beat, Ordering::Relaxed);
    if crate::diag::diag_proc_on() {
        crate::serial_println!("[watchdog] enter_input_wait beat={}", beat);
    }
}

/// Leave input-wait mode.  Call this when input arrives or the prompt exits.
pub fn leave_input_wait() {
    WAIT_MODE.store(WaitMode::Running as u8, Ordering::Relaxed);
    if crate::diag::diag_proc_on() {
        crate::serial_println!("[watchdog] leave_input_wait");
    }
}

/// Check if we're currently waiting for input.
fn is_input_wait() -> bool {
    WAIT_MODE.load(Ordering::Relaxed) == WaitMode::WaitingForInput as u8
}

/// Called from the heartbeat path.  If no forward progress has been
/// made for [`TIMEOUT_SECS`] seconds and we're NOT waiting for input,
/// dump state and panic.
/// While waiting for input, the watchdog does not check for progress at all.
pub fn tick() {
    if let Some(panic) = crate::panic_state::snapshot() {
        crate::serial_println!("[watchdog] system already panicking");
        crate::serial_println!(
            "[watchdog] panic_cpu={} panic_pid={} panic_rip={:#x} panic_time={}",
            panic.owner_cpu,
            panic.owner_pid,
            panic.rip,
            panic.time
        );
        return;
    }

    let now_boot = crate::shell::commands::boot_ticks();
    let _prev_boot = LAST_BOOT_TICKS_AT_HEARTBEAT.swap(now_boot, Ordering::Relaxed);
    let _now_timer = LAST_TIMER_IRQS_AT_HEARTBEAT.load(Ordering::Relaxed);

    // If we're waiting for input, the watchdog does NOT check for progress.
    // Waiting for user input is legitimate and should not cause a panic.
    if is_input_wait() {
        return;
    }

    // Bootstrap grace period: give the system time to initialize before
    // strict progress checking begins.  The scheduler and shell don't run
    // until after boot, so we ignore progress checks during the first 30
    // heartbeats (30 seconds).
    let now_beat = HEARTBEAT_LAST_TICK.load(Ordering::Relaxed);
    if now_beat < 30 {
        // Update progress to current heartbeat to extend the grace period
        LAST_PROGRESS_HEARTBEAT.store(now_beat, Ordering::Relaxed);
        return;
    }

    // Only check progress when NOT in input-wait mode.
    let last_prog = LAST_PROGRESS_HEARTBEAT.load(Ordering::Relaxed);
    let current_progress = crate::progress_contract::ProgressContract::snapshot();
    let previous_progress = stored_progress_snapshot();
    let progress_delta =
        crate::progress_contract::ProgressContract::delta(previous_progress, current_progress);
    if progress_delta.work_progressing() {
        store_progress_snapshot(current_progress);
        LAST_PROGRESS_HEARTBEAT.store(now_beat, Ordering::Relaxed);
        if crate::diag::diag_sched_on() {
            crate::serial_println!(
                "[watchdog] progress-contract busy=true work_progress=true sched={} cs={} events={} metrics={} state={} queue={}",
                progress_delta.scheduler_progress_changed,
                progress_delta.context_switches_changed,
                progress_delta.kds_events_changed,
                progress_delta.kds_metrics_changed,
                progress_delta.kds_state_changed,
                progress_delta.run_queue_changed
            );
        }
        return;
    }

    // Diagnostic: print progress state on each tick (only during bootstrap)
    if now_beat < 30 {
        crate::serial_println!(
            "[watchdog] tick heartbeat={} last_progress={} grace",
            now_beat,
            last_prog
        );
    }

    if now_beat.saturating_sub(last_prog) < TIMEOUT_SECS {
        return;
    }

    // --- Stall: dump and panic --------------------------------------
    dump_and_panic(now_beat, last_prog);
}

fn dump_and_panic(now_beat: u64, last_prog: u64) {
    use crate::process::table::TABLE;
    let secs_stalled = now_beat.saturating_sub(last_prog);

    // Snapshot what we can.  All of these are best-effort; if a lock
    // is held in a way that would deadlock, we use `try_lock` and
    // print "?" for the value.
    let timer_irqs = crate::interrupts::TIMER_IRQS.load(Ordering::Relaxed);
    let boot_ticks = crate::shell::commands::boot_ticks();
    let cr3 = crate::memory::paging::active_pml4();

    let (sched_line, current_pid, current_name) = if let Some(t) = TABLE.try_lock() {
        let snapshot = t.scheduler_snapshot();
        let line = alloc::format!(
            "run_queue=[{}] current=[{}] idle=[{}] procs={} zombies={}",
            {
                let mut s = alloc::string::String::new();
                for (i, p) in snapshot.run_queue.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(alloc::format!("{}", p).as_str());
                }
                s
            },
            {
                let mut s = alloc::string::String::new();
                for i in 0..crate::process::table::MAX_CPUS {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(alloc::format!("{}", snapshot.current[i]).as_str());
                }
                s
            },
            {
                let mut s = alloc::string::String::new();
                for i in 0..crate::process::table::MAX_CPUS {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(alloc::format!("{}", snapshot.idle[i]).as_str());
                }
                s
            },
            t.procs.len(),
            t.zombies.len(),
        );
        let current = t
            .current_ref()
            .map(|p| (p.pid, p.name.clone()))
            .unwrap_or((0, alloc::string::String::from("<none>")));
        (line, current.0, current.1)
    } else {
        (
            alloc::string::String::from("TABLE lock held - try_lock failed"),
            0,
            alloc::string::String::from("<unknown>"),
        )
    };

    crate::serial_println!("\n[watchdog] NO FORWARD PROGRESS FOR {} s", secs_stalled);
    crate::serial_println!(
        "[watchdog] heartbeat={} last_progress={} timer_irqs={} boot_ticks={}",
        now_beat,
        last_prog,
        timer_irqs,
        boot_ticks
    );
    crate::serial_println!(
        "[watchdog] current pid={} name='{}' cr3={:#x}",
        current_pid,
        current_name,
        cr3
    );
    crate::serial_println!("[watchdog] {}", sched_line);
    crate::serial_println!(
        "[watchdog] sched stale_on_cpu_repairs={}",
        crate::process::scheduler::stale_on_cpu_repair_count()
    );
    dump_cpu_progress(crate::time::rdtsc());
    dump_progress_contract();
    crate::progress_contract::ProgressContract::emit_forward_progress_stall(
        secs_stalled,
        last_prog,
        now_beat,
        current_pid,
        cr3,
    );
    crate::observability_contract::ObservabilityContract::kds_metric_for(
        crate::kds::KdsSubsystem::Watchdog,
        crate::kds::KdsMetricId::WatchdogStallMs,
        secs_stalled.saturating_mul(1000),
        current_pid,
        current_pid,
        [last_prog, now_beat],
    );

    // Hand off to the panic handler.  Print a short message so the
    // panic banner is informative.
    panic!("watchdog: no forward progress for {} s", secs_stalled);
}
