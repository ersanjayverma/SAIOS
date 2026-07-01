use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::som::HealthState;
use crate::{object_manager, scheduler, shell, sif, timer};

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
    pub const SHELL: ServiceId = ServiceId(10);
}

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

pub trait KernelService {
    fn id(&self) -> ServiceId;
    fn name(&self) -> &'static str;
    fn dependencies(&self) -> &'static [ServiceId];
    fn initialize(&mut self) -> Result<(), &'static str>;
    fn start(&mut self) -> Result<(), &'static str>;
    fn stop(&mut self);
    fn health(&self) -> HealthState;
}

pub struct ServiceSnapshot {
    pub id: ServiceId,
    pub name: String,
    pub state: ServiceState,
    pub health: HealthState,
    pub dependencies: Vec<ServiceId>,
}

pub struct ServiceManager {
    services: Vec<Box<dyn KernelService>>,
    states: Vec<ServiceState>,
}

impl ServiceManager {
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            states: Vec::new(),
        }
    }

    pub fn register(&mut self, service: Box<dyn KernelService>) {
        let id = service.id();
        if self.index_of(id).is_some() {
            return;
        }

        self.services.push(service);
        self.states.push(ServiceState::Registered);
    }

    fn index_of(&self, id: ServiceId) -> Option<usize> {
        self.services.iter().position(|s| s.id() == id)
    }

    fn deps_ready_to_start(&self, idx: usize) -> bool {
        self.services[idx].dependencies().iter().all(|dep| {
            self.index_of(*dep)
                .and_then(|dep_idx| self.states.get(dep_idx).copied())
                .is_some_and(|state| state == ServiceState::Running || state == ServiceState::Ready)
        })
    }

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
                Ok(())
            }
            Err(e) => {
                self.states[idx] = ServiceState::Failed;
                Err(e)
            }
        }
    }

    pub fn start_all(&mut self) -> Result<(), &'static str> {
        let mut progressed = true;

        while progressed {
            progressed = false;
            for idx in 0..self.services.len() {
                if self.states[idx] == ServiceState::Running {
                    continue;
                }

                if self.deps_ready_to_start(idx) && self.start_at(idx).is_ok() {
                    progressed = true;
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

    pub fn start_by_name(&mut self, name: &str) -> Result<(), &'static str> {
        let idx = self
            .services
            .iter()
            .position(|s| s.name().eq_ignore_ascii_case(name))
            .ok_or("service not found")?;
        self.start_at(idx)
    }

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

    pub fn restart_by_name(&mut self, name: &str) -> Result<(), &'static str> {
        self.stop_by_name(name)?;
        self.start_by_name(name)
    }

    pub fn snapshots(&self) -> Vec<ServiceSnapshot> {
        self.services
            .iter()
            .zip(self.states.iter())
            .map(|(svc, state)| ServiceSnapshot {
                id: svc.id(),
                name: svc.name().to_string(),
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
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
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

    fn dependencies(&self) -> &'static [ServiceId] {
        &[]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

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

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::SIF]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

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

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::EVENT]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

struct ShellService;

impl KernelService for ShellService {
    fn id(&self) -> ServiceId {
        ids::SHELL
    }

    fn name(&self) -> &'static str {
        "shell"
    }

    fn dependencies(&self) -> &'static [ServiceId] {
        &[ids::CONSOLE, ids::SIF, ids::SCHEDULER]
    }

    fn initialize(&mut self) -> Result<(), &'static str> {
        shell::init();
        Ok(())
    }

    fn start(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    fn stop(&mut self) {}

    fn health(&self) -> HealthState {
        HealthState::Healthy
    }
}

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
        manager.register(Box::new(ShellService));
        manager.start_all()
    })
}

pub fn list() -> Vec<ServiceSnapshot> {
    with_manager(|manager| manager.snapshots())
}

pub fn start(name: &str) -> Result<(), &'static str> {
    with_manager(|manager| manager.start_by_name(name))
}

pub fn stop(name: &str) -> Result<(), &'static str> {
    with_manager(|manager| manager.stop_by_name(name))
}

pub fn restart(name: &str) -> Result<(), &'static str> {
    with_manager(|manager| manager.restart_by_name(name))
}

pub fn health() -> Vec<(String, HealthState)> {
    with_manager(|manager| {
        manager
            .snapshots()
            .into_iter()
            .map(|s| (s.name, s.health))
            .collect()
    })
}

pub fn info(name: &str) -> Option<ServiceSnapshot> {
    with_manager(|manager| {
        manager
            .snapshots()
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    })
}
