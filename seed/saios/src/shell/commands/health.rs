use alloc::boxed::Box;

use crate::console;
use crate::object_manager;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "health",
        description: "Show system health summary",
        handler: cmd_health,
    }));
}

fn cmd_health(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    for line in object_manager::health_summary() {
        console::println!("{}", line);
    }
    Ok(())
}
