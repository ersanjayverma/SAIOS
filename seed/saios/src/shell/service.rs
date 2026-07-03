use core::sync::atomic::{AtomicBool, Ordering};

use crate::console;
use crate::kernel::process;
use crate::scheduler;

use super::engine::ShellEngine;

static STARTED: AtomicBool = AtomicBool::new(false);

fn sish_thread_entry() {
    console::println!("[BOOTCHK] shell.thread.entry");
    console::println!("[BOOTCHK] shell.thread.pid1.start");
    let _ = process::start_pid1("/system/init");
    console::println!("[BOOTCHK] shell.thread.pid1.started");
    let mut engine = ShellEngine::new();
    let _ = engine.execute_line("source /system/init");
    let _ = process::finish_pid1(0);
    let _ = process::ensure_shell_process("snsh");
    console::println!("Launching SNSH...");
    let _ = engine.execute_line("clear");
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
    // Hand off once so the shell thread can run immediately even before timer preemption.
    scheduler::yield_now();
    console::println!("[BOOTCHK] shell.service.yielded");
    Ok(())
}
