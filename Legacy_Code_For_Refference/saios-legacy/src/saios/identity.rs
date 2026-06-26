//! SAIOS-native identity model.
//!
//! Native identity data lives under `/system/users` and `/system/auth`.
//! `/etc/passwd`, `/etc/group`, and `/etc/shadow` remain compatibility views
//! for POSIX tools and imported Linux userspace.

use alloc::string::String;
use alloc::vec::Vec;

pub const USERS_ROOT: &str = "/system/users";
pub const AUTH_ROOT: &str = "/system/auth";
pub const NATIVE_PASSWD: &str = "/system/users/passwd";
pub const NATIVE_GROUP: &str = "/system/users/group";
pub const NATIVE_SHADOW: &str = "/system/auth/shadow";

pub const COMPAT_PASSWD: &str = "/etc/passwd";
pub const COMPAT_GROUP: &str = "/etc/group";
pub const COMPAT_SHADOW: &str = "/etc/shadow";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeIdentityRecord {
    pub uid: u32,
    pub primary_gid: u32,
    pub username: String,
    pub display_name: String,
    pub home: String,
    pub default_interface: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityMigrationPlan {
    pub native_roots: Vec<&'static str>,
    pub generated_compat_views: Vec<(&'static str, &'static str)>,
}

impl IdentityMigrationPlan {
    pub fn current() -> Self {
        Self {
            native_roots: alloc::vec![USERS_ROOT, AUTH_ROOT],
            generated_compat_views: alloc::vec![
                (COMPAT_PASSWD, NATIVE_PASSWD),
                (COMPAT_GROUP, NATIVE_GROUP),
                (COMPAT_SHADOW, NATIVE_SHADOW),
            ],
        }
    }
}
