//! User and group management for SAIOS.
//!
//! Implements a basic UNIX-style user/group system with:
//! - User database in /system/users/passwd
//! - UID/GID assignment
//! - Process credentials (uid, gid, euid, egid)
//! - File ownership (st_uid, st_gid)
//! - Basic permission checks

use crate::vfs::{
    self, DirEntry, FileType, Inode, InodeOps, Stat, VfsError, VfsResult, alloc_ino, file::OpenFile,
};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

/// User structure representing a system user
#[derive(Debug, Clone)]
pub struct User {
    pub uid: u32,
    pub gid: u32,
    pub username: String,
    pub home: String,
    pub shell: String,
}

/// Group structure representing a system group
#[derive(Debug, Clone)]
pub struct Group {
    pub gid: u32,
    pub name: String,
}

/// Global user registry
static USER_REGISTRY: Mutex<UserRegistry> = Mutex::new(UserRegistry::new());

/// User registry holding all users and groups
struct UserRegistry {
    users: Vec<User>,
    groups: Vec<Group>,
}

impl UserRegistry {
    const fn new() -> Self {
        UserRegistry {
            users: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Initialize the registry with default users
    fn init_defaults(&mut self) {
        // Root user
        self.users.push(User {
            uid: 0,
            gid: 0,
            username: String::from("root"),
            home: String::from("/users/root"),
            shell: String::from("/bin/bash"),
        });

        // Nobody user
        self.users.push(User {
            uid: 65534,
            gid: 65534,
            username: String::from("nobody"),
            home: String::from("/dev/null"),
            shell: String::from("/bin/false"),
        });

        // Root group
        self.groups.push(Group {
            gid: 0,
            name: String::from("root"),
        });

        // Nobody group
        self.groups.push(Group {
            gid: 65534,
            name: String::from("nobody"),
        });
    }

    fn clear(&mut self) {
        self.users.clear();
        self.groups.clear();
    }

    /// Add a new user to the registry
    fn add_user(&mut self, user: User) {
        self.users.push(user);
    }

    /// Add a new group to the registry
    fn add_group(&mut self, group: Group) {
        self.groups.push(group);
    }

    /// Find user by UID
    fn find_user_by_uid(&self, uid: u32) -> Option<&User> {
        self.users.iter().find(|u| u.uid == uid)
    }

    /// Find user by username
    fn find_user_by_name(&self, username: &str) -> Option<&User> {
        self.users.iter().find(|u| u.username == username)
    }

    /// Find group by GID
    fn find_group_by_gid(&self, gid: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.gid == gid)
    }

    /// Find group by name
    fn find_group_by_name(&self, name: &str) -> Option<&Group> {
        self.groups.iter().find(|g| g.name == name)
    }

    /// Get all users
    fn get_users(&self) -> &[User] {
        &self.users
    }

    /// Get all groups
    fn get_groups(&self) -> &[Group] {
        &self.groups
    }
}

/// Initialize the user system
pub fn init() {
    let mut registry = USER_REGISTRY.lock();
    registry.clear();

    // Try to load users from /system/users/passwd if it exists
    load_users_from_file(&mut registry);
    if registry.users.is_empty() {
        registry.init_defaults();
        let _ = save_registry_to_files(&registry);
    }
}

/// Load users from the passwd file
fn load_users_from_file(registry: &mut UserRegistry) {
    // Try to read the passwd file
    if let Ok(buffer) = crate::vfs_contract::VfsContract::read_file("/system/users/passwd") {
        // Parse the content
        if let Ok(content) = core::str::from_utf8(&buffer) {
            for line in content.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 7
                    && let (Ok(uid), Ok(gid)) = (parts[2].parse::<u32>(), parts[3].parse::<u32>())
                    && registry.find_user_by_uid(uid).is_none()
                {
                    let user = User {
                        uid,
                        gid,
                        username: parts[0].to_string(),
                        home: parts[5].to_string(),
                        shell: parts[6].to_string(),
                    };
                    registry.add_user(user);

                    // Also add a group with the same ID if it doesn't exist
                    if registry.find_group_by_gid(gid).is_none() {
                        let group = Group {
                            gid,
                            name: parts[0].to_string(),
                        };
                        registry.add_group(group);
                    }
                }
            }
        }
    }
}

/// Save users to the passwd file
fn save_users_to_file() -> VfsResult<()> {
    let registry = USER_REGISTRY.lock();
    save_registry_to_files(&registry)
}

fn save_registry_to_files(registry: &UserRegistry) -> VfsResult<()> {
    let mut passwd = String::new();
    let mut group = String::new();
    let mut shadow = String::new();

    for user in registry.get_users() {
        let gecos = if user.username == "root" {
            "root"
        } else {
            user.username.as_str()
        };
        passwd.push_str(&format!(
            "{}:x:{}:{}:{}:{}:{}\n",
            user.username, user.uid, user.gid, gecos, user.home, user.shell
        ));
        shadow.push_str(&format!("{}:*:19700:0:99999:7:::\n", user.username));
        create_home_directory(&user.home, user.uid, user.gid);
    }

    for entry in registry.get_groups() {
        group.push_str(&format!("{}:x:{}:\n", entry.name, entry.gid));
    }

    write_text_file(crate::saios::identity::NATIVE_PASSWD, &passwd)?;
    write_text_file(crate::saios::identity::NATIVE_GROUP, &group)?;
    write_text_file(crate::saios::identity::NATIVE_SHADOW, &shadow)?;

    if !crate::ensure_symlink_pub(
        crate::saios::identity::COMPAT_PASSWD,
        crate::saios::identity::NATIVE_PASSWD,
    ) {
        write_text_file(crate::saios::identity::COMPAT_PASSWD, &passwd)?;
    }
    if !crate::ensure_symlink_pub(
        crate::saios::identity::COMPAT_GROUP,
        crate::saios::identity::NATIVE_GROUP,
    ) {
        write_text_file(crate::saios::identity::COMPAT_GROUP, &group)?;
    }
    if !crate::ensure_symlink_pub(
        crate::saios::identity::COMPAT_SHADOW,
        crate::saios::identity::NATIVE_SHADOW,
    ) {
        write_text_file(crate::saios::identity::COMPAT_SHADOW, &shadow)?;
    }

    Ok(())
}

fn write_text_file(path: &str, text: &str) -> VfsResult<()> {
    crate::write_file_pub(path, text.as_bytes());
    Ok(())
}

/// Add a new user
pub fn add_user(username: String, home: Option<String>) -> Result<u32, &'static str> {
    let mut registry = USER_REGISTRY.lock();

    // Check if user already exists
    if registry.find_user_by_name(&username).is_some() {
        return Err("User already exists");
    }

    // Find next available UID (starting from 1000)
    let mut next_uid = 1000;
    for user in registry.get_users() {
        if user.uid >= next_uid && user.uid < 65534 {
            next_uid = user.uid + 1;
        }
    }

    // Use provided home or create default
    let home_dir = home.unwrap_or_else(|| format!("/users/{}", username));

    let user = User {
        uid: next_uid,
        gid: next_uid, // Use same ID for group
        username,
        home: home_dir.clone(),
        shell: String::from("/bin/bash"),
    };

    registry.add_user(user);

    // Also add a group with the same ID
    let group = Group {
        gid: next_uid,
        name: registry
            .find_user_by_uid(next_uid)
            .unwrap()
            .username
            .clone(),
    };
    registry.add_group(group);

    // Try to create home directory
    create_home_directory(&home_dir, next_uid, next_uid);

    // Save to file
    let _ = save_users_to_file();

    Ok(next_uid)
}

/// Create home directory for a user
fn create_home_directory(path: &str, _uid: u32, _gid: u32) {
    // Try to create the home directory
    let _ = crate::vfs_contract::VfsContract::mkdir(path, 0o755);

    if let Some(username) = path.strip_prefix("/users/") {
        let compat_path = format!("/home/{}", username.trim_matches('/'));
        let _ = crate::vfs_contract::VfsContract::mkdir(&compat_path, 0o755);
    }
}

/// Get current user (for the current process)
pub fn get_current_user() -> Option<User> {
    let uid = crate::process::table::TABLE
        .lock()
        .current_ref()
        .map(|proc| proc.uid)
        .unwrap_or(0);
    let registry = USER_REGISTRY.lock();
    registry.find_user_by_uid(uid).cloned()
}

/// Get user by UID
pub fn get_user_by_uid(uid: u32) -> Option<User> {
    let registry = USER_REGISTRY.lock();
    registry.find_user_by_uid(uid).cloned()
}

/// Get user by username
pub fn get_user_by_name(username: &str) -> Option<User> {
    let registry = USER_REGISTRY.lock();
    registry.find_user_by_name(username).cloned()
}

/// Get all users
pub fn get_all_users() -> Vec<User> {
    let registry = USER_REGISTRY.lock();
    registry.get_users().to_vec()
}

/// Get all groups
pub fn get_all_groups() -> Vec<Group> {
    let registry = USER_REGISTRY.lock();
    registry.get_groups().to_vec()
}

/// Get group by name
pub fn get_group_by_name(name: &str) -> Option<Group> {
    let registry = USER_REGISTRY.lock();
    registry.find_group_by_name(name).cloned()
}

/// Get current process credentials (uid, gid, euid, egid)
pub fn get_current_credentials() -> (u32, u32, u32, u32) {
    match crate::process::table::TABLE.lock().current_ref() {
        Some(proc) => (proc.uid, proc.gid, proc.euid, proc.egid),
        None => (0, 0, 0, 0), // Kernel context - root
    }
}

/// Check if the current process has permission to perform an operation on a file
pub fn check_permission(stat: &Stat, operation: PermissionOperation) -> bool {
    // Get current process credentials
    let (uid, gid, euid, egid) = match crate::process::table::TABLE.lock().current_ref() {
        Some(proc) => (proc.uid, proc.gid, proc.euid, proc.egid),
        None => (0, 0, 0, 0), // Kernel context - allow everything
    };

    // Root can do anything
    if uid == 0 || euid == 0 {
        return true;
    }

    // Check permissions based on owner, group, or other
    let mode = stat.st_mode;

    match operation {
        PermissionOperation::Read => {
            // Owner read permission
            if uid == stat.st_uid && (mode & 0o400) != 0 {
                return true;
            }
            // Group read permission
            if gid == stat.st_gid && (mode & 0o040) != 0 {
                return true;
            }
            // Other read permission
            if (mode & 0o004) != 0 {
                return true;
            }
            false
        }
        PermissionOperation::Write => {
            // Owner write permission
            if uid == stat.st_uid && (mode & 0o200) != 0 {
                return true;
            }
            // Group write permission
            if gid == stat.st_gid && (mode & 0o020) != 0 {
                return true;
            }
            // Other write permission
            if (mode & 0o002) != 0 {
                return true;
            }
            false
        }
        PermissionOperation::Execute => {
            // Owner execute permission
            if uid == stat.st_uid && (mode & 0o100) != 0 {
                return true;
            }
            // Group execute permission
            if gid == stat.st_gid && (mode & 0o010) != 0 {
                return true;
            }
            // Other execute permission
            if (mode & 0o001) != 0 {
                return true;
            }
            false
        }
    }
}

/// Types of permission operations
#[derive(Debug, Clone, Copy)]
pub enum PermissionOperation {
    Read,
    Write,
    Execute,
}
