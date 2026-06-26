//! SAIOS-native architecture surface.
//!
//! This module is intentionally higher level than POSIX syscall and TTY
//! primitives. It gives native SAIOS code stable vocabulary for sessions,
//! identity, user environments, services, permissions, and task domains while
//! the existing Unix-compatible machinery remains the execution substrate.

pub mod identity;
pub mod permission;
pub mod rootfs;
pub mod service;
pub mod session;
pub mod storage_platform;
pub mod task_domain;
pub mod user_environment;
