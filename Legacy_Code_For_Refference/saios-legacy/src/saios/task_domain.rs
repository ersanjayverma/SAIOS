//! SAIOS task-domain abstraction.
//!
//! A TaskDomain is a native grouping label for scheduling, policy, and UI
//! ownership. The first implementation maps domains onto existing process
//! groups so job control, signals, sessions, and foreground TTY behavior keep
//! their POSIX semantics.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDomainKind {
    Foreground,
    Background,
    Service,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskDomain {
    pub id: u32,
    pub kind: TaskDomainKind,
    pub mapped_pgid: u32,
}

impl TaskDomain {
    pub const fn new(kind: TaskDomainKind, mapped_pgid: u32) -> Self {
        Self {
            id: mapped_pgid,
            kind,
            mapped_pgid,
        }
    }

    pub const fn foreground(pgid: u32) -> Self {
        Self::new(TaskDomainKind::Foreground, pgid)
    }

    pub const fn background(pgid: u32) -> Self {
        Self::new(TaskDomainKind::Background, pgid)
    }

    pub const fn service(pgid: u32) -> Self {
        Self::new(TaskDomainKind::Service, pgid)
    }

    pub const fn system(pgid: u32) -> Self {
        Self::new(TaskDomainKind::System, pgid)
    }
}
