use alloc::string::String;
use alloc::vec::Vec;

use super::registry::CommandInfo;

pub struct ShellSession {
    pub running: bool,
    pub current_working_directory: String,
    pub current_namespace: String,
    pub environment: Vec<(String, String)>,
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
                last_exit_code: 0,
                history: Vec::new(),
                prompt: "SNSH>".into(),
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
}
