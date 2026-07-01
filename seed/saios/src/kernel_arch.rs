use alloc::vec::Vec;

use crate::som::ObjectId;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum KernelLayer {
    Manager,
    Provider,
    Service,
    Hal,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ExecutionContext {
    Boot,
    Interrupt,
    Scheduler,
    Worker,
    User,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ResourceLifecycle {
    Created,
    Initialized,
    Registered,
    Active,
    Suspended,
    Stopping,
    Destroyed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BootStage {
    Firmware,
    Bootloader,
    Hal,
    MemoryManager,
    ObjectManager,
    ProviderRegistry,
    Sif,
    Saifs,
    Scheduler,
    DeviceManager,
    Drivers,
    Services,
    Shell,
    UserSpace,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DependencyDirection {
    DownwardOnly,
}

pub trait ManagedResource {
    fn object_id(&self) -> ObjectId;
    fn lifecycle(&self) -> ResourceLifecycle;
}

pub trait Manager {
    fn name(&self) -> &str;
    fn layer(&self) -> KernelLayer {
        KernelLayer::Manager
    }

    fn owns(&self, object: ObjectId) -> bool;
    fn lifecycle_of(&self, object: ObjectId) -> Option<ResourceLifecycle>;
}

pub trait ProviderAdapter {
    fn name(&self) -> &str;
    fn layer(&self) -> KernelLayer {
        KernelLayer::Provider
    }

    fn source_manager(&self) -> &str;
    fn enumerate(&self) -> Vec<ObjectId>;
}

pub trait KernelService {
    fn name(&self) -> &str;
    fn layer(&self) -> KernelLayer {
        KernelLayer::Service
    }

    fn start(&mut self);
    fn stop(&mut self);
}

pub trait ContextAwareApi {
    fn supported_contexts(&self) -> &'static [ExecutionContext];

    fn supports_context(&self, ctx: ExecutionContext) -> bool {
        self.supported_contexts().contains(&ctx)
    }
}

pub const KERNEL_CONSTITUTION: [&str; 10] = [
    "Everything is a kernel object.",
    "Every object is owned by exactly one manager.",
    "Providers expose objects but never own them.",
    "Services operate on objects but never own them.",
    "Hardware is accessed only through the HAL.",
    "Dependencies flow downward only.",
    "Every object participates in events, health, metrics, and discovery.",
    "Every public kernel API declares its execution context.",
    "Every resource follows the same lifecycle.",
    "Identity, storage, and representation are separate concerns.",
];

pub const BOOT_ORDER: [BootStage; 14] = [
    BootStage::Firmware,
    BootStage::Bootloader,
    BootStage::Hal,
    BootStage::MemoryManager,
    BootStage::ObjectManager,
    BootStage::ProviderRegistry,
    BootStage::Sif,
    BootStage::Saifs,
    BootStage::Scheduler,
    BootStage::DeviceManager,
    BootStage::Drivers,
    BootStage::Services,
    BootStage::Shell,
    BootStage::UserSpace,
];
