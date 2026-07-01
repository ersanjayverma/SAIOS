use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "shutdown",
        description: "Shutdown kernel (halt)",
        handler: cmd_shutdown,
    }));
}

fn halt_forever() -> ! {
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}

fn cmd_shutdown(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("Shutdown requested");
    halt_forever()
}
