use alloc::string::String;
use alloc::vec::Vec;

use super::registry::CommandInfo;

pub struct ShellSession {
    pub running: bool,
    pub current_working_directory: String,
    pub current_namespace: String,
    pub environment: Vec<(String, String)>,
    pub aliases: Vec<(String, String)>,
    pub last_exit_code: i32,
    pub history: Vec<String>,
    pub prompt: String,
    pub current_user: Option<String>,
}

pub struct CommandContext {
    pub session: ShellSession,
    pub command_catalog: Vec<CommandInfo>,
}

impl CommandContext {
    pub fn new() -> Self {
        let cwd = crate::saifs::pwd();
        Self {
            session: ShellSession {
                running: true,
                current_working_directory: cwd.clone(),
                current_namespace: cwd,
                environment: Vec::new(),
                aliases: Vec::new(),
                last_exit_code: 0,
                history: Vec::new(),
                prompt: "SAIOS v1.0>".into(),
                current_user: None,
            },
            command_catalog: Vec::new(),
        }
    }

    pub fn push_history(&mut self, line: &str) {
        if !line.is_empty() {
            self.session.history.push(line.into());
        }

        const MAX_HISTORY: usize = 128;
        if self.session.history.len() > MAX_HISTORY {
            let overflow = self.session.history.len() - MAX_HISTORY;
            self.session.history.drain(0..overflow);
        }
    }

    pub fn sync_namespace_from_saifs(&mut self) {
        let cwd = crate::saifs::pwd();
        self.session.current_working_directory = cwd.clone();
        self.session.current_namespace = cwd;
    }

    pub fn env_get(&self, key: &str) -> Option<&str> {
        self.session
            .environment
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn env_set(&mut self, key: &str, value: &str) {
        for (k, v) in self.session.environment.iter_mut() {
            if k == key {
                *v = value.into();
                return;
            }
        }
        self.session.environment.push((key.into(), value.into()));
    }

    pub fn env_unset(&mut self, key: &str) {
        self.session.environment.retain(|(k, _)| k != key);
    }

    pub fn alias_get(&self, key: &str) -> Option<&str> {
        self.session
            .aliases
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn alias_set(&mut self, key: &str, value: &str) {
        for (k, v) in self.session.aliases.iter_mut() {
            if k == key {
                *v = value.into();
                return;
            }
        }
        self.session.aliases.push((key.into(), value.into()));
    }

    pub fn alias_unset(&mut self, key: &str) {
        self.session.aliases.retain(|(k, _)| k != key);
    }
}
