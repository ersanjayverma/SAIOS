//! SAIOS user-environment abstraction.
//!
//! A user environment owns the post-authentication context that chooses an
//! interface. The shell is the default interface today, but it is only one
//! possible consumer of this environment.

use super::session::{InterfaceKind, SessionContext};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserEnvironment {
    pub uid: u32,
    pub gid: u32,
    pub username: String,
    pub home: String,
    pub default_interface: InterfaceKind,
    pub variables: Vec<EnvironmentVariable>,
}

impl UserEnvironment {
    pub fn from_user(user: &crate::user::User, session: &SessionContext) -> Self {
        let variables = vec![
            EnvironmentVariable {
                key: "HOME".to_string(),
                value: user.home.clone(),
            },
            EnvironmentVariable {
                key: "USER".to_string(),
                value: user.username.clone(),
            },
            EnvironmentVariable {
                key: "SHELL".to_string(),
                value: user.shell.clone(),
            },
            EnvironmentVariable {
                key: "SAIOS_SESSION_ID".to_string(),
                value: session.session_id.to_string(),
            },
        ];

        Self {
            uid: user.uid,
            gid: user.gid,
            username: user.username.clone(),
            home: user.home.clone(),
            default_interface: session.interface,
            variables,
        }
    }
}
