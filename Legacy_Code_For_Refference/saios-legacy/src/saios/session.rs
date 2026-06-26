//! SAIOS session architecture.
//!
//! The native session manager composes a provider, an authentication provider,
//! a user environment, and an interface. Console getty/login is one provider,
//! not the architecture itself.

use super::task_domain::TaskDomain;
use super::user_environment::UserEnvironment;
use alloc::string::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionProviderKind {
    Console,
    Ssh,
    Gui,
    Remote,
    Ai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationProviderKind {
    LocalPassword,
    Token,
    Remote,
    Biometric,
    AiBrokered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceKind {
    Shell,
    Gui,
    Dashboard,
    RemoteSession,
    Ai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionContext {
    pub session_id: u32,
    pub provider: SessionProviderKind,
    pub auth_provider: AuthenticationProviderKind,
    pub interface: InterfaceKind,
    pub task_domain: TaskDomain,
    pub controlling_tty: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ConsoleSessionBootstrap {
    pub session: SessionContext,
    pub user: crate::user::User,
    pub environment: UserEnvironment,
    pub initial_cwd: String,
}

impl SessionContext {
    pub const fn console_shell(session_id: u32, pgid: u32, controlling_tty: u64) -> Self {
        Self {
            session_id,
            provider: SessionProviderKind::Console,
            auth_provider: AuthenticationProviderKind::LocalPassword,
            interface: InterfaceKind::Shell,
            task_domain: TaskDomain::foreground(pgid),
            controlling_tty: Some(controlling_tty),
        }
    }
}

pub fn bootstrap_console_shell(shell_pid: u32, controlling_tty: u64) -> ConsoleSessionBootstrap {
    let session = SessionContext::console_shell(shell_pid, shell_pid, controlling_tty);
    let user = crate::user::get_current_user()
        .or_else(|| crate::user::get_user_by_name("root"))
        .unwrap_or_else(|| crate::user::User {
            uid: 0,
            gid: 0,
            username: String::from("root"),
            home: String::from("/users/root"),
            shell: String::from("/bin/sh"),
        });
    let environment = UserEnvironment::from_user(&user, &session);

    ConsoleSessionBootstrap {
        session,
        user,
        environment,
        initial_cwd: String::from("/"),
    }
}
