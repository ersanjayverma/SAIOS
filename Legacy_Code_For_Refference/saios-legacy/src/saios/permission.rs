//! Future permission-model extension points.
//!
//! POSIX uid/gid ownership and chmod remain authoritative today. These types
//! reserve native places for ACLs, capabilities, and service permissions
//! without changing current enforcement behavior.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionExtensionKind {
    PosixMode,
    Acl,
    Capability,
    ServicePermission,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionExtensionPoint {
    pub kind: PermissionExtensionKind,
    pub policy_path: &'static str,
    pub enforced: bool,
}

pub const POSIX_MODE_EXTENSION: PermissionExtensionPoint = PermissionExtensionPoint {
    kind: PermissionExtensionKind::PosixMode,
    policy_path: "/system/auth/posix-mode",
    enforced: true,
};

pub const ACL_EXTENSION: PermissionExtensionPoint = PermissionExtensionPoint {
    kind: PermissionExtensionKind::Acl,
    policy_path: "/system/auth/acls",
    enforced: false,
};

pub const CAPABILITY_EXTENSION: PermissionExtensionPoint = PermissionExtensionPoint {
    kind: PermissionExtensionKind::Capability,
    policy_path: "/system/auth/capabilities",
    enforced: false,
};

pub const SERVICE_PERMISSION_EXTENSION: PermissionExtensionPoint = PermissionExtensionPoint {
    kind: PermissionExtensionKind::ServicePermission,
    policy_path: "/system/auth/services",
    enforced: false,
};
