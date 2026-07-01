use core::sync::atomic::{AtomicBool, Ordering};

use crate::console;
use crate::scheduler;

use super::engine::ShellEngine;

static STARTED: AtomicBool = AtomicBool::new(false);

fn sish_thread_entry() {
    console::println!("[BOOTCHK] shell.thread.entry");
    let mut engine = ShellEngine::new();
    engine.run();
}

pub fn start() -> Result<(), &'static str> {
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }

    console::println!("[BOOTCHK] shell.service.spawn");
    let _ = scheduler::spawn(sish_thread_entry);
    console::println!("[BOOTCHK] shell.service.spawned");
    Ok(())
}
