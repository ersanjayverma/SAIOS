use alloc::string::String;

use crate::console;
use crate::heap;
use crate::object_manager;
use crate::pci;
use crate::pmm;
use crate::scheduler;
use crate::shell::commands;
use crate::shell::parser::ParsedCommand;
use crate::saifs;
use crate::timer;

fn print_help() {
    console::println!("help");
    console::println!("clear");
    console::println!("version");
    console::println!("echo");
    console::println!("mem");
    console::println!("heap");
    console::println!("pci");
    console::println!("ticks");
    console::println!("uptime");
    console::println!("threads");
    console::println!("objects");
    console::println!("inspect");
    console::println!("explain");
    console::println!("diagnose");
    console::println!("health");
    console::println!("events");
    console::println!("providers");
    console::println!("query");
    console::println!("ls");
    console::println!("pwd");
    console::println!("cd");
    console::println!("mkdir");
    console::println!("touch");
    console::println!("cat");
    console::println!("rm");
    console::println!("panic");
    console::println!("reboot");
    console::println!("shutdown");
}

fn print_uptime() {
    let d = timer::uptime();
    let total_ms = d.as_millis() as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;

    console::println!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis);
}

fn state_name(state: scheduler::ThreadState) -> &'static str {
    match state {
        scheduler::ThreadState::Ready => "Ready",
        scheduler::ThreadState::Running => "Running",
        scheduler::ThreadState::Sleeping => "Sleeping",
        scheduler::ThreadState::Blocked => "Blocked",
        scheduler::ThreadState::Dead => "Dead",
    }
}

fn halt_forever() -> ! {
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}

pub fn execute(parsed: ParsedCommand<'_>) {
    match parsed.command {
        commands::HELP => {
            print_help();
        }
        commands::CLEAR => {
            console::clear();
        }
        commands::VERSION => {
            console::println!("SAIOS v0.1");
        }
        commands::ECHO => {
            let mut first = true;
            for arg in parsed.args {
                if !first {
                    console::print(" ");
                }
                console::print(arg);
                first = false;
            }
            console::newline();
        }
        commands::MEM => {
            if parsed.args.first().copied() == Some("test") {
                pmm::run_reuse_test(1000);
            } else {
                console::println!("Total RAM : {} MB", pmm::total_ram_mb());
                console::println!("Pages     : {}", pmm::total_pages());
                console::println!("Used      : {}", pmm::used_pages());
                console::println!("Free      : {}", pmm::free_pages());
            }
        }
        commands::HEAP => {
            let stats = heap::stats();
            console::println!("Heap Size : {} MB", stats.total / (1024 * 1024));
            console::println!("Used      : {} KB", stats.used / 1024);
            console::println!("Free      : {} KB", stats.free / 1024);
        }
        commands::PCI => {
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
        }
        commands::TICKS => {
            console::println!("{}", timer::ticks());
        }
        commands::UPTIME => {
            print_uptime();
        }
        commands::THREADS => {
            console::println!("ID   State");
            for t in scheduler::threads() {
                console::println!("{}    {}", t.id, state_name(t.state));
            }
        }
        commands::OBJECTS => {
            for ty in object_manager::object_types() {
                console::println!("{}", ty);
            }
        }
        commands::INSPECT => {
            match parsed.args.first().copied() {
                Some(path) => match object_manager::inspect(path) {
                    Ok(lines) => {
                        for line in lines {
                            console::println!("{}", line);
                        }
                    }
                    Err(e) => console::println!("inspect: {}", e),
                },
                None => console::println!("inspect: missing object path"),
            }
        }
        commands::EXPLAIN => {
            match parsed.args.first().copied() {
                Some(path) => match object_manager::explain(path) {
                    Ok(lines) => {
                        for line in lines {
                            console::println!("{}", line);
                        }
                    }
                    Err(e) => console::println!("explain: {}", e),
                },
                None => console::println!("explain: missing object path"),
            }
        }
        commands::DIAGNOSE => {
            match parsed.args.first().copied() {
                Some(path) => match object_manager::diagnose(path) {
                    Ok(lines) => {
                        for line in lines {
                            console::println!("{}", line);
                        }
                    }
                    Err(e) => console::println!("diagnose: {}", e),
                },
                None => console::println!("diagnose: missing object path"),
            }
        }
        commands::HEALTH => {
            for line in object_manager::health_summary() {
                console::println!("{}", line);
            }
        }
        commands::EVENTS => {
            let limit = parsed
                .args
                .first()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(16);

            for line in object_manager::events(limit) {
                console::println!("{}", line);
            }
        }
        commands::PROVIDERS => {
            for provider in object_manager::providers() {
                console::println!(
                    "{} [{}] {}",
                    provider.name,
                    provider.namespace,
                    provider_type_name(provider.provider_type)
                );
            }
        }
        commands::QUERY => {
            if parsed.args.is_empty() {
                console::println!("query: missing expression");
                return;
            }

            let mut expr = String::new();
            for (i, part) in parsed.args.iter().enumerate() {
                if i > 0 {
                    expr.push(',');
                }
                expr.push_str(part);
            }

            match object_manager::query(expr.as_str()) {
                    Ok(paths) => {
                        for path in paths {
                            console::println!("{}", path);
                        }
                    }
                    Err(e) => console::println!("query: {}", e),
            }
        }
        commands::LS => {
            let path = parsed.args.first().copied().unwrap_or(".");
            match saifs::list(path) {
                Ok(entries) => {
                    for name in entries {
                        console::println!("{}", name);
                    }
                }
                Err(e) => console::println!("ls: {:?}", e),
            }
        }
        commands::PWD => {
            console::println!("{}", saifs::pwd());
        }
        commands::CD => {
            match parsed.args.first().copied() {
                Some(path) => {
                    if let Err(e) = saifs::cd(path) {
                        console::println!("cd: {:?}", e);
                    }
                }
                None => console::println!("cd: missing path"),
            }
        }
        commands::MKDIR => {
            match parsed.args.first().copied() {
                Some(path) => {
                    if let Err(e) = saifs::mkdir(path) {
                        console::println!("mkdir: {:?}", e);
                    }
                }
                None => console::println!("mkdir: missing path"),
            }
        }
        commands::TOUCH => {
            match parsed.args.first().copied() {
                Some(path) => {
                    if let Err(e) = saifs::touch(path) {
                        console::println!("touch: {:?}", e);
                    }
                }
                None => console::println!("touch: missing path"),
            }
        }
        commands::CAT => {
            match parsed.args.first().copied() {
                Some(path) => match saifs::read_text(path) {
                    Ok(contents) => {
                        if !contents.is_empty() {
                            console::println!("{}", contents);
                        }
                    }
                    Err(e) => console::println!("cat: {:?}", e),
                },
                None => console::println!("cat: missing path"),
            }
        }
        commands::RM => {
            match parsed.args.first().copied() {
                Some(path) => {
                    if let Err(e) = saifs::remove(path) {
                        console::println!("rm: {:?}", e);
                    }
                }
                None => console::println!("rm: missing path"),
            }
        }
        commands::PANIC => {
            panic!("panic command invoked");
        }
        commands::REBOOT => {
            // 8042 keyboard controller pulse reset line.
            hal::arch::x86_64::io::outb(0x64, 0xFE);
            halt_forever();
        }
        commands::SHUTDOWN => {
            console::println!("Shutdown requested");
            halt_forever();
        }
        _ => {
            console::println!("Unknown command");
        }
    }
}

fn provider_type_name(kind: crate::provider::ProviderType) -> &'static str {
    match kind {
        crate::provider::ProviderType::Core => "Core",
        crate::provider::ProviderType::Memory => "Memory",
        crate::provider::ProviderType::Storage => "Storage",
        crate::provider::ProviderType::Filesystem => "Filesystem",
        crate::provider::ProviderType::Device => "Device",
        crate::provider::ProviderType::Driver => "Driver",
        crate::provider::ProviderType::Process => "Process",
        crate::provider::ProviderType::Thread => "Thread",
        crate::provider::ProviderType::Scheduler => "Scheduler",
        crate::provider::ProviderType::Network => "Network",
        crate::provider::ProviderType::Security => "Security",
        crate::provider::ProviderType::User => "User",
        crate::provider::ProviderType::Service => "Service",
        crate::provider::ProviderType::Event => "Event",
        crate::provider::ProviderType::Log => "Log",
        crate::provider::ProviderType::AI => "AI",
    }
}
