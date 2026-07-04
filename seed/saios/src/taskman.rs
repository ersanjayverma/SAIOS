//! TASKMAN — SAIOS system task and service manager.
//!
//! Invoked as a /bin binary via `exec taskman [args]`.
//!
//! Usage:
//!   taskman                       Full view: system + processes + services
//!   taskman proc  / -p            Processes only
//!   taskman svc   / -s            Services only
//!   taskman kill <pid>            Terminate a process
//!   taskman svc start   <name>    Start a service
//!   taskman svc stop    <name>    Stop a service
//!   taskman svc restart <name>    Restart a service
//!   taskman svc health            Service health summary
//!   taskman help                  Show this help text

use alloc::string::String;

use crate::console;
use crate::kernel::process;
use crate::kernel::telemetry;
use crate::ksf;
use crate::scheduler;
use crate::som::HealthState;
use crate::timer;

type TaskmanResult = Result<i32, &'static str>;

// ─────────────────────────────────────────────────────────────────────────────
//  Constants
// ─────────────────────────────────────────────────────────────────────────────

const WIDTH: usize = 66;

// ─────────────────────────────────────────────────────────────────────────────
//  Display helpers
// ─────────────────────────────────────────────────────────────────────────────

fn rule() {
    // 66 box-drawing horizontal bars
    console::println!("══════════════════════════════════════════════════════════════════");
}

fn thin_rule() {
    console::println!("──────────────────────────────────────────────────────────────────");
}

fn section(title: &str) {
    // Centre the title inside a 66-char wide banner
    let inner = WIDTH - 4; // leave "══ " and " ══"
    let pad = inner.saturating_sub(title.len());
    let left = pad / 2;
    let right = pad - left;

    let mut line = String::new();
    line.push_str("══");
    for _ in 0..left {
        line.push('═');
    }
    line.push(' ');
    line.push_str(title);
    line.push(' ');
    for _ in 0..right {
        line.push('═');
    }
    line.push_str("══");
    console::println!("{}", line);
}

fn process_state_label(s: &process::ProcessState) -> &'static str {
    match s {
        process::ProcessState::Running => "Running",
        process::ProcessState::Waiting => "Waiting",
        process::ProcessState::Exited => "Exited ",
    }
}

fn service_state_label(s: ksf::ServiceState) -> &'static str {
    match s {
        ksf::ServiceState::Registered => "Registered",
        ksf::ServiceState::Initializing => "Init      ",
        ksf::ServiceState::Ready => "Ready     ",
        ksf::ServiceState::Running => "Running   ",
        ksf::ServiceState::Paused => "Paused    ",
        ksf::ServiceState::Stopping => "Stopping  ",
        ksf::ServiceState::Stopped => "Stopped   ",
        ksf::ServiceState::Failed => "FAILED    ",
    }
}

fn health_label(h: HealthState) -> &'static str {
    match h {
        HealthState::Healthy => "OK     ",
        HealthState::Warning => "WARN   ",
        HealthState::Critical => "CRIT   ",
        HealthState::Offline => "OFFLINE",
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Sections
// ─────────────────────────────────────────────────────────────────────────────

fn print_system_summary() {
    let t = telemetry::snapshot();
    let uptime = timer::uptime();
    let secs = uptime.as_secs();
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    let threads = scheduler::threads();
    let svc_count = ksf::list().len();

    section("SAIOS TASK MANAGER");
    console::println!("  Uptime        {:02}:{:02}:{:02}", hours, minutes, seconds);
    console::println!("  CPU Logical   {}", t.cpu_logical);
    console::println!("  RAM           {} MB", t.ram_mb);
    console::println!(
        "  Heap          {} KB used  /  {} KB total",
        t.heap_used_kb,
        t.heap_total_kb
    );
    console::println!("  Threads       {}", threads.len());
    console::println!("  Processes     {}", t.process_count);
    console::println!("  Services      {}", svc_count);
    console::println!("  IRQ Ticks     {}", t.irq_total);
    console::println!("  Events        {}", t.event_total);
    rule();
}

fn print_processes() {
    section("PROCESSES");
    console::println!(
        "  {:>6}  {:<10}  {:>7}  {}",
        "PID",
        "STATE",
        "THREADS",
        "NAME"
    );
    thin_rule();

    let mut jobs = process::jobs();
    jobs.sort_by_key(|p| p.pid);

    for p in &jobs {
        console::println!(
            "  {:>6}  {:<10}  {:>7}  {}",
            p.pid,
            process_state_label(&p.state),
            p.thread_count,
            p.name
        );
    }

    if jobs.is_empty() {
        console::println!("  (no processes)");
    }

    thin_rule();
    console::println!("  {} process(es) total", jobs.len());
    rule();
}

fn print_services() {
    section("SERVICES");
    console::println!(
        "  {:>4}  {:<10}  {:<7}  {:<7}  {}",
        "ID",
        "STATE",
        "HEALTH",
        "VER",
        "NAME"
    );
    thin_rule();

    let svcs = ksf::list();

    for s in &svcs {
        console::println!(
            "  {:>4}  {:<10}  {:<7}  {:<7}  {}",
            s.id.0,
            service_state_label(s.state),
            health_label(s.health),
            s.version,
            s.name
        );
    }

    if svcs.is_empty() {
        console::println!("  (no services registered)");
    }

    thin_rule();
    console::println!("  {} service(s) total", svcs.len());
    rule();
}

fn print_service_health() {
    section("SERVICE HEALTH");
    thin_rule();

    let health = ksf::health();

    for (name, state) in &health {
        console::println!("  {:<7}  {}", health_label(*state), name);
    }

    if health.is_empty() {
        console::println!("  (no services registered)");
    }

    rule();
}

fn print_help() {
    section("TASKMAN — SAIOS Task Manager");
    console::println!("  taskman                       Full view (system + processes + services)");
    console::println!("  taskman proc  / -p            Process list");
    console::println!("  taskman svc   / -s            Service list");
    console::println!("  taskman kill <pid>            Terminate a process by PID");
    console::println!("  taskman svc start   <name>    Start a service");
    console::println!("  taskman svc stop    <name>    Stop a service");
    console::println!("  taskman svc restart <name>    Restart a service");
    console::println!("  taskman svc health            Service health summary");
    console::println!("  taskman help                  Show this help");
    rule();
}

// ─────────────────────────────────────────────────────────────────────────────
//  Entry point
// ─────────────────────────────────────────────────────────────────────────────

pub fn run(args: &[&str], _env: &[(String, String)]) -> TaskmanResult {
    match args.first().copied() {
        // ── process kill ────────────────────────────────────────────────────
        Some("kill") => {
            let pid = args
                .get(1)
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or("taskman kill: usage: taskman kill <pid>")?;
            process::kill(pid)?;
            console::println!("taskman: process {} terminated", pid);
            Ok(0)
        }

        // ── service sub-commands ────────────────────────────────────────────
        Some("svc") | Some("-s") => {
            // Check whether the first arg was "-s" (shorthand for svc list)
            let is_shorthand = args.first().copied() == Some("-s");
            let sub = if is_shorthand {
                None
            } else {
                args.get(1).copied()
            };

            match sub {
                Some("start") => {
                    let name = args
                        .get(2)
                        .copied()
                        .ok_or("taskman svc start: missing service name")?;
                    ksf::start(name)?;
                    console::println!("taskman: service '{}' started", name);
                    Ok(0)
                }
                Some("stop") => {
                    let name = args
                        .get(2)
                        .copied()
                        .ok_or("taskman svc stop: missing service name")?;
                    ksf::stop(name)?;
                    console::println!("taskman: service '{}' stopped", name);
                    Ok(0)
                }
                Some("restart") => {
                    let name = args
                        .get(2)
                        .copied()
                        .ok_or("taskman svc restart: missing service name")?;
                    ksf::restart(name)?;
                    console::println!("taskman: service '{}' restarted", name);
                    Ok(0)
                }
                Some("health") => {
                    print_service_health();
                    Ok(0)
                }
                // "svc list" or "svc" with no sub-command or "-s" shorthand
                Some("list") | None => {
                    print_services();
                    Ok(0)
                }
                Some(_) => {
                    Err("taskman svc: unknown subcommand; try start|stop|restart|health|list")
                }
            }
        }

        // ── process list ────────────────────────────────────────────────────
        Some("proc") | Some("-p") => {
            print_processes();
            Ok(0)
        }

        // ── help ────────────────────────────────────────────────────────────
        Some("help") | Some("-h") | Some("--help") => {
            print_help();
            Ok(0)
        }

        // ── full view (default, no args) ─────────────────────────────────────
        None => {
            print_system_summary();
            print_processes();
            print_services();
            Ok(0)
        }

        Some(_) => Err("taskman: unknown command; run 'taskman help'"),
    }
}
