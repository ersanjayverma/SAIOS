use alloc::boxed::Box;
use alloc::vec::Vec;

use super::command::Command;

#[derive(Copy, Clone)]
pub struct CommandInfo {
    pub name: &'static str,
    pub description: &'static str,
}

pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn register(&mut self, command: Box<dyn Command>) {
        if self
            .commands
            .iter()
            .any(|existing| existing.name().eq_ignore_ascii_case(command.name()))
        {
            return;
        }
        self.commands.push(command);
    }

    pub fn find(&self, name: &str) -> Option<&dyn Command> {
        self.commands
            .iter()
            .find(|cmd| cmd.name().eq_ignore_ascii_case(name))
            .map(|cmd| cmd.as_ref())
    }

    pub fn list(&self) -> Vec<CommandInfo> {
        let mut out: Vec<CommandInfo> = self
            .commands
            .iter()
            .map(|cmd| CommandInfo {
                name: cmd.name(),
                description: cmd.description(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(b.name));
        out
    }
}
