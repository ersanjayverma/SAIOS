use alloc::boxed::Box;

use crate::console;
use crate::object_manager;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "inspect",
        description: "Inspect one object",
        handler: cmd_inspect,
    }));
}

fn cmd_inspect(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let path = args.first().copied().ok_or("inspect: missing object path")?;
    for line in object_manager::inspect(path)? {
        console::println!("{}", line);
    }
    Ok(())
}
