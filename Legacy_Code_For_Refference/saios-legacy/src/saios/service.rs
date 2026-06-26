//! SAIOS service registry concepts.
//!
//! This is not a service manager. It is a registry vocabulary for native
//! subsystems so PID 1 does not become the only architectural model for
//! discovering network, audio, display, login, package, or future services.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    Network,
    Audio,
    Display,
    Login,
    Package,
    Storage,
    Ai,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Registered,
    Starting,
    Ready,
    Degraded,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDescriptor {
    pub name: String,
    pub kind: ServiceKind,
    pub state: ServiceState,
    pub owner_uid: u32,
    pub task_domain_id: Option<u32>,
}

#[derive(Debug, Default)]
pub struct ServiceRegistry {
    services: Vec<ServiceDescriptor>,
}

impl ServiceRegistry {
    pub const fn new() -> Self {
        Self {
            services: Vec::new(),
        }
    }

    pub fn register(&mut self, descriptor: ServiceDescriptor) {
        if let Some(existing) = self
            .services
            .iter_mut()
            .find(|service| service.name == descriptor.name)
        {
            *existing = descriptor;
        } else {
            self.services.push(descriptor);
        }
    }

    pub fn get(&self, name: &str) -> Option<&ServiceDescriptor> {
        self.services.iter().find(|service| service.name == name)
    }

    pub fn all(&self) -> &[ServiceDescriptor] {
        &self.services
    }
}
