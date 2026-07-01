use crate::console;

use super::parser;
use super::registry::CommandRegistry;
use super::session::ShellContext;
use super::{compatibility, native};

const PROMPT: &str = "SNSH>";

pub struct ShellEngine {
    registry: CommandRegistry,
    ctx: ShellContext,
}

impl ShellEngine {
    pub fn new() -> Self {
        let mut registry = CommandRegistry::new();
        native::register(&mut registry);
        compatibility::register(&mut registry);

        let mut ctx = ShellContext::new();
        ctx.command_catalog = registry.list();

        Self { registry, ctx }
    }

    pub fn run(&mut self) -> ! {
        while self.ctx.session.running {
            console::print(PROMPT);
            let line = console::read_line();

            if let Some(parsed) = parser::parse_line(line.as_str()) {
                let args: &[&str] = parsed.args.as_slice();
                match self.registry.find(parsed.command) {
                    Some(command) => {
                        if let Err(e) = command.execute(&mut self.ctx, args) {
                            console::println!("{}", e);
                        }
                    }
                    None => {
                        console::println!("Unknown command");
                    }
                }
            }
        }

        loop {
            hal::arch::x86_64::cpu::hlt();
        }
    }
}
