use core::sync::atomic::{AtomicBool, Ordering};

use crate::console;
use crate::kernel::process;
use crate::scheduler;

use super::engine::ShellEngine;

static STARTED: AtomicBool = AtomicBool::new(false);

fn sish_thread_entry() {
    console::println!("[BOOTCHK] shell.thread.entry");
    let _ = process::start_pid1("/system/init");
    let mut engine = ShellEngine::new();
    let _ = engine.execute_line("source /system/init");
    let _ = process::finish_pid1(0);
    let _ = process::ensure_shell_process("sish");
    console::println!("Launching SISH...");
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
