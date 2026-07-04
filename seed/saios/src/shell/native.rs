use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::console;
use crate::driver::dhcp;
use crate::driver::network;
use crate::driver::storage as disk;
use crate::driver::usb;
use crate::heap;
use crate::kernel::crt;
use crate::kernel::device;
use crate::kernel::driver;
use crate::kernel::event;
use crate::kernel::object as kom;
use crate::kernel::package_image;
use crate::kernel::process;
use crate::kernel::sairu;
use crate::kernel::syscall;
use crate::kernel::telemetry;
use crate::kernel::testing;
use crate::kernel::timeline;
use crate::ksf;
use crate::object_manager;
use crate::pci;
use crate::pmm;
use crate::saifs;
use crate::scheduler;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::regex as shell_regex;
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;
use crate::timer;
use crate::vfs;

#[derive(Clone, Default)]
struct DetectSnapshot {
    pci: Vec<String>,
    devices: Vec<String>,
    usb: Vec<String>,
    ahci_controllers: Vec<String>,
    ahci_disks: Vec<String>,
    storage_fs: Vec<String>,
}

static DETECT_LOCK: AtomicBool = AtomicBool::new(false);
static DETECT_BASELINE: StaticCell<Option<DetectSnapshot>> = StaticCell::new(None);

fn detect_lock() {
    while DETECT_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn detect_unlock() {
    DETECT_LOCK.store(false, Ordering::Release);
}

fn with_detect_baseline_mut<R>(f: impl FnOnce(&mut Option<DetectSnapshot>) -> R) -> R {
    detect_lock();
    let out = {
        // SAFETY: guarded by spin lock.
        let slot = unsafe { &mut *DETECT_BASELINE.get() };
        f(slot)
    };
    detect_unlock();
    out
}

fn push_unique(out: &mut Vec<String>, value: String) {
    if !out.iter().any(|v| v == &value) {
        out.push(value);
    }
}

fn snapshot_collect(live_probe: bool) -> DetectSnapshot {
    let mut snap = DetectSnapshot::default();

    for d in pci::devices() {
        push_unique(
            &mut snap.pci,
            format!(
                "{:02x}:{:02x}.{} {:04x}:{:04x} cls={:02x}{:02x}{:02x}",
                d.bus,
                d.device,
                d.function,
                d.vendor_id,
                d.device_id,
                d.class,
                d.subclass,
                d.prog_if
            ),
        );
    }

    for d in device::devices() {
        push_unique(
            &mut snap.devices,
            format!("{} driver={} class={}", d.name, d.driver, d.class),
        );
    }

    let usb_list = if live_probe {
        usb::controllers()
    } else {
        usb::controllers_cached()
    };
    for u in usb_list {
        push_unique(
            &mut snap.usb,
            format!(
                "{} {:02x}:{:02x}.{} {:04x}:{:04x} {}",
                u.name, u.bus, u.device, u.function, u.vendor_id, u.device_id, u.kind
            ),
        );
    }

    let ahci_ctrls = if live_probe {
        crate::driver::ahci::controllers()
    } else {
        crate::driver::ahci::controllers_cached()
    };
    for c in ahci_ctrls {
        push_unique(
            &mut snap.ahci_controllers,
            format!(
                "{} {:02x}:{:02x}.{} {:04x}:{:04x}",
                c.name, c.bus, c.device, c.function, c.vendor_id, c.device_id
            ),
        );
    }

    let ahci_disks = if live_probe {
        crate::driver::ahci::disks()
    } else {
        crate::driver::ahci::disks_cached()
    };
    for d in ahci_disks {
        push_unique(
            &mut snap.ahci_disks,
            format!(
                "{} ctrl={} port={} sectors={} model={}",
                d.name, d.controller, d.port, d.total_sectors, d.model
            ),
        );
    }

    let volumes = if live_probe {
        disk::volumes()
    } else {
        disk::volumes_cached()
    };
    for v in volumes {
        let fs = v.filesystem.as_str();
        if !(fs.eq_ignore_ascii_case("fat32")
            || fs.eq_ignore_ascii_case("ntfs")
            || fs.eq_ignore_ascii_case("ext4"))
        {
            continue;
        }
        push_unique(
            &mut snap.storage_fs,
            format!(
                "{} fs={} backing={} size_mb={}",
                v.name,
                fs,
                v.backing,
                v.total_bytes / (1024 * 1024)
            ),
        );
    }

    snap
}

fn diff_new(current: &[String], baseline: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for item in current {
        if !baseline.iter().any(|b| b == item) {
            out.push(item.clone());
        }
    }
    out
}

fn print_detect_section(title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    console::println!("{}", title);
    for item in items {
        console::println!("  + {}", item);
    }
}

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "help",
        description: "List registered commands",
        handler: cmd_help,
    }));
    registry.register(Box::new(StaticCommand {
        name: "registry",
        description: "Show command registry",
        handler: cmd_registry,
    }));
    registry.register(Box::new(StaticCommand {
        name: "echo",
        description: "Print text to console",
        handler: cmd_echo,
    }));
    registry.register(Box::new(StaticCommand {
        name: "grep",
        description: "Filter stdin by regex",
        handler: cmd_grep,
    }));
    registry.register(Box::new(StaticCommand {
        name: "wc",
        description: "Count stdin lines/words/bytes",
        handler: cmd_wc,
    }));
    registry.register(Box::new(StaticCommand {
        name: "version",
        description: "Show kernel version",
        handler: cmd_version,
    }));
    registry.register(Box::new(StaticCommand {
        name: "clear",
        description: "Clear console output",
        handler: cmd_clear,
    }));
    registry.register(Box::new(StaticCommand {
        name: "exit",
        description: "Exit shell session",
        handler: cmd_exit,
    }));
    registry.register(Box::new(StaticCommand {
        name: "history",
        description: "Show command history",
        handler: cmd_history,
    }));
    registry.register(Box::new(StaticCommand {
        name: "time",
        description: "Show monotonic system time",
        handler: cmd_time,
    }));
    registry.register(Box::new(StaticCommand {
        name: "mem",
        description: "Show memory usage",
        handler: cmd_mem,
    }));
    registry.register(Box::new(StaticCommand {
        name: "memory",
        description: "Alias for mem",
        handler: cmd_mem,
    }));
    registry.register(Box::new(StaticCommand {
        name: "cpu",
        description: "Show CPU information",
        handler: cmd_cpu,
    }));
    registry.register(Box::new(StaticCommand {
        name: "ps",
        description: "List active threads",
        handler: cmd_ps,
    }));
    registry.register(Box::new(StaticCommand {
        name: "jobs",
        description: "List managed processes",
        handler: cmd_jobs,
    }));
    registry.register(Box::new(StaticCommand {
        name: "kill",
        description: "Kill process by pid",
        handler: cmd_kill,
    }));
    registry.register(Box::new(StaticCommand {
        name: "wait",
        description: "Wait for process exit by pid",
        handler: cmd_wait,
    }));
    registry.register(Box::new(StaticCommand {
        name: "dmesg",
        description: "Show recent kernel events",
        handler: cmd_dmesg,
    }));
    registry.register(Box::new(StaticCommand {
        name: "panic",
        description: "Trigger kernel panic",
        handler: cmd_panic,
    }));
    registry.register(Box::new(StaticCommand {
        name: "spawn",
        description: "Spawn program and print pid",
        handler: cmd_spawn,
    }));
    registry.register(Box::new(StaticCommand {
        name: "exec",
        description: "Execute program with args/env and return exit code",
        handler: cmd_exec,
    }));
    registry.register(Box::new(StaticCommand {
        name: "syscall",
        description: "Show or smoke-test stable syscall ABI",
        handler: cmd_syscall,
    }));
    registry.register(Box::new(StaticCommand {
        name: "crt",
        description: "Show or probe C runtime startup contract",
        handler: cmd_crt,
    }));
    registry.register(Box::new(StaticCommand {
        name: "pkgimg",
        description: "Show or remount package image profile",
        handler: cmd_pkgimg,
    }));
    registry.register(Box::new(StaticCommand {
        name: "env",
        description: "List shell environment variables",
        handler: cmd_env,
    }));
    registry.register(Box::new(StaticCommand {
        name: "setenv",
        description: "Set shell environment variable",
        handler: cmd_setenv,
    }));
    registry.register(Box::new(StaticCommand {
        name: "unsetenv",
        description: "Remove shell environment variable",
        handler: cmd_unsetenv,
    }));
    registry.register(Box::new(StaticCommand {
        name: "alias",
        description: "Create or list aliases",
        handler: cmd_alias,
    }));
    registry.register(Box::new(StaticCommand {
        name: "unalias",
        description: "Remove alias by name",
        handler: cmd_unalias,
    }));
    registry.register(Box::new(StaticCommand {
        name: "aliases",
        description: "List configured aliases",
        handler: cmd_aliases,
    }));
    registry.register(Box::new(StaticCommand {
        name: "status",
        description: "Show last exit code",
        handler: cmd_status,
    }));
    registry.register(Box::new(StaticCommand {
        name: "source",
        description: "Run script file in current shell context",
        handler: cmd_source,
    }));
    registry.register(Box::new(StaticCommand {
        name: ".",
        description: "Alias for source",
        handler: cmd_source,
    }));
    registry.register(Box::new(StaticCommand {
        name: "dashboard",
        description: "Show one-page system readiness dashboard",
        handler: cmd_dashboard,
    }));
    registry.register(Box::new(StaticCommand {
        name: "dash",
        description: "Alias for dashboard",
        handler: cmd_dashboard,
    }));
    registry.register(Box::new(StaticCommand {
        name: "stats",
        description: "Show KOM object statistics",
        handler: cmd_stats,
    }));
    registry.register(Box::new(StaticCommand {
        name: "st",
        description: "Alias for stats",
        handler: cmd_stats,
    }));
    registry.register(Box::new(StaticCommand {
        name: "objects",
        description: "List KOM objects (optionally filtered by type)",
        handler: cmd_objects,
    }));
    registry.register(Box::new(StaticCommand {
        name: "obj",
        description: "Alias for objects",
        handler: cmd_objects,
    }));
    registry.register(Box::new(StaticCommand {
        name: "providers",
        description: "List registered providers",
        handler: cmd_providers,
    }));
    registry.register(Box::new(StaticCommand {
        name: "devices",
        description: "List registered devices",
        handler: cmd_devices,
    }));
    registry.register(Box::new(StaticCommand {
        name: "dev",
        description: "Alias for devices",
        handler: cmd_devices,
    }));
    registry.register(Box::new(StaticCommand {
        name: "drivers",
        description: "List registered drivers",
        handler: cmd_drivers,
    }));
    registry.register(Box::new(StaticCommand {
        name: "drv",
        description: "Alias for drivers",
        handler: cmd_drivers,
    }));
    registry.register(Box::new(StaticCommand {
        name: "driver",
        description: "Inspect one driver",
        handler: cmd_driver,
    }));
    registry.register(Box::new(StaticCommand {
        name: "service",
        description: "Manage kernel services",
        handler: cmd_service,
    }));
    registry.register(Box::new(StaticCommand {
        name: "svc",
        description: "Alias for service",
        handler: cmd_service,
    }));
    registry.register(Box::new(StaticCommand {
        name: "restart",
        description: "Restart a kernel service",
        handler: cmd_restart,
    }));
    registry.register(Box::new(StaticCommand {
        name: "reload",
        description: "Reload a driver",
        handler: cmd_reload,
    }));
    registry.register(Box::new(StaticCommand {
        name: "test",
        description: "Run kernel test suites",
        handler: cmd_test,
    }));
    registry.register(Box::new(StaticCommand {
        name: "verify",
        description: "Verify runtime invariants",
        handler: cmd_verify,
    }));
    registry.register(Box::new(StaticCommand {
        name: "validate",
        description: "Run kernel validation suite",
        handler: cmd_validate,
    }));
    registry.register(Box::new(StaticCommand {
        name: "services",
        description: "List service objects",
        handler: cmd_services,
    }));
    registry.register(Box::new(StaticCommand {
        name: "svcs",
        description: "Alias for services",
        handler: cmd_services,
    }));
    registry.register(Box::new(StaticCommand {
        name: "query",
        description: "Run object query expression",
        handler: cmd_query,
    }));
    registry.register(Box::new(StaticCommand {
        name: "inspect",
        description: "Inspect one object",
        handler: cmd_inspect,
    }));
    registry.register(Box::new(StaticCommand {
        name: "describe",
        description: "Describe object via SIF/SAIFS handle",
        handler: cmd_describe,
    }));
    registry.register(Box::new(StaticCommand {
        name: "health",
        description: "Show system health summary",
        handler: cmd_health,
    }));
    registry.register(Box::new(StaticCommand {
        name: "diagnose",
        description: "Run diagnostics for object",
        handler: cmd_diagnose,
    }));
    registry.register(Box::new(StaticCommand {
        name: "explain",
        description: "Explain object behavior",
        handler: cmd_explain,
    }));
    registry.register(Box::new(StaticCommand {
        name: "events",
        description: "Show recent events",
        handler: cmd_events,
    }));
    registry.register(Box::new(StaticCommand {
        name: "ev",
        description: "Alias for events",
        handler: cmd_events,
    }));
    registry.register(Box::new(StaticCommand {
        name: "logs",
        description: "Alias for events",
        handler: cmd_events,
    }));
    registry.register(Box::new(StaticCommand {
        name: "storage",
        description: "Storage stack diagnostics and rescan",
        handler: cmd_storage,
    }));
    registry.register(Box::new(StaticCommand {
        name: "scan",
        description: "Rescan storage hardware and list volumes",
        handler: cmd_scan,
    }));
    registry.register(Box::new(StaticCommand {
        name: "hdd",
        description: "Check HDD/storage controllers, disks, and volumes",
        handler: cmd_hdd,
    }));
    registry.register(Box::new(StaticCommand {
        name: "drive",
        description: "Alias for hdd",
        handler: cmd_hdd,
    }));
    registry.register(Box::new(StaticCommand {
        name: "drives",
        description: "Alias for hdd",
        handler: cmd_hdd,
    }));
    registry.register(Box::new(StaticCommand {
        name: "disks",
        description: "Alias for volumes disks",
        handler: cmd_disks,
    }));
    registry.register(Box::new(StaticCommand {
        name: "volume",
        description: "Alias for volumes",
        handler: cmd_volumes,
    }));
    registry.register(Box::new(StaticCommand {
        name: "diskpart",
        description: "Alias for volumes (disk/volume management)",
        handler: cmd_volumes,
    }));
    registry.register(Box::new(StaticCommand {
        name: "volumes",
        description: "Manage storage volumes, disks, scan, and mounts",
        handler: cmd_volumes,
    }));
    registry.register(Box::new(StaticCommand {
        name: "mount",
        description: "Mount storage volume or list mounts; see 'mount help'",
        handler: cmd_mount,
    }));
    registry.register(Box::new(StaticCommand {
        name: "umount",
        description: "Unmount a mounted path",
        handler: cmd_umount,
    }));
    registry.register(Box::new(StaticCommand {
        name: "df",
        description: "Show mounted filesystem usage",
        handler: cmd_df,
    }));
    registry.register(Box::new(StaticCommand {
        name: "graph",
        description: "Show dependency graph (e.g. graph services)",
        handler: cmd_graph,
    }));
    registry.register(Box::new(StaticCommand {
        name: "gr",
        description: "Alias for graph",
        handler: cmd_graph,
    }));
    registry.register(Box::new(StaticCommand {
        name: "timeline",
        description: "Show boot and service timeline",
        handler: cmd_timeline,
    }));
    registry.register(Box::new(StaticCommand {
        name: "tl",
        description: "Alias for timeline",
        handler: cmd_timeline,
    }));
    registry.register(Box::new(StaticCommand {
        name: "tree",
        description: "Render SAIFS directory tree",
        handler: cmd_tree,
    }));
    registry.register(Box::new(StaticCommand {
        name: "threads",
        description: "List scheduler threads",
        handler: cmd_threads,
    }));
    registry.register(Box::new(StaticCommand {
        name: "uptime",
        description: "Show system uptime",
        handler: cmd_uptime,
    }));
    registry.register(Box::new(StaticCommand {
        name: "ticks",
        description: "Show timer ticks",
        handler: cmd_ticks,
    }));
    registry.register(Box::new(StaticCommand {
        name: "irq",
        description: "Show interrupt counters",
        handler: cmd_irq,
    }));
    registry.register(Box::new(StaticCommand {
        name: "heap",
        description: "Show heap usage",
        handler: cmd_heap,
    }));
    registry.register(Box::new(StaticCommand {
        name: "pci",
        description: "List PCI devices",
        handler: cmd_pci,
    }));
    registry.register(Box::new(StaticCommand {
        name: "detect",
        description: "Detect newly available hardware since previous snapshot",
        handler: cmd_detect,
    }));
    registry.register(Box::new(StaticCommand {
        name: "usb",
        description: "List or rescan USB host controllers",
        handler: cmd_usb,
    }));
    registry.register(Box::new(StaticCommand {
        name: "net",
        description: "Network stack control and status",
        handler: cmd_net,
    }));
    registry.register(Box::new(StaticCommand {
        name: "dhcp",
        description: "Renew DHCP lease and show IPv4 config",
        handler: cmd_dhcp,
    }));
    registry.register(Box::new(StaticCommand {
        name: "ping",
        description: "Send IPv4 ICMP echo requests",
        handler: cmd_ping,
    }));
    registry.register(Box::new(StaticCommand {
        name: "wget",
        description: "HTTP download to local filesystem",
        handler: cmd_wget,
    }));
    registry.register(Box::new(StaticCommand {
        name: "shutdown",
        description: "Shutdown kernel (halt)",
        handler: cmd_shutdown,
    }));
    registry.register(Box::new(StaticCommand {
        name: "reboot",
        description: "Reboot machine",
        handler: cmd_reboot,
    }));
    registry.register(Box::new(StaticCommand {
        name: "sairu",
        description: "SAIRU diagnostics interface",
        handler: cmd_sairu,
    }));
    registry.register(Box::new(StaticCommand {
        name: "recover",
        description: "Run automated recovery actions",
        handler: cmd_recover,
    }));
    registry.register(Box::new(StaticCommand {
        name: "rcv",
        description: "Alias for recover",
        handler: cmd_recover,
    }));
}

fn cmd_help(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!(
        "namespace={} env_vars={}",
        ctx.session.current_namespace,
        ctx.session.environment.len()
    );
    print_command_table(ctx);
    Ok(())
}

fn cmd_registry(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    print_command_table(ctx);
    Ok(())
}

fn print_command_table(ctx: &CommandContext) {
    let name_width = ctx
        .command_catalog
        .iter()
        .map(|item| item.name.len())
        .max()
        .unwrap_or(7)
        .max(7);

    console::println!("{:<width$}  DESCRIPTION", "COMMAND", width = name_width);
    console::println!("{:-<width$}  {:-<11}", "", "", width = name_width);

    for item in &ctx.command_catalog {
        console::println!(
            "{:<width$}  {}",
            item.name,
            item.description,
            width = name_width
        );
    }
}

fn cmd_version(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("SAIOS v1.0 SISH");
    Ok(())
}

fn cmd_echo(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() {
        if let Some(stdin) = _ctx.env_get("SISH_STDIN") {
            console::println!("{}", stdin);
        }
        return Ok(());
    }

    let mut first = true;
    for arg in args {
        if !first {
            console::print(" ");
        }
        console::print(arg);
        first = false;
    }
    console::newline();
    Ok(())
}

fn cmd_grep(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let (literal, pattern) = match args {
        ["-F", pat, ..] => (true, *pat),
        [pat, ..] => (false, *pat),
        _ => return Err("grep: missing pattern"),
    };

    let input = ctx.env_get("SISH_STDIN").ok_or("grep: no stdin")?;
    for line in input.lines() {
        let matched = if literal {
            line.contains(pattern)
        } else {
            shell_regex::is_match(pattern, line)?
        };
        if matched {
            console::println!("{}", line);
        }
    }
    Ok(())
}

fn cmd_wc(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let input = ctx.env_get("SISH_STDIN").ok_or("wc: no stdin")?;
    let lines = input.lines().count();
    let words = input.split_whitespace().count();
    let bytes = input.len();
    console::println!("{} {} {}", lines, words, bytes);
    Ok(())
}

fn cmd_clear(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::clear();
    Ok(())
}

fn cmd_exit(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    ctx.session.running = false;
    Ok(())
}

fn cmd_history(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    for (idx, line) in ctx.session.history.iter().enumerate() {
        console::println!("{} {}", idx + 1, line);
    }
    Ok(())
}

fn cmd_time(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let uptime = timer::uptime();
    let total_ms = uptime.as_millis() as u64;
    console::println!("ticks={} monotonic_ms={}", timer::ticks(), total_ms);
    Ok(())
}

fn cmd_mem(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("Total RAM : {} MB", pmm::total_ram_mb());
    console::println!("Pages     : {}", pmm::total_pages());
    console::println!("Used      : {}", pmm::used_pages());
    console::println!("Free      : {}", pmm::free_pages());
    Ok(())
}

fn trim_nul_bytes(bytes: &[u8]) -> String {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 {
        end -= 1;
    }
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

fn cmd_cpu(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let vendor = trim_nul_bytes(&hal::arch::x86_64::cpuid::vendor());
    let brand = trim_nul_bytes(&hal::arch::x86_64::cpuid::brand());
    let features = hal::arch::x86_64::cpuid::features();

    console::println!("Vendor : {}", vendor);
    console::println!("Brand  : {}", brand);
    console::println!(
        "Logical processors : {}",
        hal::arch::x86_64::cpuid::logical_processors()
    );
    console::println!(
        "Features: apic={} msr={} tsc={} sse={} sse2={} avx={}",
        features.apic,
        features.msr,
        features.tsc,
        features.sse,
        features.sse2,
        features.avx
    );
    Ok(())
}

fn thread_state_label(state: scheduler::ThreadState) -> &'static str {
    match state {
        scheduler::ThreadState::Ready => "ready",
        scheduler::ThreadState::Running => "running",
        scheduler::ThreadState::Sleeping => "sleeping",
        scheduler::ThreadState::Blocked => "blocked",
        scheduler::ThreadState::Dead => "dead",
    }
}

fn process_state_label(state: process::ProcessState) -> &'static str {
    match state {
        process::ProcessState::Running => "running",
        process::ProcessState::Waiting => "waiting",
        process::ProcessState::Exited => "exited",
    }
}

fn device_status_label(status: device::DeviceStatus) -> &'static str {
    match status {
        device::DeviceStatus::Online => "online",
        device::DeviceStatus::Offline => "offline",
        device::DeviceStatus::Faulted => "faulted",
    }
}

fn driver_status_label(status: driver::DriverStatus) -> &'static str {
    match status {
        driver::DriverStatus::Loaded => "loaded",
        driver::DriverStatus::Running => "running",
        driver::DriverStatus::Stopped => "stopped",
        driver::DriverStatus::Faulted => "faulted",
    }
}

fn service_state_label(state: ksf::ServiceState) -> &'static str {
    match state {
        ksf::ServiceState::Registered => "registered",
        ksf::ServiceState::Initializing => "init",
        ksf::ServiceState::Ready => "ready",
        ksf::ServiceState::Running => "running",
        ksf::ServiceState::Paused => "paused",
        ksf::ServiceState::Stopping => "stopping",
        ksf::ServiceState::Stopped => "stopped",
        ksf::ServiceState::Failed => "failed",
    }
}

fn health_state_label(state: crate::som::HealthState) -> &'static str {
    match state {
        crate::som::HealthState::Healthy => "healthy",
        crate::som::HealthState::Warning => "warning",
        crate::som::HealthState::Critical => "critical",
        crate::som::HealthState::Offline => "offline",
    }
}

fn cmd_ps(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("{:<8}  {:<10}", "ID", "STATE");
    for t in scheduler::threads() {
        console::println!("{:<8}  {:<10}", t.id, thread_state_label(t.state));
    }
    Ok(())
}

fn cmd_jobs(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let filter = _args.first().copied();
    let sort_desc = _args
        .iter()
        .skip(1)
        .find_map(|a| a.strip_prefix("sort="))
        .is_some_and(|v| v.eq_ignore_ascii_case("desc"));
    let filter = filter.map(|f| {
        if f.eq_ignore_ascii_case("r") {
            "running"
        } else if f.eq_ignore_ascii_case("w") {
            "waiting"
        } else if f.eq_ignore_ascii_case("e") {
            "exited"
        } else {
            f
        }
    });

    let mut records = process::jobs();
    records.sort_by_key(|p| p.pid);
    if sort_desc {
        records.reverse();
    }

    console::println!("{:<8}  {:<10}  {:<24}", "PID", "STATE", "NAME");
    for p in records {
        if let Some(f) = filter {
            let keep = if f.eq_ignore_ascii_case("running") {
                matches!(p.state, process::ProcessState::Running)
            } else if f.eq_ignore_ascii_case("waiting") {
                matches!(p.state, process::ProcessState::Waiting)
            } else if f.eq_ignore_ascii_case("exited") {
                matches!(p.state, process::ProcessState::Exited)
            } else {
                p.name.contains(f)
            };
            if !keep {
                continue;
            }
        }
        console::println!(
            "{:<8}  {:<10}  {:<24.24}",
            p.pid,
            process_state_label(p.state),
            p.name
        );
    }
    Ok(())
}

fn cmd_kill(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let pid = args
        .first()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or("kill: missing pid")?;
    process::kill(pid)
}

fn cmd_wait(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let pid = args
        .first()
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or("wait: missing pid")?;
    let code = process::wait(pid)?;
    console::println!("wait: pid={} exit={}", pid, code);
    Ok(())
}

fn cmd_dmesg(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let limit = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(64);

    for line in object_manager::events(limit) {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_panic(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    panic!("panic command invoked")
}

fn cmd_spawn(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let program = args.first().copied().ok_or("spawn: missing program name")?;
    let program_args = &args[1..];
    let pid = process::spawn(program, program_args, ctx.session.environment.as_slice())?;
    console::println!("spawned pid={}", pid);
    Ok(())
}

fn is_env_assignment(token: &str) -> bool {
    match token.split_once('=') {
        Some((key, _)) => !key.is_empty(),
        None => false,
    }
}

fn upsert_env(env: &mut Vec<(String, String)>, key: &str, value: &str) {
    for (k, v) in env.iter_mut() {
        if k == key {
            *v = value.to_string();
            return;
        }
    }
    env.push((key.to_string(), value.to_string()));
}

fn remove_env(env: &mut Vec<(String, String)>, key: &str) {
    env.retain(|(k, _)| k != key);
}

fn cmd_exec(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() {
        return Err("exec: missing program name");
    }

    let mut idx = 0usize;
    let mut overlays: Vec<(String, String)> = Vec::new();

    while idx < args.len() && is_env_assignment(args[idx]) {
        let (key, value) = args[idx]
            .split_once('=')
            .ok_or("exec: invalid env assignment")?;
        overlays.push((key.to_string(), value.to_string()));
        idx += 1;
    }

    if idx >= args.len() {
        return Err("exec: missing program name");
    }

    let program = args[idx];
    let program_args = &args[idx + 1..];

    let saved_env = ctx.session.environment.clone();
    for (k, v) in &overlays {
        upsert_env(&mut ctx.session.environment, k.as_str(), v.as_str());
    }

    let run = process::exec(program, program_args, ctx.session.environment.as_slice());
    ctx.session.environment = saved_env;

    let exit_code = run?;
    ctx.session.last_exit_code = exit_code;
    if exit_code != 0 {
        console::println!("exit {}", exit_code);
    }
    Ok(())
}

fn cmd_env(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    for (k, v) in &ctx.session.environment {
        console::println!("{}={}", k, v);
    }
    Ok(())
}

fn cmd_setenv(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let key = args.first().copied().ok_or("setenv: missing key")?;
    let value = args.get(1).copied().ok_or("setenv: missing value")?;
    upsert_env(&mut ctx.session.environment, key, value);
    Ok(())
}

fn cmd_unsetenv(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let key = args.first().copied().ok_or("unsetenv: missing key")?;
    remove_env(&mut ctx.session.environment, key);
    Ok(())
}

fn cmd_aliases(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    for (name, value) in &ctx.session.aliases {
        console::println!("alias {}='{}'", name, value);
    }
    Ok(())
}

fn cmd_alias(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() {
        return cmd_aliases(ctx, args);
    }

    if let Some((name, value)) = args[0].split_once('=') {
        if name.is_empty() {
            return Err("alias: invalid name");
        }
        ctx.alias_set(name, value);
        return Ok(());
    }

    if args.len() < 2 {
        return Err("alias: usage alias NAME VALUE");
    }

    let name = args[0];
    let value = args[1..].join(" ");
    ctx.alias_set(name, value.as_str());
    Ok(())
}

fn cmd_unalias(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let name = args.first().copied().ok_or("unalias: missing name")?;
    ctx.alias_unset(name);
    Ok(())
}

fn cmd_status(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("{}", ctx.session.last_exit_code);
    Ok(())
}

fn cmd_source(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    Ok(())
}

fn cmd_syscall(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() || args.first().copied() == Some("abi") {
        let v = syscall::abi_version();
        console::println!("syscall.abi={}.{}.{}", v.major, v.minor, v.patch);
        console::println!("supported:");
        for n in syscall::supported() {
            console::println!("  {} {}", *n as u16, n.as_str());
        }
        return Ok(());
    }

    if args.first().copied() == Some("check") {
        let raw = args
            .get(1)
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or("syscall check: missing numeric id")?;
        let n = syscall::SyscallNumber::from_raw(raw).ok_or("syscall check: unknown id")?;
        console::println!("{} => {}", raw, n.as_str());
        return Ok(());
    }

    if args.first().copied() == Some("invoke") {
        let sel = args
            .get(1)
            .copied()
            .ok_or("syscall invoke: missing name or id")?;
        let number = if let Ok(raw) = sel.parse::<u64>() {
            syscall::SyscallNumber::from_raw(raw).ok_or("syscall invoke: unknown id")?
        } else {
            syscall::SyscallNumber::from_name(sel).ok_or("syscall invoke: unknown name")?
        };

        let arg0 = args.get(2).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);

        let req = syscall::SyscallRequest {
            number,
            args: [arg0, 0, 0, 0, 0, 0],
        };
        let ctx = syscall::SyscallContext { pid: 1 };

        match syscall::dispatch(req, ctx) {
            Ok(ret) => console::println!("ret={}", ret),
            Err(e) => console::println!("err={} code={}", e, e.code()),
        }
        return Ok(());
    }

    Err("usage: syscall [abi|check <id>|invoke <name|id> [arg0]]")
}

fn cmd_crt(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() || args.first().copied() == Some("abi") {
        let v = crt::abi_version();
        let s = crt::libc_surface();
        console::println!("crt.abi={}.{}.{}", v.major, v.minor, v.patch);
        console::println!(
            "surface crt0={} argv_envp={} malloc_free={} printf={}",
            s.crt0,
            s.argv_envp,
            s.malloc_free,
            s.printf
        );
        return Ok(());
    }

    if args.first().copied() == Some("probe") {
        let program = args.get(1).copied().unwrap_or("hello");
        let startup =
            crt::prepare_startup_block(program, &args[2..], ctx.session.environment.as_slice());
        console::println!("program={}", startup.program);
        console::println!("argc={}", startup.argc);
        for (i, a) in startup.argv.iter().enumerate() {
            console::println!("argv[{}]={}", i, a);
        }
        console::println!("envc={}", startup.envp.len());
        return Ok(());
    }

    Err("usage: crt [abi|probe <program> [args...]]")
}

fn cmd_pkgimg(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.first().copied() == Some("mount") || args.first().copied() == Some("remount") {
        package_image::mount_default()?;
    }

    let s = package_image::status();
    console::println!("mounted={}", s.mounted);
    console::println!("profile={}", s.profile);
    console::println!("manifest={}", s.manifest);
    console::println!("roots={} bins={}", s.roots, s.bins);
    Ok(())
}

fn cmd_dashboard(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let services = ksf::list();
    let telemetry = telemetry::snapshot();
    let kom_stats = kom::stats();
    let devices = device::devices();
    let drivers = driver::drivers();
    let jobs = process::jobs();
    let events = event::counters();
    let (score, warnings) = sairu::health_score();

    let service_failed = services
        .iter()
        .filter(|s| matches!(s.state, ksf::ServiceState::Failed))
        .count();
    let service_running = services
        .iter()
        .filter(|s| matches!(s.state, ksf::ServiceState::Running))
        .count();

    let service_warning = services
        .iter()
        .filter(|s| {
            matches!(
                s.health,
                crate::som::HealthState::Warning
                    | crate::som::HealthState::Critical
                    | crate::som::HealthState::Offline
            )
        })
        .count();

    let driver_faulted = drivers
        .iter()
        .filter(|d| matches!(d.status, driver::DriverStatus::Faulted))
        .count();
    let device_faulted = devices
        .iter()
        .filter(|d| matches!(d.status, device::DeviceStatus::Faulted))
        .count();

    let kom_ready = kom_stats.total > 0;
    let ksm_ready = !services.is_empty();
    let device_mgr_ready = !devices.is_empty();
    let driver_mgr_ready = !drivers.is_empty();
    let process_mgr_ready = !jobs.is_empty();
    let event_bus_ready = telemetry.event_total >= events.total;
    let sairu_ready = true;

    let d = timer::uptime();
    let total_ms = d.as_millis() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;

    console::println!("========================================");
    console::println!("SAIOS SYSTEM DASHBOARD");
    console::println!("========================================");
    console::println!("Boot            {}", if kom_ready { "OK" } else { "FAIL" });
    console::println!(
        "Memory          {}",
        if telemetry.ram_mb > 0 { "OK" } else { "FAIL" }
    );
    console::println!(
        "Scheduler       {}",
        if telemetry.scheduler_threads > 0 {
            "OK"
        } else {
            "FAIL"
        }
    );
    console::println!(
        "Processes       {} running",
        jobs.iter()
            .filter(|p| matches!(p.state, process::ProcessState::Running))
            .count()
    );
    console::println!(
        "Drivers         {}/{} healthy",
        drivers.len().saturating_sub(driver_faulted),
        drivers.len()
    );
    console::println!(
        "Devices         {} online",
        devices
            .iter()
            .filter(|d| matches!(d.status, device::DeviceStatus::Online))
            .count()
    );
    console::println!(
        "Filesystem      {}",
        if telemetry.mount_count > 0 {
            "Mounted"
        } else {
            "Not Mounted"
        }
    );
    console::println!(
        "Event Bus       {}",
        if event_bus_ready {
            "Active"
        } else {
            "Inactive"
        }
    );
    console::println!(
        "Telemetry       {}",
        if telemetry.event_total > 0 {
            "Active"
        } else {
            "Warmup"
        }
    );
    console::println!(
        "SAIRU           {}",
        if sairu_ready { "Healthy" } else { "Degraded" }
    );
    console::println!("CPU             {} logical", telemetry.cpu_logical);
    console::println!("RAM             {} MiB", telemetry.ram_mb);
    console::println!(
        "Heap            {:.1} MiB / {:.1} MiB",
        (telemetry.heap_used_kb as f64) / 1024.0,
        (telemetry.heap_total_kb as f64) / 1024.0
    );
    console::println!("Events          {}", telemetry.event_total);
    console::println!("Objects         {}", kom_stats.total);
    console::println!("Services        {}", services.len());
    console::println!("Uptime          {:02}:{:02}:{:02}", hours, minutes, seconds);
    console::println!("Overall Health  {}%", score);
    console::println!("----------------------------------------");
    if warnings.is_empty() {
        console::println!("Warnings: none");
    } else {
        console::println!("Warnings:");
        for w in warnings {
            console::println!("- {}", w);
        }
    }
    console::println!("----------------------------------------");
    console::println!(
        "READY  KOM={} KSM={} DEV={} DRV={} PROC={} EVT={} SAIRU={}",
        kom_ready,
        ksm_ready,
        device_mgr_ready,
        driver_mgr_ready,
        process_mgr_ready,
        event_bus_ready,
        sairu_ready
    );
    console::println!(
        "SERVICES running={} total={} failed={} warn={}",
        service_running,
        services.len(),
        service_failed,
        service_warning
    );
    console::println!(
        "DRIVERS total={} faulted={} | DEVICES total={} faulted={}",
        drivers.len(),
        driver_faulted,
        devices.len(),
        device_faulted
    );
    console::println!(
        "PROCESS jobs={} | THREADS={} | KOM objects={}",
        jobs.len(),
        telemetry.scheduler_threads,
        kom_stats.total
    );
    console::println!(
        "MEM ram={}MB heap={}KB/{}KB | CPU logical={} | IRQ={} | EVENTS={}",
        telemetry.ram_mb,
        telemetry.heap_used_kb,
        telemetry.heap_total_kb,
        telemetry.cpu_logical,
        telemetry.irq_total,
        telemetry.event_total
    );

    console::println!("========================================");

    Ok(())
}

fn cmd_graph(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let target = args.first().copied().unwrap_or("services");
    if !target.eq_ignore_ascii_case("services") {
        return Err("graph: expected services");
    }

    let services = ksf::list();
    if services.is_empty() {
        console::println!("Kernel");
        return Ok(());
    }

    console::println!("Kernel");

    fn print_children(
        parent_id: Option<ksf::ServiceId>,
        all: &[ksf::ServiceSnapshot],
        depth: usize,
    ) {
        for svc in all {
            let is_child = match parent_id {
                None => svc.dependencies.is_empty(),
                Some(pid) => svc.dependencies.contains(&pid),
            };
            if !is_child {
                continue;
            }

            let indent = "  ".repeat(depth);
            crate::console::println!("{}- {}", indent, svc.name);
            print_children(Some(svc.id), all, depth + 1);
        }
    }

    print_children(None, services.as_slice(), 1);
    Ok(())
}

fn cmd_timeline(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let limit = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(32);

    for m in timeline::recent(limit) {
        let ms = m.tick.saturating_mul(10);
        let sec = ms / 1000;
        let sub = ms % 1000;
        console::println!("{:02}.{:03} {}", sec, sub, m.label);
    }
    Ok(())
}

fn cmd_recover(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    for line in sairu::recover() {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_stats(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let t = telemetry::snapshot();
    let s = kom::stats();
    console::println!("CPU Logical : {}", t.cpu_logical);
    console::println!("RAM MB      : {}", t.ram_mb);
    console::println!("Heap UsedKB : {} / {}", t.heap_used_kb, t.heap_total_kb);
    console::println!("Threads     : {}", t.scheduler_threads);
    console::println!("IRQs        : {}", t.irq_total);
    console::println!("Drivers     : {}", t.driver_count);
    console::println!("Processes   : {}", t.process_count);
    console::println!("Mounts      : {}", t.mount_count);
    console::println!("Events      : {}", t.event_total);
    console::println!("KOM Objects : {}", s.total);
    Ok(())
}

fn cmd_objects(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.first().copied() == Some("types") {
        console::println!("Kernel");
        console::println!("Service");
        console::println!("Process");
        console::println!("Thread");
        console::println!("Driver");
        console::println!("Device");
        console::println!("Timer");
        console::println!("Event");
        console::println!("Surface");
        console::println!("Window");
        console::println!("File");
        console::println!("Directory");
        console::println!("Volume");
        console::println!("Filesystem");
        console::println!("Mount");
        console::println!("Socket");
        console::println!("Pipe");
        return Ok(());
    }

    let records = if let Some(filter) = args.first().copied() {
        let object_type = kom::ObjectType::parse(filter)
            .ok_or("objects: expected one of object types; run 'objects types'")?;
        kom::find_by_type(object_type)
    } else {
        kom::enumerate()
    };

    console::println!("{:<6}  {:<10}  {:<24}", "ID", "TYPE", "NAME");
    for obj in records {
        console::println!("{:<6}  {:<10.10}  {:<24.24}", obj.id.0, obj.object_type.as_str(), obj.name);
    }
    Ok(())
}

fn cmd_providers(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    for provider in object_manager::providers() {
        console::println!(
            "{} [{}] {:?}",
            provider.name,
            provider.namespace,
            provider.provider_type
        );
    }
    Ok(())
}

fn cmd_devices(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let filter = _args.first().copied();
    let sort_desc = _args
        .iter()
        .skip(1)
        .find_map(|a| a.strip_prefix("sort="))
        .is_some_and(|v| v.eq_ignore_ascii_case("desc"));
    let filter = filter.map(|f| {
        if f.eq_ignore_ascii_case("o") {
            "online"
        } else if f.eq_ignore_ascii_case("off") {
            "offline"
        } else if f.eq_ignore_ascii_case("f") {
            "faulted"
        } else {
            f
        }
    });

    let mut records = device::devices();
    records.sort_by(|a, b| a.name.cmp(&b.name));
    if sort_desc {
        records.reverse();
    }

    console::println!(
        "{:<16}  {:<14}  {:<14}  {:<8}  {:>8}",
        "NAME",
        "DRIVER",
        "CLASS",
        "STATUS",
        "OBJECT"
    );
    for d in records {
        if let Some(f) = filter {
            let keep = if f.eq_ignore_ascii_case("online") {
                matches!(d.status, device::DeviceStatus::Online)
            } else if f.eq_ignore_ascii_case("offline") {
                matches!(d.status, device::DeviceStatus::Offline)
            } else if f.eq_ignore_ascii_case("faulted") {
                matches!(d.status, device::DeviceStatus::Faulted)
            } else {
                d.name.contains(f) || d.driver.contains(f) || d.class.contains(f)
            };
            if !keep {
                continue;
            }
        }

        console::println!(
            "{:<16.16}  {:<14.14}  {:<14.14}  {:<8}  {:>8}",
            d.name,
            d.driver,
            d.class,
            device_status_label(d.status),
            d.object_id.0
        );
    }
    Ok(())
}

fn cmd_drivers(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let filter = _args.first().copied();
    let sort_desc = _args
        .iter()
        .skip(1)
        .find_map(|a| a.strip_prefix("sort="))
        .is_some_and(|v| v.eq_ignore_ascii_case("desc"));
    let filter = filter.map(|f| {
        if f.eq_ignore_ascii_case("r") {
            "running"
        } else if f.eq_ignore_ascii_case("l") {
            "loaded"
        } else if f.eq_ignore_ascii_case("s") {
            "stopped"
        } else if f.eq_ignore_ascii_case("f") {
            "faulted"
        } else {
            f
        }
    });

    let mut records = driver::drivers();
    records.sort_by(|a, b| a.name.cmp(&b.name));
    if sort_desc {
        records.reverse();
    }

    console::println!(
        "{:<16}  {:<8}  {:<8}  {:>7}  {:>5}  {:>5}  {:>6}  {:>5}  {:>8}",
        "NAME",
        "VERSION",
        "STATUS",
        "DEVICES",
        "START",
        "STOP",
        "RELOAD",
        "FAULT",
        "OBJECT"
    );
    for d in records {
        if let Some(f) = filter {
            let keep = if f.eq_ignore_ascii_case("running") {
                matches!(d.status, driver::DriverStatus::Running)
            } else if f.eq_ignore_ascii_case("loaded") {
                matches!(d.status, driver::DriverStatus::Loaded)
            } else if f.eq_ignore_ascii_case("stopped") {
                matches!(d.status, driver::DriverStatus::Stopped)
            } else if f.eq_ignore_ascii_case("faulted") {
                matches!(d.status, driver::DriverStatus::Faulted)
            } else {
                d.name.contains(f)
            };
            if !keep {
                continue;
            }
        }

        console::println!(
            "{:<16.16}  {:<8.8}  {:<8}  {:>7}  {:>5}  {:>5}  {:>6}  {:>5}  {:>8}",
            d.name,
            d.version,
            driver_status_label(d.status),
            d.devices.len(),
            d.start_count,
            d.stop_count,
            d.reload_count,
            d.fault_count,
            d.object_id.0
        );
    }
    Ok(())
}

fn cmd_driver(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.first().copied() == Some("start") {
        let name = args.get(1).copied().ok_or("driver start: missing name")?;
        return driver::start(name);
    }

    if args.first().copied() == Some("stop") {
        let name = args.get(1).copied().ok_or("driver stop: missing name")?;
        return driver::stop(name);
    }

    if args.first().copied() == Some("reload") {
        let name = args.get(1).copied().ok_or("driver reload: missing name")?;
        return driver::reload(name);
    }

    let name = args.first().copied().ok_or("driver: missing name")?;
    let d = driver::find(name).ok_or("driver: not found")?;

    console::println!("Driver");
    console::println!("------");
    console::println!("Name: {}", d.name);
    console::println!("Version: {}", d.version);
    console::println!("Author: {}", d.author);
    console::println!("Status: {:?}", d.status);
    console::println!("Object Id: {}", d.object_id.0);
    console::println!("Starts: {}", d.start_count);
    console::println!("Stops: {}", d.stop_count);
    console::println!("Reloads: {}", d.reload_count);
    console::println!("Faults: {}", d.fault_count);
    if let Some(err) = d.last_error {
        console::println!("Last Error: {}", err);
    } else {
        console::println!("Last Error: none");
    }
    if d.dependencies.is_empty() {
        console::println!("Dependencies: none");
    } else {
        console::print("Dependencies:");
        for dep in d.dependencies {
            console::print(&alloc::format!(" {}", dep));
        }
        console::newline();
    }
    if d.devices.is_empty() {
        console::println!("Devices: none");
    } else {
        console::print("Devices:");
        for dev in d.devices {
            console::print(&alloc::format!(" {}", dev));
        }
        console::newline();
    }
    Ok(())
}

fn cmd_service(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let action = args.first().copied().unwrap_or("list");
    let action = if action.eq_ignore_ascii_case("ls") {
        "list"
    } else if action.eq_ignore_ascii_case("st") {
        "start"
    } else if action.eq_ignore_ascii_case("sp") {
        "stop"
    } else if action.eq_ignore_ascii_case("rs") {
        "restart"
    } else if action.eq_ignore_ascii_case("i") {
        "info"
    } else if action.eq_ignore_ascii_case("h") {
        "health"
    } else {
        action
    };

    match action {
        "list" => {
            for svc in ksf::list() {
                console::println!(
                    "{} v{} id={} state={:?} health={:?}",
                    svc.name,
                    svc.version,
                    svc.id.0,
                    svc.state,
                    svc.health
                );
            }
            Ok(())
        }
        "start" => {
            let name = args.get(1).copied().ok_or("service start: missing name")?;
            ksf::start(name)
        }
        "stop" => {
            let name = args.get(1).copied().ok_or("service stop: missing name")?;
            ksf::stop(name)
        }
        "restart" => {
            let name = args
                .get(1)
                .copied()
                .ok_or("service restart: missing name")?;
            ksf::restart(name)
        }
        "health" => {
            for (name, health) in ksf::health() {
                console::println!("{} : {:?}", name, health);
            }
            Ok(())
        }
        "info" => {
            let name = args.get(1).copied().ok_or("service info: missing name")?;
            let info = ksf::info(name).ok_or("service info: not found")?;
            console::println!("Name         : {}", info.name);
            console::println!("Version      : {}", info.version);
            console::println!("Id           : {}", info.id.0);
            console::println!("State        : {:?}", info.state);
            console::println!("Health       : {:?}", info.health);
            if info.dependencies.is_empty() {
                console::println!("Dependencies : none");
            } else {
                console::print("Dependencies :");
                for dep in info.dependencies {
                    console::print(&alloc::format!(" {}", dep.0));
                }
                console::newline();
            }
            Ok(())
        }
        _ => {
            let info = ksf::info(action).ok_or("service: not found")?;
            console::println!("Name         : {}", info.name);
            console::println!("Version      : {}", info.version);
            console::println!("Id           : {}", info.id.0);
            console::println!("State        : {:?}", info.state);
            console::println!("Health       : {:?}", info.health);
            if info.dependencies.is_empty() {
                console::println!("Dependencies : none");
            } else {
                console::print("Dependencies :");
                for dep in info.dependencies {
                    console::print(&alloc::format!(" {}", dep.0));
                }
                console::newline();
            }
            Ok(())
        }
    }
}

fn cmd_restart(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let name = args
        .first()
        .copied()
        .ok_or("restart: missing service name")?;
    ksf::restart(name)
}

fn cmd_reload(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let name = args.first().copied().ok_or("reload: missing driver name")?;
    driver::reload(name)
}

fn cmd_test(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let target = args.first().copied();
    let report = testing::run_tests(target)?;

    console::println!("Running {} tests...", report.total);
    for failure in &report.failures {
        console::println!(
            "FAIL {}::{} - {}",
            failure.suite,
            failure.test,
            failure.reason
        );
    }

    if report.failed == 0 {
        console::println!("{} / {} Passed", report.passed, report.total);
    } else {
        console::println!(
            "{} / {} Passed ({} failed, {}%)",
            report.passed,
            report.total,
            report.failed,
            report.pass_rate_percent()
        );
    }

    Ok(())
}

fn cmd_verify(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let target = args.first().copied();
    let reports = testing::verify_target(target)?;

    for report in reports {
        console::println!("verify {}", report.target);
        for check in report.checks {
            console::println!("Checking {}...", check.name);
            if check.passed {
                console::println!("PASS ({})", check.detail);
            } else {
                console::println!("FAIL ({})", check.detail);
            }
        }
    }

    Ok(())
}

fn cmd_validate(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    match crate::kernel::validation::ValidateOptions::parse(args) {
        Ok(options) => {
            let report = crate::kernel::validation::run(&options);
            crate::kernel::validation::print_report(&report, &options);
            Ok(())
        }
        Err("help") => {
            crate::kernel::validation::print_help();
            Ok(())
        }
        Err(e) => {
            crate::kernel::validation::print_help();
            Err(e)
        }
    }
}

fn cmd_services(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let filter = _args.first().copied();
    let sort_desc = _args
        .iter()
        .skip(1)
        .find_map(|a| a.strip_prefix("sort="))
        .is_some_and(|v| v.eq_ignore_ascii_case("desc"));
    let filter = filter.map(|f| {
        if f.eq_ignore_ascii_case("r") {
            "running"
        } else if f.eq_ignore_ascii_case("f") {
            "failed"
        } else if f.eq_ignore_ascii_case("s") {
            "stopped"
        } else if f.eq_ignore_ascii_case("h") {
            "healthy"
        } else if f.eq_ignore_ascii_case("w") {
            "warning"
        } else if f.eq_ignore_ascii_case("c") {
            "critical"
        } else if f.eq_ignore_ascii_case("o") {
            "offline"
        } else {
            f
        }
    });

    let mut records = ksf::list();
    records.sort_by(|a, b| a.name.cmp(&b.name));
    if sort_desc {
        records.reverse();
    }

    console::println!(
        "{:<16}  {:<8}  {:>6}  {:<8}  {:<8}",
        "NAME",
        "VERSION",
        "ID",
        "STATE",
        "HEALTH"
    );
    for svc in records {
        if let Some(f) = filter {
            let keep = if f.eq_ignore_ascii_case("running") {
                matches!(svc.state, ksf::ServiceState::Running)
            } else if f.eq_ignore_ascii_case("failed") {
                matches!(svc.state, ksf::ServiceState::Failed)
            } else if f.eq_ignore_ascii_case("stopped") {
                matches!(svc.state, ksf::ServiceState::Stopped)
            } else if f.eq_ignore_ascii_case("healthy") {
                matches!(svc.health, crate::som::HealthState::Healthy)
            } else if f.eq_ignore_ascii_case("warning") {
                matches!(svc.health, crate::som::HealthState::Warning)
            } else if f.eq_ignore_ascii_case("critical") {
                matches!(svc.health, crate::som::HealthState::Critical)
            } else if f.eq_ignore_ascii_case("offline") {
                matches!(svc.health, crate::som::HealthState::Offline)
            } else {
                svc.name.contains(f)
            };
            if !keep {
                continue;
            }
        }

        console::println!(
            "{:<16.16}  {:<8.8}  {:>6}  {:<8}  {:<8}",
            svc.name,
            svc.version,
            svc.id.0,
            service_state_label(svc.state),
            health_state_label(svc.health)
        );
    }
    Ok(())
}

fn join_args_with_commas(args: &[&str]) -> String {
    let mut expr = String::new();
    for (idx, part) in args.iter().enumerate() {
        if idx > 0 {
            expr.push(',');
        }
        expr.push_str(part);
    }
    expr
}

fn cmd_query(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() {
        return Err("query: missing expression");
    }

    let expr = join_args_with_commas(args);
    for item in object_manager::query(expr.as_str())? {
        console::println!("{}", item);
    }
    Ok(())
}

fn cmd_inspect(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let target = args
        .first()
        .copied()
        .ok_or("inspect: missing object id or path")?;

    if let Ok(id) = target.parse::<u64>() {
        if let Some(lines) = kom::inspect(kom::ObjectId(id)) {
            for line in lines {
                console::println!("{}", line);
            }
            return Ok(());
        }
        return Err("inspect: object id not found");
    }

    if let Some(dev) = device::find(target) {
        console::println!("Device");
        console::println!("------");
        console::println!("Name: {}", dev.name);
        console::println!("Driver: {}", dev.driver);
        console::println!("Class: {}", dev.class);
        console::println!("Status: {:?}", dev.status);
        console::println!("Object Id: {}", dev.object_id.0);
        return Ok(());
    }

    if let Some(drv) = driver::find(target) {
        console::println!("Driver");
        console::println!("------");
        console::println!("Name: {}", drv.name);
        console::println!("Version: {}", drv.version);
        console::println!("Author: {}", drv.author);
        console::println!("Status: {:?}", drv.status);
        console::println!("Object Id: {}", drv.object_id.0);
        return Ok(());
    }

    let by_name = kom::find_by_name(target);
    if let Some(obj) = by_name.first()
        && let Some(lines) = kom::inspect(obj.id)
    {
        for line in lines {
            console::println!("{}", line);
        }
        return Ok(());
    }

    for line in object_manager::inspect(target)? {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_describe(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let path = args
        .first()
        .copied()
        .ok_or("describe: missing object path")?;
    let handle = saifs::open(path).map_err(|_| "describe: open failed")?;

    console::println!("Path : {}", handle.path());
    console::println!("Kind : {:?}", handle.kind());

    if let Some(meta) = handle.metadata() {
        console::println!("Object Id : {}", meta.id.0);
        console::println!("Class : {:?}", meta.class);
        console::println!("Provider : {}", meta.provider.0);
        console::println!("Health : {:?}", meta.health);
        console::println!("Status : {:?}", meta.status);
        console::println!("Provider Name : {}", meta.provider_name);
    }

    let props =
        crate::saifs::Handle::properties(&handle).map_err(|_| "describe: properties failed")?;
    for p in props {
        console::println!("{} : {}", p.key, p.value);
    }

    let children = crate::saifs::Handle::children(&handle).unwrap_or_default();
    if !children.is_empty() {
        console::println!("Children:");
        for c in children {
            console::println!("  {}", c);
        }
    }

    Ok(())
}

fn cmd_health(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    for line in object_manager::health_summary() {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_diagnose(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let path = args
        .first()
        .copied()
        .ok_or("diagnose: missing object path")?;
    for line in object_manager::diagnose(path)? {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_explain(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let path = args
        .first()
        .copied()
        .ok_or("explain: missing object path")?;
    if path.eq_ignore_ascii_case("heap") || path.eq_ignore_ascii_case("memory") {
        let h = heap::stats();
        let used_pct = h.used.saturating_mul(100).checked_div(h.total).unwrap_or(0) as u64;
        console::println!("Heap uses a grow-on-demand policy with guarded expansion.");
        console::println!(
            "Current Usage: {} MiB / {} MiB ({}%)",
            h.used / (1024 * 1024),
            h.total / (1024 * 1024),
            used_pct
        );
        console::println!(
            "Growth: starts at 32 MiB, expands in 2 MiB/4 MiB chunks, capped at 1 GiB."
        );
        if used_pct >= 85 {
            console::println!("Recommendation: run recover and inspect heavy services/drivers.");
        } else if used_pct >= 70 {
            console::println!("Recommendation: monitor allocations and event volume.");
        } else {
            console::println!("Recommendation: heap headroom is healthy.");
        }
        return Ok(());
    }
    for line in object_manager::explain(path)? {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_events(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let limit = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(16);
    let mut source_filter: Option<&str> = None;
    let mut sort_desc = true;
    for token in args.iter().skip(1) {
        if let Some(v) = token.strip_prefix("sort=") {
            sort_desc = v.eq_ignore_ascii_case("desc");
        } else {
            source_filter = Some(token);
        }
    }

    for line in object_manager::events(limit) {
        console::println!("{}", line);
    }

    let mut records = event::recent(limit);
    records.sort_by_key(|e| e.seq);
    if sort_desc {
        records.reverse();
    }

    for e in records {
        if let Some(source) = source_filter
            && !e.source.contains(source)
        {
            continue;
        }
        console::println!(
            "bus#{} {} {} {}",
            e.seq,
            e.kind.as_str(),
            e.source,
            e.detail
        );
    }
    Ok(())
}

fn storage_controller_kind(dev: &pci::PciDevice) -> &'static str {
    if dev.vendor_id == 0x1af4 && dev.class == 0x01 {
        return "VirtIO Block";
    }
    if dev.class != 0x01 {
        return "Other";
    }
    if dev.subclass == 0x06 && dev.prog_if == 0x01 {
        "AHCI"
    } else if dev.subclass == 0x08 && dev.prog_if == 0x02 {
        "NVMe"
    } else if dev.subclass == 0x01 {
        "IDE"
    } else {
        "Storage"
    }
}

fn pci_bar_label(dev: &pci::PciDevice, index: u8) -> String {
    pci::read_bar(dev, index)
        .map(|bar| {
            let kind = if bar.is_io { "io" } else { "mem" };
            let width = if bar.is_64bit { "64" } else { "32" };
            format!("0x{:x}({},{})", bar.base, kind, width)
        })
        .unwrap_or_else(|| "-".to_string())
}

fn pci_irq_label(dev: &pci::PciDevice) -> String {
    let irq = pci::read_u8(dev.bus, dev.device, dev.function, 0x3c);
    if irq == 0 || irq == 0xff {
        "-".to_string()
    } else {
        irq.to_string()
    }
}

fn print_storage_pci_detail() {
    pci::init();
    let devices: Vec<_> = pci::devices()
        .into_iter()
        .filter(|dev| dev.class == 0x01)
        .collect();

    console::println!("PCI Mass Storage Controllers");
    console::println!("============================");
    if devices.is_empty() {
        console::println!("  (none found)");
        return;
    }

    for dev in &devices {
        let bar0 = pci_bar_label(dev, 0);
        let bar5 = pci_bar_label(dev, 5);
        let irq = pci_irq_label(dev);
        console::println!(
            "{:02x}:{:02x}.{} Vendor={:04x} Device={:04x} Class={:02x} Subclass={:02x} ProgIF={:02x} BAR0={} BAR5={} IRQ={} Kind={}",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            dev.class,
            dev.subclass,
            dev.prog_if,
            bar0,
            bar5,
            irq,
            storage_controller_kind(dev)
        );
    }

    console::println!("");
    console::println!("AHCI requires Class=01 Subclass=06 ProgIF=01 and a non-zero memory BAR5.");
}

fn print_storage_scan_status(prefix: &str) {
    let status = disk::scan_status();
    console::println!(
        "{}: scan {} epoch={} queued={} running={} disks={} volumes={} failures={}",
        prefix,
        status.phase,
        status.epoch,
        if status.queued { "yes" } else { "no" },
        if status.running { "yes" } else { "no" },
        status.disks,
        status.volumes,
        status.failures
    );
}

fn print_cached_disks() {
    let disks = disk::disks_cached();

    console::println!("Detected Disks");
    console::println!("==============");
    console::println!(
        "  {:<12}  {:>8}  {:<6}  {:<5}  {:<10}  {}",
        "NAME", "SIZE(MB)", "SECTOR", "HW", "PARTS", "BACKING"
    );
    console::println!(
        "  {:-<12}  {:->8}  {:-<6}  {:-<5}  {:-<10}  {:-<32}",
        "", "", "", "", "", ""
    );

    for d in &disks {
        let mb = d.total_bytes / (1024 * 1024);
        console::println!(
            "  {:<12.12}  {:>8}  {:<6}  {:<5}  {:<10}  {}",
            d.name,
            mb,
            d.sector_size,
            if d.hardware { "yes" } else { "no" },
            d.partitions.len(),
            d.backing
        );
    }

    if disks.is_empty() {
        console::println!("  (no disks detected)");
        console::println!("");
        console::println!("Run 'volumes scan' to request storage discovery.");
    } else {
        console::println!("");
        console::println!("{} disk(s) detected. Use 'volumes' for mountable volumes.", disks.len());
    }
}

fn print_cached_volumes() {
    let volumes = disk::volumes_cached();
    console::println!("Detected Volumes");
    console::println!("================");
    console::println!(
        "  {:<12}  {:<8}  {:>8}  {:<10}  {:<18}  {}",
        "NAME", "FS", "SIZE(MB)", "ACCESS", "MOUNTED", "BACKING"
    );
    console::println!(
        "  {:-<12}  {:-<8}  {:->8}  {:-<10}  {:-<18}  {:-<28}",
        "", "", "", "", "", ""
    );

    for v in &volumes {
        console::println!(
            "  {:<12.12}  {:<8.8}  {:>8}  {:<10}  {:<18.18}  {}",
            v.name,
            v.filesystem.as_str(),
            v.total_bytes / (1024 * 1024),
            if v.writable { "rw" } else { "ro" },
            v.mounted_at.as_deref().unwrap_or("-"),
            v.backing
        );
    }

    if volumes.iter().all(|v| v.name == "tmpfs") {
        console::println!("");
        console::println!("Only tmpfs is known. Run 'volumes scan' to request storage discovery.");
    }
}

fn print_cached_ahci() {
    console::println!("AHCI Controllers");
    console::println!("----------------");
    let ctrls = crate::driver::ahci::controllers_cached();
    if ctrls.is_empty() {
        console::println!("  (none discovered - run 'volumes scan')");
    }
    for c in &ctrls {
        let state = match c.state {
            crate::driver::ahci::AhciControllerState::Discovered => "ready",
            crate::driver::ahci::AhciControllerState::Faulted => "faulted",
        };
        let abar = c
            .abar
            .map(|value| format!("0x{:x}", value))
            .unwrap_or_else(|| "-".to_string());
        console::println!(
            "  {} {} {:02x}:{:02x}.{} {:04x}:{:04x} abar={}",
            c.name, state, c.bus, c.device, c.function, c.vendor_id, c.device_id, abar
        );
        if let Some(err) = c.last_error.as_ref() {
            console::println!("    error: {}", err);
        }
    }

    console::println!("\nAHCI Disks");
    console::println!("----------");
    let disks = crate::driver::ahci::disks_cached();
    if disks.is_empty() {
        console::println!("  (none found)");
    }
    for d in &disks {
        let gb = d.total_sectors.saturating_mul(d.sector_size as u64) / (1024 * 1024 * 1024);
        console::println!("  {} port={} {}GB \"{}\"", d.name, d.port, gb, d.model);
    }
}

fn print_cached_mounts() {
    let vfs_mounts = vfs::mounts();
    console::println!("VFS Mounts");
    console::println!("==========");
    console::println!("  {:<24}  {:<10}  {:<5}", "PATH", "FS", "FLAGS");
    for m in &vfs_mounts {
        console::println!(
            "  {:<24.24}  {:<10.10}  {:<5}",
            m.path,
            m.fs_name,
            if m.read_only { "ro" } else { "rw" }
        );
    }
    if vfs_mounts.is_empty() {
        console::println!("  (none)");
    }
}

fn print_volume_info(name: &str) -> ShellResult {
    let volume =
        disk::find_volume_cached(name).ok_or("volumes: volume not found (run 'volumes scan')")?;
    console::println!("Volume Info");
    console::println!("===========");
    console::println!("  name      : {}", volume.name);
    console::println!("  fs        : {}", volume.filesystem.as_str());
    console::println!("  size_mb   : {}", volume.total_bytes / (1024 * 1024));
    console::println!("  sector    : {}", volume.sector_size);
    console::println!(
        "  access    : {}",
        if volume.writable { "rw" } else { "ro" }
    );
    console::println!("  backing   : {}", volume.backing);
    console::println!(
        "  mounted   : {}",
        volume.mounted_at.as_deref().unwrap_or("-")
    );
    Ok(())
}

fn cmd_volumes(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let action = args.first().copied().unwrap_or("list");

    match action {
        "help" => {
            console::println!("volumes                 List mountable volumes (cached)");
            console::println!("volumes disks           List detected block disks (cached)");
            console::println!("volumes pci             Print PCI mass-storage BAR/IRQ details");
            console::println!("volumes ahci            Show cached AHCI controllers/disks");
            console::println!("volumes mounts          Show VFS mounts and volumes");
            console::println!("volumes scan            Request background storage discovery");
            console::println!("volumes info <vol>      Show volume details");
            console::println!("volumes format <vol> <fs>  Format a volume (currently: fat32)");
            console::println!("volumes mount <vol> <path> [ro]  Mount a volume");
            console::println!("volumes umount <path>   Unmount a mounted path");
            console::println!("volumes check           Storage check summary");
            console::println!("volume                  Alias for volumes");
            console::println!("diskpart                Alias for volumes");
            console::println!("disks                   Alias for volumes disks");
            console::println!("scan                    Alias for volumes scan");
            Ok(())
        }
        "info" => {
            let name = args
                .get(1)
                .copied()
                .ok_or("volumes info: missing volume name")?;
            print_volume_info(name)
        }
        "format" => {
            let name = args
                .get(1)
                .copied()
                .ok_or("volumes format: missing volume name")?;
            let fs_str = args
                .get(2)
                .copied()
                .ok_or("volumes format: missing filesystem type")?;
            let fs = disk::FilesystemKind::from_str(fs_str)
                .ok_or("volumes format: unknown filesystem; use fat32")?;
            disk::format_volume(name, fs)?;
            console::println!("volumes: '{}' formatted as {}", name, fs.as_str());
            Ok(())
        }
        "mount" => cmd_mount(_ctx, &args[1..]),
        "umount" => cmd_umount(_ctx, &args[1..]),
        "scan" | "rescan" => {
            console::println!("volumes: scan requested");
            disk::request_rescan();
            print_storage_scan_status("volumes");
            console::println!("volumes: scan complete");
            Ok(())
        }
        "disks" | "disk" => {
            print_cached_disks();
            Ok(())
        }
        "pci" => {
            print_storage_pci_detail();
            Ok(())
        }
        "ahci" => {
            print_cached_ahci();
            Ok(())
        }
        "mounts" => {
            print_cached_mounts();
            console::println!("");
            print_cached_volumes();
            Ok(())
        }
        "check" | "hdd" | "drives" => {
            print_storage_scan_status("volumes");
            print_cached_disks();
            console::println!("");
            print_cached_volumes();
            let mountable = disk::volumes_cached()
                .into_iter()
                .find(|volume| volume.name != "tmpfs" && volume.mounted_at.is_none());
            if let Some(volume) = mountable {
                console::println!("\nmount hint: mount {} /mnt/{}", volume.name, volume.name);
            } else {
                console::println!("\nmount hint: no unmounted disk volume found");
            }
            Ok(())
        }
        "list" | "ls" | _ => {
            print_cached_volumes();
            Ok(())
        }
    }
}

pub fn run_diskpart_alias(args: &[&str], _env: &[(String, String)]) -> Result<i32, &'static str> {
    let mut ctx = CommandContext::new();
    cmd_volumes(&mut ctx, args)?;
    Ok(0)
}

fn cmd_disks(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if args.is_empty() {
        cmd_volumes(ctx, &["disks"])
    } else {
        cmd_volumes(ctx, args)
    }
}

fn cmd_storage(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let sub = args.first().copied().unwrap_or("diag");

    match sub {
        "help" => {
            console::println!("storage diag            Run bottom-up diagnostics (cached, non-blocking)");
            console::println!("storage diag probe      Run diagnostics with live AHCI/storage probe");
            console::println!("storage scan            Force AHCI + storage rescan");
            console::println!("storage pci             List PCI storage controllers");
            Ok(())
        }
        "scan" => {
            disk::request_rescan();
            print_storage_scan_status("storage");
            Ok(())
        }
        "pci" => {
            print_storage_pci_detail();
            Ok(())
        }
        "diag" => {
            let live_probe = args
                .get(1)
                .is_some_and(|v| v.eq_ignore_ascii_case("probe") || v.eq_ignore_ascii_case("live"));

            console::println!("Storage Diagnostics");
            console::println!("===================");

            console::println!("\n[1] PCI Enumeration");
            pci::init();
            let storage_pci: Vec<_> = pci::devices()
                .into_iter()
                .filter(|d| d.class == 0x01)
                .collect();
            if storage_pci.is_empty() {
                console::println!("  FAIL: no PCI storage class devices found");
            } else {
                for dev in &storage_pci {
                    console::println!(
                        "  {:02x}:{:02x}.{} {} {:04x}:{:04x}",
                        dev.bus,
                        dev.device,
                        dev.function,
                        storage_controller_kind(dev),
                        dev.vendor_id,
                        dev.device_id
                    );
                }
            }

            let has_ahci_pci = storage_pci
                .iter()
                .any(|d| d.subclass == 0x06 && d.prog_if == 0x01);
            let has_nvme_pci = storage_pci
                .iter()
                .any(|d| d.subclass == 0x08 && d.prog_if == 0x02);

            console::println!("\n[2] Driver Binding");
            let pci_drv = driver::find("pci");
            let storage_drv = driver::find("storage");
            let ext4_drv = driver::find("ext4");
            let ntfs_drv = driver::find("ntfs");
            let fat32_drv = driver::find("fat32");
            console::println!(
                "  pci={} storage={} ext4={} ntfs={} fat32={}",
                if pci_drv.is_some() { "ok" } else { "missing" },
                if storage_drv.is_some() { "ok" } else { "missing" },
                if ext4_drv.is_some() { "ok" } else { "missing" },
                if ntfs_drv.is_some() { "ok" } else { "missing" },
                if fat32_drv.is_some() { "ok" } else { "missing" }
            );

            console::println!("\n[3] Controller Initialization");
            if live_probe {
                console::println!("  mode: live probe");
                console::println!("  probing: ahci.rescan");
                crate::driver::ahci::rescan();
            } else {
                console::println!("  mode: cached (use 'storage diag probe' for live)");
            }
            let ahci_ctrl = if live_probe {
                crate::driver::ahci::controllers()
            } else {
                crate::driver::ahci::controllers_cached()
            };
            if ahci_ctrl.is_empty() {
                console::println!(
                    "  AHCI: {}",
                    if has_ahci_pci {
                        "FAIL (PCI AHCI present but no initialized controller)"
                    } else {
                        "Not Found"
                    }
                );
            } else {
                console::println!("  AHCI: PASS ({})", ahci_ctrl.len());
                for c in &ahci_ctrl {
                    let state = match c.state {
                        crate::driver::ahci::AhciControllerState::Discovered => "ready",
                        crate::driver::ahci::AhciControllerState::Faulted => "faulted",
                    };
                    let abar = c
                        .abar
                        .map(|v| format!("0x{:x}", v))
                        .unwrap_or_else(|| "-".to_string());
                    console::println!(
                        "    {} {} abar={} {:02x}:{:02x}.{}",
                        c.name,
                        state,
                        abar,
                        c.bus,
                        c.device,
                        c.function
                    );
                    if let Some(err) = c.last_error.as_ref() {
                        console::println!("      note: {}", err);
                    }
                }
            }
            console::println!("  NVMe: {}", if has_nvme_pci { "PCI Found (driver pending)" } else { "Not Found" });

            console::println!("\n[4] Port Enumeration + IDENTIFY");
            let ahci_disks = if live_probe {
                crate::driver::ahci::disks()
            } else {
                crate::driver::ahci::disks_cached()
            };
            if ahci_disks.is_empty() {
                console::println!("  no AHCI media identified");
            } else {
                for d in &ahci_disks {
                    let gb = d.total_sectors.saturating_mul(d.sector_size as u64) / (1024 * 1024 * 1024);
                    console::println!(
                        "  {} ctrl={} port={} model={} size={}GB",
                        d.name, d.controller, d.port, d.model, gb
                    );
                }
            }

            console::println!("\n[5] Block Device Registration");
            if live_probe {
                console::println!("  probing: storage.rescan");
                disk::rescan();
            } else {
                console::println!("  using cached storage registry");
            }
            let block_devices: Vec<_> = device::devices()
                .into_iter()
                .filter(|d| d.class.contains("block/") || d.driver.eq_ignore_ascii_case("storage") || d.driver.eq_ignore_ascii_case("ahci"))
                .collect();
            if block_devices.is_empty() {
                console::println!("  FAIL: no block devices registered");
            } else {
                for d in &block_devices {
                    console::println!(
                        "  {} driver={} class={} status={}",
                        d.name,
                        d.driver,
                        d.class,
                        device_status_label(d.status)
                    );
                }
            }

            console::println!("\n[6] Volume Manager");
            let volumes = if live_probe {
                disk::volumes()
            } else {
                disk::volumes_cached()
            };
            let real_volumes: Vec<_> = volumes
                .into_iter()
                .filter(|v| v.name != "tmpfs")
                .collect();
            if real_volumes.is_empty() {
                if !storage_pci.is_empty() {
                    console::println!("  INFO: no non-tmpfs volumes (controller present; driver support may be pending)");
                } else {
                    console::println!("  FAIL: no non-tmpfs volumes");
                }
            } else {
                for v in &real_volumes {
                    console::println!(
                        "  {} fs={} backing={} size_mb={}",
                        v.name,
                        v.filesystem.as_str(),
                        v.backing,
                        v.total_bytes / (1024 * 1024)
                    );
                }
            }

            console::println!("\n[7] Partition Scan");
            let parts: Vec<_> = real_volumes
                .iter()
                .filter(|v| v.backing.contains(':') || v.name.contains('p'))
                .collect();
            if parts.is_empty() {
                console::println!("  no partitions detected");
            } else {
                for p in parts {
                    console::println!(
                        "  {} fs={} backing={}",
                        p.name,
                        p.filesystem.as_str(),
                        p.backing
                    );
                }
            }

            console::println!("\n[8] Mounted Filesystems");
            let mounts = vfs::mounts();
            if mounts.is_empty() {
                console::println!("  none");
            } else {
                for m in mounts {
                    console::println!(
                        "  {} -> {} [{}]",
                        m.path,
                        m.fs_name,
                        if m.read_only { "ro" } else { "rw" }
                    );
                }
            }

            console::println!("\n[9] Scan Diagnostics");
            let diagnostics = if live_probe {
                disk::scan_diagnostics()
            } else {
                disk::scan_diagnostics_cached()
            };
            if diagnostics.is_empty() {
                console::println!("  none");
            } else {
                for diag in diagnostics {
                    console::println!("  {}", diag);
                }
            }

            Ok(())
        }
        _ => Err("storage: usage: storage [diag [probe]|scan|pci|help]"),
    }
}

fn cmd_scan(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    cmd_volumes(ctx, &["scan"])
}

fn cmd_hdd(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let cached = args
        .first()
        .is_some_and(|arg| arg.eq_ignore_ascii_case("cached") || arg.eq_ignore_ascii_case("list"));

    if cached {
        cmd_volumes(ctx, &["check"])
    } else {
        cmd_volumes(ctx, &["scan"])?;
        cmd_volumes(ctx, &["check"])
    }
}

fn cmd_mount(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let sub = args.first().copied().unwrap_or("list");

    match sub {
        // ── mount list (default) ────────────────────────────────────────────
        "list" | "ls" => {
            cmd_volumes(_ctx, &["mounts"])
        }

        "disks" => cmd_volumes(_ctx, &["disks"]),

        "volumes" | "volume" => cmd_volumes(_ctx, &args[1..]),

        // ── mount scan ──────────────────────────────────────────────────────
        "scan" | "rescan" => cmd_scan(_ctx, &[]),

        // ── mount help ──────────────────────────────────────────────────────
        "help" => {
            console::println!("mount                         List mounts and storage volumes");
            console::println!("mount list                    Same as above");
            console::println!("mount disks                   Alias for volumes disks");
            console::println!("mount volumes                 List detected volumes (cached)");
            console::println!("mount scan                    Alias for volumes scan");
            console::println!("mount <device> <path> [ro]    Mount a storage volume");
            console::println!("umount <path>                 Unmount a mounted path");
            console::println!("");
            console::println!("Volumes are shown by 'volumes'. Common names: sata0p1, disk0p1, ...");
            console::println!("Mount points must exist as directories (e.g. /mnt/disk0).");
            Ok(())
        }

        // ── mount <device> <mountpoint> [ro] ────────────────────────────────
        device => {
            let mountpoint = args
                .get(1)
                .copied()
                .ok_or("mount: usage: mount <device> <path> [ro]")?;

            // Reject paths that could escape or corrupt critical dirs
            if mountpoint == "/" || mountpoint == "/boot" || mountpoint == "/bin" {
                return Err("mount: cannot mount over a system directory");
            }

            let read_only = args.get(2).is_some_and(|f| f.eq_ignore_ascii_case("ro"));

            let vol = disk::resolve_mountable_volume(device)
                .ok_or("mount: no mountable volume found (run 'mount scan', then use a partition like sata0p1)")?;

            let fs_name = vol.filesystem.as_str();

            // Auto-create the mount-point directory if it doesn't exist
            let _ = vfs::mkdir(mountpoint);

            // Register in VFS
            vfs::mount(mountpoint, fs_name, read_only)?;

            // Update storage driver's mounted_at marker
            if let Err(e) = disk::mount_volume(vol.name.as_str(), mountpoint, read_only) {
                let _ = vfs::umount(mountpoint);
                return Err(e);
            }

            console::println!(
                "mount: {} ({}) mounted at {} [{}]",
                vol.name,
                fs_name,
                mountpoint,
                if read_only { "ro" } else { "rw" }
            );
            Ok(())
        }
    }
}

fn cmd_umount(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let mountpoint = args
        .first()
        .copied()
        .ok_or("umount: usage: umount <path>")?;

    // Remove from VFS (validates path and protects root)
    vfs::umount(mountpoint)?;

    // Clear the storage driver's mounted_at marker (best-effort)
    let _ = disk::umount_volume(mountpoint);

    console::println!("umount: {} unmounted", mountpoint);
    Ok(())
}

fn cmd_df(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let target = args.first().copied().unwrap_or("/");
    let path = if target.starts_with('/') {
        target.to_string()
    } else {
        let cwd = saifs::pwd();
        if cwd == "/" {
            format!("/{}", target)
        } else {
            format!("{}/{}", cwd, target)
        }
    };

    let volume = disk::mounted_volume_for_path(path.as_str())
        .ok_or("df: path is not on a mounted volume")?;
    let total = disk::total_bytes(path.as_str()).unwrap_or(0);
    let used = disk::used_bytes(path.as_str()).unwrap_or(0);
    let free = total.saturating_sub(used);

    console::println!(
        "{:<12}  {:<20}  {:>9}  {:>8}  {:>8}",
        "FILESYSTEM",
        "MOUNTED ON",
        "TOTAL(MB)",
        "USED(MB)",
        "FREE(MB)"
    );
    console::println!(
        "{:<12.12}  {:<20.20}  {:>9}  {:>8}  {:>8}",
        volume.name,
        volume.mounted_at.unwrap_or_else(|| "-".to_string()),
        total / (1024 * 1024),
        used / (1024 * 1024),
        free / (1024 * 1024)
    );
    Ok(())
}

fn cmd_tree(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let root = if let Some(path) = args.first().copied() {
        if path.starts_with('/') {
            path.to_string()
        } else {
            let cwd = saifs::pwd();
            if cwd == "/" {
                alloc::format!("/{}", path)
            } else {
                alloc::format!("{}/{}", cwd, path)
            }
        }
    } else {
        saifs::pwd()
    };

    fn walk(path: &str, depth: usize) {
        let indent = "  ".repeat(depth);
        crate::console::println!("{}{}", indent, path);
        let Ok(entries) = crate::saifs::list(path) else {
            return;
        };
        for child in entries {
            let child_path = if path == "/" {
                alloc::format!("/{}", child)
            } else {
                alloc::format!("{}/{}", path, child)
            };
            walk(child_path.as_str(), depth + 1);
        }
    }

    walk(root.as_str(), 0);
    Ok(())
}

fn cmd_threads(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("{:<8}  {:<10}", "ID", "STATE");
    for t in scheduler::threads() {
        console::println!("{:<8}  {:<10}", t.id, thread_state_label(t.state));
    }
    Ok(())
}

fn cmd_uptime(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let d = timer::uptime();
    let total_ms = d.as_millis() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    console::println!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis);
    Ok(())
}

fn cmd_ticks(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("{}", timer::ticks());
    Ok(())
}

fn cmd_irq(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let t = telemetry::snapshot();
    console::println!("irq.total={}", t.irq_total);
    Ok(())
}

fn cmd_heap(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let stats = heap::stats();
    console::println!("Heap Size : {} MB", stats.total / (1024 * 1024));
    console::println!("Used      : {} KB", stats.used / 1024);
    console::println!("Free      : {} KB", stats.free / 1024);
    Ok(())
}

fn cmd_pci(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("Bus Dev Fn Vendor Device Class");
    for dev in pci::devices() {
        console::println!(
            "{:02x} {:02x} {:02x} {:04x} {:04x} {}",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            pci::class_name(dev.class)
        );
    }
    Ok(())
}

fn cmd_detect(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let action = args.first().copied().unwrap_or("scan");

    if action.eq_ignore_ascii_case("help") {
        console::println!("detect             Real probe + report newly available hardware");
        console::println!("detect quick       Compare cached snapshot only (fast, no heavy probe)");
        console::println!("detect probe       Alias for real probe mode");
        console::println!("detect real        Alias for real probe mode");
        console::println!("detect reset       Reset baseline to current hardware snapshot");
        console::println!("detect scan        Same as default");
        console::println!("Includes new PCI/USB/AHCI devices and new FAT32/NTFS/EXT4 volumes.");
        return Ok(());
    }

    let force_probe = action.eq_ignore_ascii_case("scan")
        || action.eq_ignore_ascii_case("probe")
        || action.eq_ignore_ascii_case("real")
        || action.eq_ignore_ascii_case("rescan");

    if force_probe {
        console::println!("detect: pci probe...");
        pci::init();
        console::println!("detect: usb probe...");
        usb::rescan();
        console::println!("detect: storage+ahci probe...");
        disk::rescan();
    } else if !action.eq_ignore_ascii_case("quick") && !action.eq_ignore_ascii_case("reset") {
        return Err("detect: usage: detect [scan|real|probe|quick|reset|help]");
    }

    let current = snapshot_collect(force_probe);

    if action.eq_ignore_ascii_case("reset") {
        with_detect_baseline_mut(|baseline| {
            *baseline = Some(current);
        });
        console::println!("detect: baseline reset to current hardware state");
        return Ok(());
    }

    let mut had_changes = false;
    with_detect_baseline_mut(|baseline| {
        if baseline.is_none() {
            *baseline = Some(current.clone());
            console::println!("detect: baseline initialized (run detect again to see new hardware)");
            return;
        }

        let prev = baseline.as_ref().expect("detect baseline missing");

        let new_pci = diff_new(current.pci.as_slice(), prev.pci.as_slice());
        let new_devices = diff_new(current.devices.as_slice(), prev.devices.as_slice());
        let new_usb = diff_new(current.usb.as_slice(), prev.usb.as_slice());
        let new_ahci_ctrl = diff_new(
            current.ahci_controllers.as_slice(),
            prev.ahci_controllers.as_slice(),
        );
        let new_ahci_disks = diff_new(current.ahci_disks.as_slice(), prev.ahci_disks.as_slice());
        let new_storage_fs = diff_new(current.storage_fs.as_slice(), prev.storage_fs.as_slice());

        print_detect_section("new.pci", new_pci.as_slice());
        print_detect_section("new.device.registry", new_devices.as_slice());
        print_detect_section("new.usb.controllers", new_usb.as_slice());
        print_detect_section("new.ahci.controllers", new_ahci_ctrl.as_slice());
        print_detect_section("new.ahci.media", new_ahci_disks.as_slice());
        print_detect_section("new.storage.fs(ext4|ntfs|fat32)", new_storage_fs.as_slice());

        had_changes = !new_pci.is_empty()
            || !new_devices.is_empty()
            || !new_usb.is_empty()
            || !new_ahci_ctrl.is_empty()
            || !new_ahci_disks.is_empty()
            || !new_storage_fs.is_empty();

        *baseline = Some(current);
    });

    if !had_changes {
        console::println!("detect: no new hardware/media since last snapshot");
    }

    Ok(())
}

fn cmd_usb(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let action = args.first().copied().unwrap_or("status");

    match action {
        "scan" | "rescan" | "up" => {
            driver::start("usb")?;
            console::println!("usb: {} controller(s) detected", usb::controller_count());
            cmd_usb(_ctx, &["status"])
        }
        "status" | "ls" => {
            let controllers = usb::controllers();
            if controllers.is_empty() {
                console::println!("usb: no host controllers detected");
                console::println!("usb: no USB HID keyboard/mouse path is available yet");
                return Ok(());
            }

            console::println!("Name  Type  State  Ver  Ports  Vendor Device  Resource");
            for controller in controllers {
                let resource = if let Some(mmio) = controller.mmio_base {
                    format!("mmio@0x{:x}", mmio)
                } else if let Some(io) = controller.io_base {
                    format!("io@0x{:x}", io)
                } else {
                    "unmapped".to_string()
                };
                let state = match controller.state {
                    usb::UsbControllerState::Discovered => "disc",
                    usb::UsbControllerState::Initialized => "init",
                    usb::UsbControllerState::Faulted => "fail",
                };
                let version = controller
                    .version
                    .map(|v| format!("{:x}.{:02x}", v >> 8, v & 0xFF))
                    .unwrap_or_else(|| "--".to_string());
                console::println!(
                    "{}  {}  {}  {}  {}/{}  {:04x} {:04x}  {}",
                    controller.name,
                    controller.kind,
                    state,
                    version,
                    controller.connected_ports,
                    controller.port_count,
                    controller.vendor_id,
                    controller.device_id,
                    resource,
                );
                if let Some(err) = controller.last_error.as_ref() {
                    console::println!("  note: {}", err);
                }
            }
            console::println!(
                "usb: xHCI init/probe is present; HID keyboard/mouse still needs device enumeration, transfer rings, and report parsing"
            );
            Ok(())
        }
        _ => Err("usb: usage: usb [status|scan|rescan|up]"),
    }
}

fn cmd_net(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let action = args.first().copied().unwrap_or("status");

    match action {
        "up" => {
            driver::start("network")?;
            driver::start("loopback")?;
            driver::start("ethernet")?;
            driver::start("wifi")?;
            driver::start("dhcp")?;
            driver::start("dns")?;
            let _ = network::bind_nic();
            let _ = network::apply_dhcp();
            console::println!("network: up");
            cmd_net(_ctx, &["status"])
        }
        "status" | "st" => {
            let st = network::status();
            console::println!("boot->pci.nic.detected={}", st.pci_nic_detected);
            console::println!("pci->driver.bind={}", st.driver_bound);
            console::println!("driver->rx.tx={}", st.rx_tx_ready);
            console::println!("rx.tx->arp={}", st.arp_ready);
            console::println!("arp->ipv4={}", st.ipv4_ready);
            console::println!("ipv4->udp={}", st.udp_ready);
            console::println!("udp->dhcp={}", st.dhcp_ready);
            console::println!("dhcp->tcp={}", st.tcp_ready);
            console::println!("tcp->http={}", st.http_ready);
            console::println!("counters tx={} rx={}", st.tx_packets, st.rx_packets);
            if let Some(nic) = st.nic {
                console::println!(
                    "nic iface={} kind={} backing={} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    nic.interface,
                    nic.kind,
                    nic.backing,
                    nic.mac[0],
                    nic.mac[1],
                    nic.mac[2],
                    nic.mac[3],
                    nic.mac[4],
                    nic.mac[5]
                );
            } else {
                console::println!("nic: none");
            }
            if let Some(ipv4) = st.ipv4 {
                console::println!(
                    "ipv4 addr={} mask={} gw={} dns={}",
                    ipv4.address,
                    ipv4.subnet_mask,
                    ipv4.gateway,
                    ipv4.dns_server
                );
            } else {
                console::println!("ipv4: none");
            }
            Ok(())
        }
        "reset" => {
            driver::reload("network")?;
            driver::reload("ethernet")?;
            driver::reload("wifi")?;
            driver::reload("dhcp")?;
            let _ = network::bind_nic();
            let _ = network::apply_dhcp();
            console::println!("network: reset complete");
            Ok(())
        }
        _ => Err("net: expected up|status|reset"),
    }
}

fn cmd_dhcp(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    driver::start("dhcp")?;
    let cfg = network::apply_dhcp()?;
    console::println!(
        "dhcp: ipv4={} mask={} gw={} dns={}",
        cfg.address,
        cfg.subnet_mask,
        cfg.gateway,
        cfg.dns_server
    );

    for lease in dhcp::leases() {
        console::println!(
            "lease iface={} ip={} lease={}s",
            lease.interface,
            lease.address,
            lease.lease_seconds
        );
    }
    Ok(())
}

fn cmd_ping(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let ip = args.first().copied().ok_or("ping: missing IPv4 target")?;
    let count = args
        .get(1)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(4)
        .clamp(1, 16);

    if network::status().ipv4_ready {
        // Keep existing state if already configured.
    } else {
        let _ = network::bind_nic();
        let _ = network::apply_dhcp();
    }

    let mut success = 0usize;
    for seq in 0..count {
        match network::ping_ipv4(ip) {
            Ok(rtt) => {
                success += 1;
                console::println!(
                    "{} bytes from {}: icmp_seq={} ttl=64 time={}ms",
                    64,
                    ip,
                    seq,
                    rtt
                );
            }
            Err(e) => {
                console::println!("ping: seq={} error={}", seq, e);
            }
        }
    }

    console::println!(
        "ping stats: tx={} rx={} loss={}%%",
        count,
        success,
        ((count.saturating_sub(success)) * 100) / count
    );
    Ok(())
}

fn default_download_path(url: &str) -> String {
    if let Some((_, tail)) = url.rsplit_once('/')
        && !tail.is_empty()
    {
        return if tail.starts_with('/') {
            tail.to_string()
        } else {
            format!("/tmp/{}", tail)
        };
    }
    "/tmp/download.bin".to_string()
}

fn cmd_wget(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let url = args.first().copied().ok_or("wget: missing URL")?;
    let out = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| default_download_path(url));

    if network::status().ipv4_ready {
        // Keep existing lease.
    } else {
        let _ = network::bind_nic();
        let _ = network::apply_dhcp();
    }

    let result = network::http_download(url, out.as_str())?;
    console::println!(
        "wget: status={} bytes={} saved={} url={}",
        result.status_code,
        result.size,
        result.path,
        url
    );
    Ok(())
}

fn cmd_shutdown(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("Shutdown requested");
    halt_forever()
}

fn cmd_reboot(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    hal::arch::x86_64::io::outb(0x64, 0xFE);
    halt_forever()
}

fn cmd_sairu(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let action = args.first().copied().unwrap_or("health");
    match action {
        "health" => {
            for line in sairu::health() {
                console::println!("{}", line);
            }
            for line in sairu::service_health() {
                console::println!("{}", line);
            }
            Ok(())
        }
        "diagnose" => {
            for line in sairu::diagnose() {
                console::println!("{}", line);
            }
            Ok(())
        }
        "explain" => {
            let target = args
                .get(1)
                .copied()
                .ok_or("sairu explain: missing target")?;
            for line in sairu::explain(target) {
                console::println!("{}", line);
            }
            Ok(())
        }
        "recover" => {
            for line in sairu::recover() {
                console::println!("{}", line);
            }
            Ok(())
        }
        _ => Err("sairu: expected health|diagnose|explain|recover"),
    }
}

fn halt_forever() -> ! {
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}
