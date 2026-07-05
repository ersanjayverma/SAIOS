use crate::console;
use crate::scheduler;

use super::commands;
use super::dispatcher::CommandDispatcher;
use super::prompt::{PromptProvider, SessionPromptProvider};
use super::registry::CommandRegistry;
use super::session::CommandContext;
use super::{compatibility, native};

pub struct ShellEngine {
    registry: CommandRegistry,
    dispatcher: CommandDispatcher,
    ctx: CommandContext,
    needs_prompt: bool,
}

impl ShellEngine {
    pub fn new() -> Self {
        let mut registry = CommandRegistry::new();
        commands::register(&mut registry);
        native::register(&mut registry);
        compatibility::register(&mut registry);

        let mut ctx = CommandContext::new();
        ctx.command_catalog = registry.list();

        Self {
            registry,
            dispatcher: CommandDispatcher::new(),
            ctx,
            needs_prompt: true,
        }
    }

    fn refresh_completion_snapshot(&self) {
        let commands = self.registry.names();
        let aliases = self
            .ctx
            .session
            .aliases
            .iter()
            .map(|(k, _)| k.clone())
            .collect();
        super::update_completion_snapshot(commands, aliases);
    }

    pub fn execute_line(&mut self, line: &str) -> Result<(), &'static str> {
        self.dispatcher
            .dispatch(&self.registry, &mut self.ctx, line)
    }

    pub fn set_current_user(&mut self, user: &str) {
        self.ctx.session.current_user = Some(user.into());
        self.ctx.env_set("USER", user);
        self.ctx.env_set("LOGNAME", user);
    }

    fn render_prompt(&self) {
        let provider = SessionPromptProvider::new(&self.ctx.session);
        let prompt = provider.render();
        console::set_input_prompt(prompt.as_str());
        console::print(prompt.as_str());
    }

    pub fn run(&mut self) {
        self.refresh_completion_snapshot();

        while self.ctx.session.running {
            if self.needs_prompt {
                self.render_prompt();
                self.needs_prompt = false;
            }

            if let Some(line) = console::poll_input() {
                let line = line.as_str();
                self.ctx.push_history(line);
                if let Err(e) = self.execute_line(line) {
                    console::println!("{}", e);
                }
                self.refresh_completion_snapshot();

                self.needs_prompt = self.ctx.session.running;
            } else {
                scheduler::yield_now();
            }
        }
    }
}
