use crate::console;
use crate::shell::commands;
use crate::shell::parser::ParsedCommand;

pub enum DispatchOutcome {
    Continue,
}

pub fn execute(parsed: ParsedCommand<'_>) -> DispatchOutcome {
    match parsed.command {
        commands::HELP => {
            console::println!("help");
            console::println!("clear");
            console::println!("version");
            console::println!("echo");
            console::println!("panic");
            console::println!("reboot");
            console::println!("shutdown");
            DispatchOutcome::Continue
        }
        commands::CLEAR => {
            console::clear();
            DispatchOutcome::Continue
        }
        commands::VERSION => {
            console::println!("SAIOS v0.1");
            DispatchOutcome::Continue
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
            DispatchOutcome::Continue
        }
        commands::PANIC => {
            panic!("panic command invoked");
        }
        commands::REBOOT => {
            // 8042 keyboard controller pulse reset line.
            hal::arch::x86_64::io::outb(0x64, 0xFE);
            loop {
                hal::arch::x86_64::cpu::hlt();
            }
        }
        commands::SHUTDOWN => {
            console::println!("Shutdown requested");
            loop {
                hal::arch::x86_64::cpu::hlt();
            }
        }
        _ => {
            console::println!("Unknown command");
            DispatchOutcome::Continue
        }
    }
}
