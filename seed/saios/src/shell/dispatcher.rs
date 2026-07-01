use super::command::ShellResult;
use super::parser;
use super::registry::CommandRegistry;
use super::session::CommandContext;
use crate::console;

pub struct CommandDispatcher;

impl CommandDispatcher {
    pub const fn new() -> Self {
        Self
    }

    pub fn dispatch(&self, registry: &CommandRegistry, ctx: &mut CommandContext, line: &str) -> ShellResult {
        let parsed = match parser::parse_line(line) {
            Some(parsed) => parsed,
            None => return Ok(()),
        };

        let args: &[&str] = parsed.args.as_slice();
        match registry.find(parsed.command) {
            Some(command) => command.execute(ctx, args),
            None => {
                if super::programs::launch(parsed.command).is_ok() {
                    Ok(())
                } else {
                    console::println!("Unknown command: {}", parsed.command);
                    Ok(())
                }
            }
        }
    }
}
