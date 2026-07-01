use alloc::boxed::Box;

use crate::console;
use crate::object_manager;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "objects",
        description: "List object kinds",
        handler: cmd_objects,
    }));
}

fn cmd_objects(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    for ty in object_manager::object_types() {
        console::println!("{}", ty);
    }
    Ok(())
}
