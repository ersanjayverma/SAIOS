use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "clear",
        description: "Clear console output",
        handler: cmd_clear,
    }));
}

fn cmd_clear(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::clear();
    Ok(())
}
