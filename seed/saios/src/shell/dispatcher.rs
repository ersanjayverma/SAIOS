use super::command::ShellResult;
use super::parser;
use super::registry::CommandRegistry;
use super::session::CommandContext;
use crate::console;
use crate::kernel::process;

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
                match process::exec(parsed.command, args, ctx.session.environment.as_slice()) {
                    Ok(exit_code) => {
                        ctx.session.last_exit_code = exit_code;
                        if exit_code != 0 {
                            console::println!("exit {}", exit_code);
                        }
                        Ok(())
                    }
                    Err(_) => {
                        console::println!("Unknown command: {}", parsed.command);
                        Ok(())
                    }
                }
            }
        }
    }
}
