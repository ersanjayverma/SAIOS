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
    registry.register(Box::new(StaticCommand {
        name: "man",
        description: "Show manual page for a command",
        handler: cmd_man,
    }));
}

fn print_command_table(ctx: &CommandContext) {
    let name_width = ctx
        .command_catalog
        .iter()
        .map(|item| item.name.len())
        .max()
        .unwrap_or(7)
        .max(7);

    console::println!("{:<width$}  DESCRIPTION", "COMMAND", width = name_width);
    console::println!("{:-<width$}  {:-<11}", "", "", width = name_width);

    for item in &ctx.command_catalog {
        console::println!(
            "{:<width$}  {}",
            item.name,
            item.description,
            width = name_width
        );
    }
}

fn cmd_help(ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!(
        "namespace={} env_vars={}",
        ctx.session.current_namespace,
        ctx.session.environment.len()
    );

    print_command_table(ctx);

    Ok(())
}

fn cmd_man(ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if let Some(topic) = args.first().copied() {
        super::super::man::print_command(ctx, topic)
    } else {
        super::super::man::print_index(ctx);
        Ok(())
    }
}
