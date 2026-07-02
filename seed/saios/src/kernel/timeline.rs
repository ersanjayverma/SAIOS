use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

#[derive(Clone, Debug)]
pub struct TimelineMark {
    pub tick: u64,
    pub label: String,
}

struct Timeline {
    marks: Vec<TimelineMark>,
}

impl Timeline {
    fn new() -> Self {
        Self { marks: Vec::new() }
    }

    fn mark(&mut self, label: &str) {
        let tick = crate::timer::ticks();
        self.marks.push(TimelineMark {
            tick,
            label: label.to_string(),
        });
        if self.marks.len() > 256 {
            let _ = self.marks.remove(0);
        }
    }
}

static TL: StaticCell<Option<Timeline>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_tl_mut<R>(f: impl FnOnce(&mut Timeline) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *TL.get() };
    if slot.is_none() {
        *slot = Some(Timeline::new());
    }
    let out = f(slot.as_mut().expect("timeline unavailable"));
    unlock();
    out
}

fn with_tl<R>(f: impl FnOnce(&Timeline) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *TL.get() };
    if slot.is_none() {
        *slot = Some(Timeline::new());
    }
    let out = f(slot.as_ref().expect("timeline unavailable"));
    unlock();
    out
}

pub fn init() {
    with_tl_mut(|_| {});
}

pub fn mark(label: &str) {
    with_tl_mut(|tl| tl.mark(label));
}

pub fn recent(limit: usize) -> Vec<TimelineMark> {
    with_tl(|tl| {
        let take = core::cmp::min(limit, tl.marks.len());
        tl.marks[tl.marks.len().saturating_sub(take)..].to_vec()
    })
}
