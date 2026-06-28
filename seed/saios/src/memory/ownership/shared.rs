use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub struct SharedCounter {
    refs: AtomicUsize,
}

impl SharedCounter {
    pub const fn new() -> Self {
        Self {
            refs: AtomicUsize::new(1),
        }
    }

    pub fn retain(&self) -> usize {
        self.refs.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn release(&self) -> usize {
        self.refs.fetch_sub(1, Ordering::SeqCst) - 1
    }
}
