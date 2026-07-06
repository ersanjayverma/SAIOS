use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "version",
        description: "Show kernel version",
        handler: cmd_version,
    }));
}

fn cmd_version(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!("{}", crate::version::SHELL_BANNER);
    Ok(())
}
