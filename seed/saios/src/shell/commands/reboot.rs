use alloc::boxed::Box;

use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "reboot",
        description: "Reboot machine",
        handler: cmd_reboot,
    }));
}

fn halt_forever() -> ! {
    loop {
        hal::arch::x86_64::cpu::hlt();
    }
}

fn cmd_reboot(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    hal::arch::x86_64::io::outb(0x64, 0xFE);
    halt_forever()
}
