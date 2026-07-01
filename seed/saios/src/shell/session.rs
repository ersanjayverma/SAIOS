use alloc::string::String;
use alloc::vec::Vec;

use super::registry::CommandInfo;

pub struct ShellSession {
    pub running: bool,
    pub current_namespace: String,
}

pub struct ShellEnvironment {
    pub vars: Vec<(String, String)>,
}

pub struct ShellContext {
    pub session: ShellSession,
    pub environment: ShellEnvironment,
    pub command_catalog: Vec<CommandInfo>,
}

impl ShellContext {
    pub fn new() -> Self {
        Self {
            session: ShellSession {
                running: true,
                current_namespace: "/".into(),
            },
            environment: ShellEnvironment { vars: Vec::new() },
            command_catalog: Vec::new(),
        }
    }
}
