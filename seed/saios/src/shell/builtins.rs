use crate::console;
use crate::heap;
use crate::pci;
use crate::pmm;
use crate::shell::commands;
use crate::shell::parser::ParsedCommand;
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
