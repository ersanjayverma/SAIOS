use alloc::boxed::Box;
use alloc::string::String;

use crate::console;
use crate::heap;
use crate::kernel::testing;
use crate::ksf;
use crate::object_manager;
use crate::pci;
use crate::saifs;
use crate::scheduler;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::ShellContext;
use crate::timer;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "help",
        description: "List registered commands",
        handler: cmd_help,
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
        name: "objects",
        description: "List object kinds",
        handler: cmd_objects,
    }));
    registry.register(Box::new(StaticCommand {
        name: "providers",
        description: "List registered providers",
        handler: cmd_providers,
    }));
    registry.register(Box::new(StaticCommand {
        name: "service",
        description: "Manage kernel services",
        handler: cmd_service,
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
        name: "services",
        description: "List service objects",
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
        name: "logs",
        description: "Alias for events",
        handler: cmd_events,
    }));
    registry.register(Box::new(StaticCommand {
        name: "mount",
        description: "List current mounts",
        handler: cmd_mounts,
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
        name: "shutdown",
        description: "Shutdown kernel (halt)",
        handler: cmd_shutdown,
    }));
    registry.register(Box::new(StaticCommand {
        name: "reboot",
        description: "Reboot machine",
        handler: cmd_reboot,
    }));
}

fn cmd_help(ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    console::println!(
        "namespace={} env_vars={}",
        ctx.session.current_namespace,
        ctx.environment.vars.len()
    );
    for item in &ctx.command_catalog {
        console::println!("{} - {}", item.name, item.description);
    }
    Ok(())
}

fn cmd_version(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    console::println!("SAIOS v0.1 SNSH");
    Ok(())
}

fn cmd_clear(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    console::clear();
    Ok(())
}

fn cmd_exit(ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    ctx.session.running = false;
    Ok(())
}

fn cmd_objects(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    for ty in object_manager::object_types() {
        console::println!("{}", ty);
    }
    Ok(())
}

fn cmd_providers(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
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

fn cmd_service(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let action = args.first().copied().unwrap_or("list");

    match action {
        "list" => {
            for svc in ksf::list() {
                console::println!(
                    "{} id={} state={:?} health={:?}",
                    svc.name,
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
            let name = args.get(1).copied().ok_or("service restart: missing name")?;
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
        _ => Err("service: expected list|start|stop|restart|health|info"),
    }
}

fn cmd_test(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let target = args.first().copied();
    let report = testing::run_tests(target)?;

    console::println!("Running {} tests...", report.total);
    for failure in &report.failures {
        console::println!("FAIL {}::{} - {}", failure.suite, failure.test, failure.reason);
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

fn cmd_verify(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
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

fn cmd_services(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    let results = object_manager::query("kind=Service")?;
    for item in results {
        console::println!("{}", item);
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

fn cmd_query(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    if args.is_empty() {
        return Err("query: missing expression");
    }

    let expr = join_args_with_commas(args);
    for item in object_manager::query(expr.as_str())? {
        console::println!("{}", item);
    }
    Ok(())
}

fn cmd_inspect(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("inspect: missing object path")?;
    for line in object_manager::inspect(path)? {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_describe(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("describe: missing object path")?;
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

    let props = crate::saifs::Handle::properties(&handle).map_err(|_| "describe: properties failed")?;
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

fn cmd_health(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    for line in object_manager::health_summary() {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_diagnose(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("diagnose: missing object path")?;
    for line in object_manager::diagnose(path)? {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_explain(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("explain: missing object path")?;
    for line in object_manager::explain(path)? {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_events(_ctx: &mut ShellContext, args: &[&str]) -> ShellResult {
    let limit = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(16);
    for line in object_manager::events(limit) {
        console::println!("{}", line);
    }
    Ok(())
}

fn cmd_mounts(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    for mount in saifs::mounts() {
        console::println!(
            "{} -> provider={} readonly={}",
            mount.path,
            mount.provider.0,
            mount.read_only
        );
    }
    Ok(())
}

fn cmd_threads(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    console::println!("ID   State");
    for t in scheduler::threads() {
        console::println!("{}    {:?}", t.id, t.state);
    }
    Ok(())
}

fn cmd_uptime(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    let d = timer::uptime();
    let total_ms = d.as_millis() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    console::println!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis);
    Ok(())
}

fn cmd_ticks(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    console::println!("{}", timer::ticks());
    Ok(())
}

fn cmd_heap(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    let stats = heap::stats();
    console::println!("Heap Size : {} MB", stats.total / (1024 * 1024));
    console::println!("Used      : {} KB", stats.used / 1024);
    console::println!("Free      : {} KB", stats.free / 1024);
    Ok(())
}

fn cmd_pci(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
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

fn cmd_shutdown(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    console::println!("Shutdown requested");
    halt_forever()
}

fn cmd_reboot(_ctx: &mut ShellContext, _args: &[&str]) -> ShellResult {
    hal::arch::x86_64::io::outb(0x64, 0xFE);
    halt_forever()
}

fn halt_forever() -> ! {
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}
