use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::kernel::event::{self, EventKind};
use crate::kernel::object as kom;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DeviceStatus {
    Online,
    Offline,
    Faulted,
}

#[derive(Clone, Debug)]
pub struct DeviceRecord {
    pub name: String,
    pub driver: String,
    pub class: String,
    pub status: DeviceStatus,
    pub object_id: kom::ObjectId,
}

struct DeviceRegistry {
    initialized: bool,
    records: Vec<DeviceRecord>,
}

impl DeviceRegistry {
    fn new() -> Self {
        Self {
            initialized: false,
            records: Vec::new(),
        }
    }

    fn register(
        &mut self,
        name: &str,
        driver: &str,
        class: &str,
        status: DeviceStatus,
    ) -> Result<kom::ObjectHandle, &'static str> {
        if name.is_empty() || driver.is_empty() || class.is_empty() {
            return Err("device: name/driver/class must be non-empty");
        }

        if self.records.iter().any(|r| r.name == name) {
            return Err("device: already exists");
        }

        let state = match status {
            DeviceStatus::Online => kom::ObjectState::Ready,
            DeviceStatus::Offline => kom::ObjectState::Stopping,
            DeviceStatus::Faulted => kom::ObjectState::Stopping,
        };

        let handle = kom::register(kom::ObjectType::Device, name, state)?;
        self.records.push(DeviceRecord {
            name: name.to_string(),
            driver: driver.to_string(),
            class: class.to_string(),
            status,
            object_id: handle.id(),
        });
        event::publish(
            EventKind::DeviceAttached,
            "device-manager",
            alloc::format!("{} via {}", name, driver).as_str(),
        );
        Ok(handle)
    }

    fn ensure(
        &mut self,
        name: &str,
        driver: &str,
        class: &str,
        status: DeviceStatus,
    ) -> Result<kom::ObjectHandle, &'static str> {
        if let Some(existing) = self.records.iter_mut().find(|r| r.name == name) {
            existing.driver = driver.to_string();
            existing.class = class.to_string();
            existing.status = status;
            return Ok(kom::ObjectHandle::new(existing.object_id));
        }

        self.register(name, driver, class, status)
    }
}

static REGISTRY: StaticCell<Option<DeviceRegistry>> = StaticCell::new(None);
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

fn with_registry_mut<R>(f: impl FnOnce(&mut DeviceRegistry) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *REGISTRY.get() };
    if slot.is_none() {
        *slot = Some(DeviceRegistry::new());
    }
    let out = f(slot.as_mut().expect("device registry unavailable"));
    unlock();
    out
}

fn with_registry<R>(f: impl FnOnce(&DeviceRegistry) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *REGISTRY.get() };
    if slot.is_none() {
        *slot = Some(DeviceRegistry::new());
    }
    let out = f(slot.as_ref().expect("device registry unavailable"));
    unlock();
    out
}

pub fn init() {
    with_registry_mut(|r| {
        if r.initialized {
            return;
        }
        r.initialized = true;
    });
}

pub fn register_device(
    name: &str,
    driver: &str,
    class: &str,
    status: DeviceStatus,
) -> Result<kom::ObjectHandle, &'static str> {
    with_registry_mut(|r| r.register(name, driver, class, status))
}

pub fn ensure_device(
    name: &str,
    driver: &str,
    class: &str,
    status: DeviceStatus,
) -> Result<kom::ObjectHandle, &'static str> {
    with_registry_mut(|r| r.ensure(name, driver, class, status))
}

pub fn devices() -> Vec<DeviceRecord> {
    with_registry(|r| r.records.clone())
}

pub fn find(name: &str) -> Option<DeviceRecord> {
    with_registry(|r| r.records.iter().find(|d| d.name == name).cloned())
}
