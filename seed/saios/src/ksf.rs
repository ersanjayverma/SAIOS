//! Kernel Service Framework (KSF).
//!
//! KSF is a minimal dependency-aware service manager. Services implement the
//! [`KernelService`] trait, declare their dependencies and are started in
//! dependency order by [`bootstrap`]. The framework also exposes health,
//! verification and lifecycle helpers used by the shell and boot sequence.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;

use hal::arch::x86_64::sync::StaticCell;

use crate::som::HealthState;
use crate::{object_manager, scheduler, sif, timer};

/// Unique identifier for a kernel service.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ServiceId(pub u16);

pub mod ids {
    use super::ServiceId;

    pub const CONSOLE: ServiceId = ServiceId(1);
    pub const MEMORY: ServiceId = ServiceId(2);
    pub const OBJECT: ServiceId = ServiceId(3);
    pub const PROVIDER: ServiceId = ServiceId(4);
    pub const SIF: ServiceId = ServiceId(5);
    pub const TIMER: ServiceId = ServiceId(6);
    pub const SCHEDULER: ServiceId = ServiceId(7);
    pub const EVENT: ServiceId = ServiceId(8);
    pub const HEALTH: ServiceId = ServiceId(9);
    pub const INPUT: ServiceId = ServiceId(10);
    pub const SHELL: ServiceId = ServiceId(11);
    pub const VFS: ServiceId = ServiceId(12);
    pub const DRIVER_MANAGER: ServiceId = ServiceId(13);
    pub const DEVICE_MANAGER: ServiceId = ServiceId(14);
    pub const PROCESS_MANAGER: ServiceId = ServiceId(15);
    pub const IPC: ServiceId = ServiceId(16);
    pub const NETWORK: ServiceId = ServiceId(17);
    pub const SAIRU: ServiceId = ServiceId(18);
    pub const STORAGE_DISCOVERY: ServiceId = ServiceId(19);
}

/// Lifecycle state of a service managed by KSF.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Registered,
    Initializing,
    Ready,
    Running,
    Paused,
    Stopping,
    Stopped,
    Failed,
}

/// Interface implemented by every kernel service.
///
/// Services declare an identity, version, dependency list and lifecycle
/// callbacks. The `stop` callback is intentionally a no-op for most services
/// because the kernel does not currently support clean shutdown of these
/// subsystems.
pub trait KernelService {
    /// Returns the service's unique identifier.
    fn id(&self) -> ServiceId;
    /// Returns the service's short name.
    fn name(&self) -> &'static str;
    /// Returns the service's version string.
    fn version(&self) -> &'static str;
    /// Returns the list of services that must be running before this one.
    fn dependencies(&self) -> &'static [ServiceId];
    /// Initializes the service. Called before `start`.
    fn initialize(&mut self) -> Result<(), &'static str>;
    /// Starts the service.
    fn start(&mut self) -> Result<(), &'static str>;
    /// Stops the service.
    ///
    /// Most kernel services do not implement a clean shutdown path, so the
    /// default implementation for each service is currently a no-op.
    fn stop(&mut self);
    /// Returns the current health of the service.
    fn health(&self) -> HealthState;
}

/// Snapshot of a service's state returned by [`ServiceManager::snapshots`].
pub struct ServiceSnapshot {
    /// Service identifier.
    pub id: ServiceId,
    /// Service name.
    pub name: String,
    /// Service version.
    pub version: String,
    /// Current lifecycle state.
    pub state: ServiceState,
    /// Current health.
    pub health: HealthState,
    /// Resolved dependency list.
    pub dependencies: Vec<ServiceId>,
}

/// Dependency-aware kernel service manager.
pub struct ServiceManager {
    /// Registered service instances.
    services: Vec<Box<dyn KernelService>>,
    /// Lifecycle state for each registered service.
    states: Vec<ServiceState>,
}

impl Default for ServiceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceManager {
    /// Creates an empty service manager.
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            states: Vec::new(),
        }
    }

    /// Registers a service if no service with the same id is already present.
    pub fn register(&mut self, service: Box<dyn KernelService>) {
        let id = service.id();
        if self.index_of(id).is_some() {
            return;
        }

        self.services.push(service);
        self.states.push(ServiceState::Registered);
    }

    /// Returns the index of the service with `id`, if registered.
    fn index_of(&self, id: ServiceId) -> Option<usize> {
        self.services.iter().position(|s| s.id() == id)
    }

    /// Returns true when all dependencies of the service at `idx` are ready.
    fn deps_ready_to_start(&self, idx: usize) -> bool {
        self.services[idx].dependencies().iter().all(|dep| {
            self.index_of(*dep)
                .and_then(|dep_idx| self.states.get(dep_idx).copied())
                .is_some_and(|state| state == ServiceState::Running || state == ServiceState::Ready)
        })
    }

    /// Initializes the service at `idx` if it is not already ready or running.
    fn init_at(&mut self, idx: usize) -> Result<(), &'static str> {
        match self.states[idx] {
            ServiceState::Registered | ServiceState::Stopped => {
                self.states[idx] = ServiceState::Initializing;
                match self.services[idx].initialize() {
                    Ok(()) => {
                        self.states[idx] = ServiceState::Ready;
                        Ok(())
                    }
                    Err(e) => {
                        self.states[idx] = ServiceState::Failed;
                        Err(e)
                    }
                }
            }
            ServiceState::Ready | ServiceState::Running => Ok(()),
            ServiceState::Initializing | ServiceState::Stopping | ServiceState::Paused => {
                Err("service busy")
            }
            ServiceState::Failed => Err("service failed"),
        }
    }

    /// Starts the service at `idx` after ensuring its dependencies are ready.
    fn start_at(&mut self, idx: usize) -> Result<(), &'static str> {
        if !self.deps_ready_to_start(idx) {
            return Err("dependencies not ready");
        }

        if self.states[idx] == ServiceState::Running {
            return Ok(());
        }

        self.init_at(idx)?;

        match self.services[idx].start() {
            Ok(()) => {
                self.states[idx] = ServiceState::Running;
                crate::kernel::timeline::mark_service(self.services[idx].name());
                Ok(())
            }
            Err(e) => {
                self.states[idx] = ServiceState::Failed;
                Err(e)
            }
        }
    }

    /// Starts all registered services in dependency order.
    pub fn start_all(&mut self) -> Result<(), &'static str> {
        let mut progressed = true;

        while progressed {
            progressed = false;
            for idx in 0..self.services.len() {
                if self.states[idx] == ServiceState::Running {
                    continue;
                }

                if self.deps_ready_to_start(idx) {
                    match self.start_at(idx) {
                        Ok(()) => {
                            progressed = true;
                        }
                        Err(e) => {
                            crate::console::println!(
                                "ksf: service '{}' failed: {}",
                                self.services[idx].name(),
                                e
                            );
                        }
                    }
                }
            }

            if self
                .states
                .iter()
                .all(|s| *s == ServiceState::Running || *s == ServiceState::Failed)
            {
                break;
            }
        }

        if self.states.iter().any(|s| *s != ServiceState::Running) {
            return Err("one or more services failed to start");
        }

        Ok(())
    }

    /// Starts the service with the given name.
    pub fn start_by_name(&mut self, name: &str) -> Result<(), &'static str> {
        let idx = self
            .services
            .iter()
            .position(|s| s.name().eq_ignore_ascii_case(name))
            .ok_or("service not found")?;
        self.start_at(idx)
    }

    /// Stops the service with the given name.
    ///
    /// The service's `stop` callback is invoked; most services currently
    /// implement this as a no-op.
    pub fn stop_by_name(&mut self, name: &str) -> Result<(), &'static str> {
        let idx = self
            .services
            .iter()
            .position(|s| s.name().eq_ignore_ascii_case(name))
            .ok_or("service not found")?;

        self.states[idx] = ServiceState::Stopping;
        self.services[idx].stop();
        self.states[idx] = ServiceState::Stopped;
        Ok(())
    }

    /// Stops and then restarts the service with the given name.
    pub fn restart_by_name(&mut self, name: &str) -> Result<(), &'static str> {
        self.stop_by_name(name)?;
        self.start_by_name(name)
    }

    /// Returns a snapshot of every registered service.
    pub fn snapshots(&self) -> Vec<ServiceSnapshot> {
        self.services
            .iter()
            .zip(self.states.iter())
            .map(|(svc, state)| ServiceSnapshot {
                id: svc.id(),
                name: svc.name().to_string(),
                version: svc.version().to_string(),
                state: *state,
                health: svc.health(),
                dependencies: svc.dependencies().to_vec(),
            })
            .collect()
    }
}

static MANAGER: StaticCell<Option<ServiceManager>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    hal::arch::x86_64::sync::spinlock_acquire(&LOCK);
}

fn unlock() {
    hal::arch::x86_64::sync::spinlock_release(&LOCK);
}

fn with_manager<R>(f: impl FnOnce(&mut ServiceManager) -> R) -> R {
    lock();
    let out = {
        let manager = unsafe {
            let slot = &mut *MANAGER.get();
            if slot.is_none() {
                *slot = Some(ServiceManager::new());
            }
            slot.as_mut().expect("service manager unavailable")
        };
        f(manager)
    };
    unlock();
    out
}

struct ConsoleService;

impl KernelService for ConsoleService {
    fn id(&self) -> ServiceId {
        ids::CONSOLE
    }

    fn name(&self) -> &'static str {
        "console"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        // Keep console startup side-effect free. Hardware-heavy PCI, storage,
        // USB and network scans are available through the driver manager and
        // explicit shell commands, but must not block early boot on real HW.
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct MemoryService;

impl KernelService for MemoryService {
    fn id(&self) -> ServiceId {
        ids::MEMORY
    }

    fn name(&self) -> &'static str {
        "memory"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::CONSOLE]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        // PMM and heap are boot-strapped before KSF currently.
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct ObjectService;

impl KernelService for ObjectService {
    fn id(&self) -> ServiceId {
        ids::OBJECT
    }

    fn name(&self) -> &'static str {
        "object"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::MEMORY]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        object_manager::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct ProviderService;

impl KernelService for ProviderService {
    fn id(&self) -> ServiceId {
        ids::PROVIDER
    }

    fn name(&self) -> &'static str {
        "provider"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::OBJECT]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        // Providers are loaded via ObjectManager init path.
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct SifService;

impl KernelService for SifService {
    fn id(&self) -> ServiceId {
        ids::SIF
    }

    fn name(&self) -> &'static str {
        "sif"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::PROVIDER]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        sif::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct TimerService;

impl KernelService for TimerService {
    fn id(&self) -> ServiceId {
        ids::TIMER
    }

    fn name(&self) -> &'static str {
        "timer"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::MEMORY]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        timer::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct SchedulerService;

impl KernelService for SchedulerService {
    fn id(&self) -> ServiceId {
        ids::SCHEDULER
    }

    fn name(&self) -> &'static str {
        "scheduler"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::TIMER]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        scheduler::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct EventService;

impl KernelService for EventService {
    fn id(&self) -> ServiceId {
        ids::EVENT
    }

    fn name(&self) -> &'static str {
        "event"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::SIF]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        crate::kernel::event::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct HealthService;

impl KernelService for HealthService {
    fn id(&self) -> ServiceId {
        ids::HEALTH
    }

    fn name(&self) -> &'static str {
        "health"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::EVENT]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct InputService;

impl KernelService for InputService {
    fn id(&self) -> ServiceId {
        ids::INPUT
    }

    fn name(&self) -> &'static str {
        "input"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::CONSOLE]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct UserSessionService;

impl KernelService for UserSessionService {
    fn id(&self) -> ServiceId {
        ids::SHELL
    }

    fn name(&self) -> &'static str {
        "user-session"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::CONSOLE, ids::INPUT, ids::SIF, ids::SCHEDULER]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        scheduler::prepare_default_user_session()?;
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        scheduler::start_default_user_session()
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct VfsService;

impl KernelService for VfsService {
    fn id(&self) -> ServiceId {
        ids::VFS
    }

    fn name(&self) -> &'static str {
        "vfs"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::SIF]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct DriverManagerService;

impl KernelService for DriverManagerService {
    fn id(&self) -> ServiceId {
        ids::DRIVER_MANAGER
    }

    fn name(&self) -> &'static str {
        "driver-manager"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::OBJECT]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        crate::kernel::driver::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct DeviceManagerService;

impl KernelService for DeviceManagerService {
    fn id(&self) -> ServiceId {
        ids::DEVICE_MANAGER
    }

    fn name(&self) -> &'static str {
        "device-manager"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::DRIVER_MANAGER]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        crate::kernel::device::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct ProcessManagerService;

impl KernelService for ProcessManagerService {
    fn id(&self) -> ServiceId {
        ids::PROCESS_MANAGER
    }

    fn name(&self) -> &'static str {
        "process-manager"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::SCHEDULER]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        crate::kernel::process::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct StorageDiscoveryService;

impl KernelService for StorageDiscoveryService {
    fn id(&self) -> ServiceId {
        ids::STORAGE_DISCOVERY
    }

    fn name(&self) -> &'static str {
        "storage-discovery"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::SCHEDULER, ids::DEVICE_MANAGER]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        crate::driver::storage::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        crate::console::println!("storage: foreground scan begin");
        crate::driver::storage::request_rescan();
        let status = crate::driver::storage::scan_status();
        crate::console::println!(
            "storage: foreground scan done phase={} disks={} volumes={} failures={}",
            status.phase,
            status.disks,
            status.volumes,
            status.failures
        );
        Ok(())
    }

    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct IpcService;

impl KernelService for IpcService {
    fn id(&self) -> ServiceId {
        ids::IPC
    }

    fn name(&self) -> &'static str {
        "ipc"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::PROCESS_MANAGER]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct NetworkService;

impl KernelService for NetworkService {
    fn id(&self) -> ServiceId {
        ids::NETWORK
    }

    fn name(&self) -> &'static str {
        "network"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::IPC]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Warning
    }
}

struct SairuService;

impl KernelService for SairuService {
    fn id(&self) -> ServiceId {
        ids::SAIRU
    }

    fn name(&self) -> &'static str {
        "sairu"
    }

    fn version(&self) -> &'static str {
        crate::version::PRODUCT_VERSION
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::HEALTH, ids::EVENT]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    /// Stops the service. Currently a no-op because clean kernel shutdown is not implemented.
    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

/// Registers all built-in kernel services and starts them in dependency order.
pub fn bootstrap() -> Result<(), &'static str> {
    with_manager(|manager| {
        manager.register(Box::new(ConsoleService));
        manager.register(Box::new(MemoryService));
        manager.register(Box::new(ObjectService));
        manager.register(Box::new(ProviderService));
        manager.register(Box::new(SifService));
        manager.register(Box::new(TimerService));
        manager.register(Box::new(SchedulerService));
        manager.register(Box::new(EventService));
        manager.register(Box::new(HealthService));
        manager.register(Box::new(InputService));
        manager.register(Box::new(VfsService));
        manager.register(Box::new(DriverManagerService));
        manager.register(Box::new(DeviceManagerService));
        manager.register(Box::new(StorageDiscoveryService));
        manager.register(Box::new(ProcessManagerService));
        manager.register(Box::new(IpcService));
        manager.register(Box::new(NetworkService));
        manager.register(Box::new(SairuService));
        manager.register(Box::new(UserSessionService));
        manager.start_all()
    })
}

/// Returns snapshots of all registered services.
pub fn list() -> Vec<ServiceSnapshot> {
    with_manager(|manager| manager.snapshots())
}

/// Starts the service with the given name.
pub fn start(name: &str) -> Result<(), &'static str> {
    with_manager(|manager| manager.start_by_name(name))
}

/// Stops the service with the given name.
pub fn stop(name: &str) -> Result<(), &'static str> {
    with_manager(|manager| manager.stop_by_name(name))
}

/// Restarts the service with the given name.
pub fn restart(name: &str) -> Result<(), &'static str> {
    with_manager(|manager| manager.restart_by_name(name))
}

/// Returns the current health of every registered service.
pub fn health() -> Vec<(String, HealthState)> {
    with_manager(|manager| {
        manager
            .snapshots()
            .into_iter()
            .map(|s| (s.name, s.health))
            .collect()
    })
}

/// Returns a snapshot for the service with the given name, if it exists.
pub fn info(name: &str) -> Option<ServiceSnapshot> {
    with_manager(|manager| {
        manager
            .snapshots()
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    })
}

/// Verifies the service registry and returns a report.
pub fn verify() -> crate::kernel::testing::report::VerifyReport {
    let snapshots = list();
    let mut checks = Vec::new();

    checks.push(if snapshots.is_empty() {
        crate::kernel::testing::report::VerifyCheck::fail(
            "Service registry",
            "no services registered",
        )
    } else {
        crate::kernel::testing::report::VerifyCheck::pass(
            "Service registry",
            "services are registered",
        )
    });

    let mut unique_names = true;
    let mut unique_ids = true;
    for i in 0..snapshots.len() {
        for j in (i + 1)..snapshots.len() {
            if snapshots[i].name == snapshots[j].name {
                unique_names = false;
            }
            if snapshots[i].id == snapshots[j].id {
                unique_ids = false;
            }
        }
    }

    checks.push(if unique_ids {
        crate::kernel::testing::report::VerifyCheck::pass("Service ids", "all ids are unique")
    } else {
        crate::kernel::testing::report::VerifyCheck::fail(
            "Service ids",
            "duplicate service id found",
        )
    });

    checks.push(if unique_names {
        crate::kernel::testing::report::VerifyCheck::pass("Service names", "all names are unique")
    } else {
        crate::kernel::testing::report::VerifyCheck::fail(
            "Service names",
            "duplicate service name found",
        )
    });

    let mut deps_resolve = true;
    for snapshot in &snapshots {
        for dep in &snapshot.dependencies {
            if snapshots.iter().find(|s| s.id == *dep).is_none() {
                deps_resolve = false;
            }
        }
    }

    checks.push(if deps_resolve {
        crate::kernel::testing::report::VerifyCheck::pass(
            "Dependencies",
            "all dependencies resolve",
        )
    } else {
        crate::kernel::testing::report::VerifyCheck::fail(
            "Dependencies",
            "unresolved service dependency",
        )
    });

    crate::kernel::testing::report::VerifyReport {
        target: "service",
        checks,
    }
}
