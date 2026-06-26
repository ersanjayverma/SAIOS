use crate::println;

pub fn diag_str(args: &str) {
    let mut parts: [&str; 4] = [""; 4];
    let mut n = 0usize;
    for tok in args.split_whitespace() {
        if n < parts.len() {
            parts[n] = tok;
            n += 1;
        }
    }
    let argv: &[&str] = &parts[..n];
    if argv.is_empty() {
        let sched = crate::diag::diag_sched_on();
        let proc_ = crate::diag::diag_proc_on();
        let heartbeat =
            crate::diag::heartbeat::HEARTBEAT_COUNT.load(core::sync::atomic::Ordering::Relaxed);
        let timer = crate::interrupts::TIMER_IRQS.load(core::sync::atomic::Ordering::Relaxed);
        let kb = crate::interrupts::KB_IRQS.load(core::sync::atomic::Ordering::Relaxed);
        let mouse = crate::interrupts::MOUSE_IRQS.load(core::sync::atomic::Ordering::Relaxed);
        println!("diag flags:  sched={}  proc={}", yes(sched), yes(proc_));
        println!("heartbeat count = {}", heartbeat);
        println!("IRQ counters:  timer={}  kb={}  mouse={}", timer, kb, mouse);
        println!(
            "watchdog timeout = {} s",
            crate::diag::watchdog::TIMEOUT_SECS
        );
        println!("use: diag sched on|off  /  diag proc on|off  /  diag freeze [N]");
        return;
    }
    match argv[0] {
        "sched" => {
            if argv.len() < 2 {
                println!("usage: diag sched on|off");
                return;
            }
            let on = is_on(argv[1]);
            crate::diag::set_flag(crate::diag::diag_sched_bit(), on);
            println!("[sched] prints: {}", if on { "ON" } else { "OFF" });
        }
        "proc" => {
            if argv.len() < 2 {
                println!("usage: diag proc on|off");
                return;
            }
            let on = is_on(argv[1]);
            crate::diag::set_flag(crate::diag::diag_proc_bit(), on);
            println!("[proc] prints: {}", if on { "ON" } else { "OFF" });
        }
        "freeze" => {
            let secs: u64 = argv.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
            println!(
                "[diag] freeze repro: watchdog timing will run for {} s; use `diag` to inspect heartbeat counters",
                secs
            );
            let start_hb =
                crate::diag::heartbeat::HEARTBEAT_COUNT.load(core::sync::atomic::Ordering::Relaxed);
            let target = start_hb + secs;
            let start_ns = crate::time::uptime_ns();
            loop {
                if crate::diag::heartbeat::HEARTBEAT_COUNT
                    .load(core::sync::atomic::Ordering::Relaxed)
                    >= target
                {
                    break;
                }
                if (crate::time::uptime_ns().wrapping_sub(start_ns)) as u128
                    > (secs as u128 + 5) * 1_000_000_000
                {
                    break;
                }
                x86_64::instructions::hlt();
                if let Some(ev) = crate::driver::keyboard::poll() {
                    use crate::driver::keyboard::KeyEvent;
                    if matches!(
                        ev,
                        KeyEvent::Char('q') | KeyEvent::Char('\x03') | KeyEvent::Escape
                    ) {
                        println!("[diag] freeze repro aborted by user");
                        break;
                    }
                }
            }
            let end_hb =
                crate::diag::heartbeat::HEARTBEAT_COUNT.load(core::sync::atomic::Ordering::Relaxed);
            println!(
                "[diag] freeze repro done. heartbeats: {} -> {} ({} s)",
                start_hb,
                end_hb,
                end_hb - start_hb
            );
        }
        _ => println!(
            "diag: unknown subcommand '{}' (try: sched, proc, freeze)",
            argv[0]
        ),
    }
}

fn is_on(s: &str) -> bool {
    s == "on" || s == "1" || s == "yes" || s == "true"
}

fn yes(b: bool) -> &'static str {
    if b { "ON" } else { "off" }
}

pub fn help_diagnostics() {
    println!("  Diagnostics:");
    println!("    diag               show diagnostic flags + IRQ counters");
    println!("    diag sched on|off  toggle [sched] pid A -> pid B prints");
    println!("    diag proc  on|off  toggle [proc] create/start/exit prints");
    println!("    diag freeze [N]    run heartbeat for N s to repro a freeze");
}
