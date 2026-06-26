//! System journal — an in-kernel ring buffer of log messages (dmesg-style).
//!
//! Every line written through `print!`/`println!` (i.e. `vga_buffer::_print`)
//! is also captured here once the heap is up, so the boot log and runtime
//! messages can be reviewed from the shell with `journal` / `dmesg` even after
//! they have scrolled off-screen.  Each line is timestamped with the boot tick.
//!
//! Lock discipline: `log()` uses `try_lock` and never blocks — `_print` can be
//! reached from interrupt context (e.g. the page-fault handler), so the journal
//! must never deadlock against a mainline holder.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

/// Maximum number of lines retained (oldest dropped past this).
const MAX_LINES: usize = 2048;

static READY: AtomicBool = AtomicBool::new(false);
static IN_DUMP: AtomicBool = AtomicBool::new(false);

struct Journal {
    lines: VecDeque<(u64, String)>, // (boot_tick, line)
    cur: String,                    // line being assembled
}

static JOURNAL: Mutex<Option<Journal>> = Mutex::new(None);

/// Initialise the journal.  MUST be called after the heap exists.
pub fn init() {
    *JOURNAL.lock() = Some(Journal {
        lines: VecDeque::new(),
        cur: String::new(),
    });
    READY.store(true, Ordering::Relaxed);
    crate::println!("[journal] system log ready ({} lines)", MAX_LINES);
}

/// True once the journal can be written (heap ready) and not mid-dump.
#[inline]
pub fn ready() -> bool {
    READY.load(Ordering::Relaxed) && !IN_DUMP.load(Ordering::Relaxed)
}

/// Capture a chunk of output, splitting on newlines into journal lines.
pub fn log(s: &str) {
    if IN_DUMP.load(Ordering::Relaxed) {
        return;
    }
    // Non-blocking: skip if another context (or an IRQ) holds the lock.
    let mut guard = match JOURNAL.try_lock() {
        Some(g) => g,
        None => return,
    };
    let Some(j) = guard.as_mut() else { return };
    let tick = crate::shell::commands::boot_ticks();
    for ch in s.chars() {
        if ch == '\n' {
            let line = core::mem::take(&mut j.cur);
            j.lines.push_back((tick, line));
            if j.lines.len() > MAX_LINES {
                j.lines.pop_front();
            }
        } else if ch != '\r' {
            j.cur.push(ch);
        }
    }
}

/// Print the journal to the console.  `tail` limits to the last N lines (0 = all);
/// `filter`, if set, only shows lines containing it.
pub fn dump(tail: usize, filter: Option<&str>) {
    // Snapshot under the lock, then print with IN_DUMP set so our own output
    // is not re-captured (which would recurse / duplicate).
    let snapshot: Vec<(u64, String)> = {
        match JOURNAL
            .try_lock()
            .and_then(|g| g.as_ref().map(|j| j.lines.iter().cloned().collect()))
        {
            Some(v) => v,
            None => {
                crate::println!("journal: unavailable");
                return;
            }
        }
    };

    let filtered: Vec<&(u64, String)> = snapshot
        .iter()
        .filter(|(_, l)| filter.is_none_or(|f| l.contains(f)))
        .collect();
    let start = if tail > 0 && filtered.len() > tail {
        filtered.len() - tail
    } else {
        0
    };

    IN_DUMP.store(true, Ordering::Relaxed);
    for (tick, line) in &filtered[start..] {
        // Seconds since boot at ~18 Hz PIT.
        let secs = *tick / 18;
        let frac = (*tick % 18) * 100 / 18;
        crate::println!("[{:>5}.{:02}] {}", secs, frac, line);
    }
    crate::println!(
        "-- {} lines ({} total) --",
        filtered.len().saturating_sub(start),
        snapshot.len()
    );
    IN_DUMP.store(false, Ordering::Relaxed);
}
