use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "help",
        description: "List registered commands",
        handler: cmd_help,
    }));
}

fn cmd_help(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!(
        "namespace={} env_vars={}",
        ctx.session.current_namespace,
        ctx.session.environment.len()
    );

    for item in &ctx.command_catalog {
        console::println!("{} - {}", item.name, item.description);
    }

    Ok(())
}
