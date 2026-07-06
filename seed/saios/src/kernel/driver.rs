use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;

use hal::arch::x86_64::sync::StaticCell;

use crate::kernel::device;
use crate::kernel::event::{self, EventKind};
use crate::kernel::object as kom;
use crate::pci;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DriverStatus {
    Loaded,
    Running,
    Stopped,
    Faulted,
}

#[derive(Clone, Debug)]
pub struct DriverRecord {
    pub name: String,
    pub version: String,
    pub author: String,
    pub status: DriverStatus,
    pub dependencies: Vec<String>,
    pub devices: Vec<String>,
    pub object_id: kom::ObjectId,
    pub start_count: u64,
    pub stop_count: u64,
    pub reload_count: u64,
    pub fault_count: u64,
    pub last_error: Option<String>,
}

struct DriverRegistry {
    initialized: bool,
    records: Vec<DriverRecord>,
}

impl DriverRegistry {
    fn new() -> Self {
        Self {
            initialized: false,
            records: Vec::new(),
        }
    }

    fn ensure_driver(
        &mut self,
        name: &str,
        version: &str,
        author: &str,
        dependencies: &[&str],
        status: DriverStatus,
    ) -> Result<kom::ObjectHandle, &'static str> {
        if name.is_empty() || version.is_empty() || author.is_empty() {
            return Err("driver: name/version/author must be non-empty");
        }

        if let Some(existing) = self.records.iter_mut().find(|r| r.name == name) {
            existing.version = version.to_string();
            existing.author = author.to_string();
            existing.status = status;
            existing.dependencies = dependencies.iter().map(|s| s.to_string()).collect();
            return Ok(kom::ObjectHandle::new(existing.object_id));
        }

        let state = match status {
            DriverStatus::Loaded | DriverStatus::Running => kom::ObjectState::Ready,
            DriverStatus::Stopped => kom::ObjectState::Stopping,
            DriverStatus::Faulted => kom::ObjectState::Stopping,
        };

        let handle = kom::register(kom::ObjectType::Driver, name, state)?;
        self.records.push(DriverRecord {
            name: name.to_string(),
            version: version.to_string(),
            author: author.to_string(),
            status,
            dependencies: dependencies.iter().map(|s| s.to_string()).collect(),
            devices: Vec::new(),
            object_id: handle.id(),
            start_count: 0,
            stop_count: 0,
            reload_count: 0,
            fault_count: 0,
            last_error: None,
        });
        event::publish(EventKind::DriverLoaded, "driver-manager", name);
        Ok(handle)
    }

    fn status_by_name(&self, name: &str) -> Option<DriverStatus> {
        self.records
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(name))
            .map(|r| r.status)
    }

    fn dependencies_ready(&self, deps: &[String]) -> bool {
        deps.iter().all(|dep| {
            self.status_by_name(dep)
                .is_some_and(|s| matches!(s, DriverStatus::Running | DriverStatus::Loaded))
        })
    }

    fn refresh_devices(&mut self) {
        for r in &mut self.records {
            r.devices.clear();
        }

        for dev in device::devices() {
            if let Some(driver) = self.records.iter_mut().find(|r| r.name == dev.driver) {
                driver.devices.push(dev.name.clone());
            }
        }
    }
}

static REGISTRY: StaticCell<Option<DriverRegistry>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    hal::arch::x86_64::sync::spinlock_acquire(&LOCK);
}

fn unlock() {
    hal::arch::x86_64::sync::spinlock_release(&LOCK);
}

fn with_registry_mut<R>(f: impl FnOnce(&mut DriverRegistry) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *REGISTRY.get() };
    if slot.is_none() {
        *slot = Some(DriverRegistry::new());
    }
    let out = f(slot.as_mut().expect("driver registry unavailable"));
    unlock();
    out
}

fn with_registry<R>(f: impl FnOnce(&DriverRegistry) -> R) -> R {
    lock();
    // SAFETY: global singleton guarded by spin lock.
    let slot = unsafe { &mut *REGISTRY.get() };
    if slot.is_none() {
        *slot = Some(DriverRegistry::new());
    }
    let out = f(slot.as_ref().expect("driver registry unavailable"));
    unlock();
    out
}

fn run_start_hook(name: &str) -> Result<(), &'static str> {
    if name.eq_ignore_ascii_case("serial") {
        hal::arch::x86_64::console::init_serial();
        Ok(())
    } else if name.eq_ignore_ascii_case("pci") {
        pci::init();
        Ok(())
    } else if name.eq_ignore_ascii_case("network") {
        crate::driver::network::init();
        crate::driver::loopback::init();
        crate::driver::ethernet::init();
        crate::driver::wifi::init();
        crate::driver::dns::init();
        let _ = crate::driver::network::bind_nic();
        let _ = device::ensure_device(
            "lo",
            "loopback",
            "network/loopback",
            device::DeviceStatus::Online,
        );
        Ok(())
    } else if name.eq_ignore_ascii_case("loopback") {
        crate::driver::loopback::init();
        let _ = device::ensure_device(
            "lo",
            "loopback",
            "network/loopback",
            device::DeviceStatus::Online,
        );
        Ok(())
    } else if name.eq_ignore_ascii_case("ethernet") {
        crate::driver::ethernet::rescan();
        let _ = crate::driver::network::bind_nic();
        let interfaces = crate::driver::ethernet::interfaces();
        for iface in interfaces {
            let _ = device::ensure_device(
                iface.name.as_str(),
                "ethernet",
                "network/ethernet",
                if iface.link_up {
                    device::DeviceStatus::Online
                } else {
                    device::DeviceStatus::Offline
                },
            );
        }
        Ok(())
    } else if name.eq_ignore_ascii_case("wifi") {
        crate::driver::wifi::rescan();
        let _ = crate::driver::network::bind_nic();
        let interfaces = crate::driver::wifi::interfaces();
        for iface in interfaces {
            let _ = device::ensure_device(
                iface.name.as_str(),
                "wifi",
                "network/wifi",
                if iface.connected {
                    device::DeviceStatus::Online
                } else {
                    device::DeviceStatus::Offline
                },
            );
        }
        Ok(())
    } else if name.eq_ignore_ascii_case("dhcp") {
        crate::driver::dhcp::renew_all();
        let _ = crate::driver::network::apply_dhcp();
        Ok(())
    } else if name.eq_ignore_ascii_case("dns") {
        crate::driver::dns::init();
        Ok(())
    } else if name.eq_ignore_ascii_case("usb") {
        crate::driver::usb::rescan();
        let controllers = crate::driver::usb::controllers();
        for controller in controllers {
            let _ = device::ensure_device(
                controller.name.as_str(),
                "usb",
                "bus/usb-host",
                if matches!(
                    controller.state,
                    crate::driver::usb::UsbControllerState::Faulted
                ) {
                    device::DeviceStatus::Faulted
                } else {
                    device::DeviceStatus::Online
                },
            );
        }
        Ok(())
    } else if name.eq_ignore_ascii_case("ahci") {
        crate::driver::ahci::rescan();
        let controllers = crate::driver::ahci::controllers();
        for controller in controllers {
            let _ = device::ensure_device(
                controller.name.as_str(),
                "ahci",
                "block/ahci-host",
                if matches!(
                    controller.state,
                    crate::driver::ahci::AhciControllerState::Faulted
                ) {
                    device::DeviceStatus::Faulted
                } else {
                    device::DeviceStatus::Online
                },
            );
        }
        Ok(())
    } else if name.eq_ignore_ascii_case("storage")
        || name.eq_ignore_ascii_case("ext4")
        || name.eq_ignore_ascii_case("ntfs")
        || name.eq_ignore_ascii_case("fat16")
        || name.eq_ignore_ascii_case("fat32")
        || name.eq_ignore_ascii_case("fat64")
        || name.eq_ignore_ascii_case("fat128")
    {
        crate::driver::storage::rescan();
        Ok(())
    } else {
        Ok(())
    }
}

fn run_stop_hook(_name: &str) -> Result<(), &'static str> {
    if _name.eq_ignore_ascii_case("dhcp") {
        crate::driver::dhcp::clear();
    }
    Ok(())
}

fn run_reload_hook(name: &str) -> Result<(), &'static str> {
    if name.eq_ignore_ascii_case("serial") {
        hal::arch::x86_64::console::init_serial();
        Ok(())
    } else if name.eq_ignore_ascii_case("pci") {
        pci::init();
        Ok(())
    } else if name.eq_ignore_ascii_case("network")
        || name.eq_ignore_ascii_case("loopback")
        || name.eq_ignore_ascii_case("ethernet")
        || name.eq_ignore_ascii_case("wifi")
        || name.eq_ignore_ascii_case("dhcp")
        || name.eq_ignore_ascii_case("dns")
        || name.eq_ignore_ascii_case("usb")
    {
        run_start_hook(name)
    } else if name.eq_ignore_ascii_case("storage")
        || name.eq_ignore_ascii_case("ext4")
        || name.eq_ignore_ascii_case("ntfs")
        || name.eq_ignore_ascii_case("fat16")
        || name.eq_ignore_ascii_case("fat32")
        || name.eq_ignore_ascii_case("fat64")
        || name.eq_ignore_ascii_case("fat128")
    {
        crate::driver::storage::rescan();
        Ok(())
    } else {
        Ok(())
    }
}

pub fn init() {
    with_registry_mut(|r| {
        if r.initialized {
            return;
        }

        // Drivers that have no early runtime registration site yet.
        let _ = r.ensure_driver("pci", "0.1.0", "SAIOS", &[], DriverStatus::Loaded);
        let _ = r.ensure_driver("network", "0.1.0", "SAIOS", &["pci"], DriverStatus::Loaded);
        let _ = r.ensure_driver("usb", "0.1.0", "SAIOS", &["pci"], DriverStatus::Loaded);
        let _ = r.ensure_driver(
            "loopback",
            "0.1.0",
            "SAIOS",
            &["network"],
            DriverStatus::Loaded,
        );
        let _ = r.ensure_driver(
            "ethernet",
            "0.1.0",
            "SAIOS",
            &["network", "pci"],
            DriverStatus::Loaded,
        );
        let _ = r.ensure_driver(
            "wifi",
            "0.1.0",
            "SAIOS",
            &["network", "pci"],
            DriverStatus::Loaded,
        );
        let _ = r.ensure_driver("dhcp", "0.1.0", "SAIOS", &["network"], DriverStatus::Loaded);
        let _ = r.ensure_driver(
            "dns",
            "0.1.0",
            "SAIOS",
            &["network", "dhcp"],
            DriverStatus::Loaded,
        );
        let _ = r.ensure_driver("storage", "0.1.0", "SAIOS", &["pci"], DriverStatus::Loaded);
        let _ = r.ensure_driver("ext4", "0.1.0", "SAIOS", &["storage"], DriverStatus::Loaded);
        let _ = r.ensure_driver("ntfs", "0.1.0", "SAIOS", &["storage"], DriverStatus::Loaded);
        let _ = r.ensure_driver(
            "fat16",
            "0.1.0",
            "SAIOS",
            &["storage"],
            DriverStatus::Loaded,
        );
        let _ = r.ensure_driver(
            "fat32",
            "0.1.0",
            "SAIOS",
            &["storage"],
            DriverStatus::Loaded,
        );
        let _ = r.ensure_driver(
            "fat64",
            "0.1.0",
            "SAIOS",
            &["storage"],
            DriverStatus::Loaded,
        );
        let _ = r.ensure_driver(
            "fat128",
            "0.1.0",
            "SAIOS",
            &["storage"],
            DriverStatus::Loaded,
        );

        r.refresh_devices();
        r.initialized = true;
    });
}

pub fn ensure_driver(
    name: &str,
    version: &str,
    author: &str,
    dependencies: &[&str],
    status: DriverStatus,
) -> Result<kom::ObjectHandle, &'static str> {
    with_registry_mut(|r| r.ensure_driver(name, version, author, dependencies, status))
}

pub fn drivers() -> Vec<DriverRecord> {
    with_registry_mut(|r| {
        r.refresh_devices();
        r.records.clone()
    })
}

pub fn find(name: &str) -> Option<DriverRecord> {
    with_registry_mut(|r| {
        r.refresh_devices();
        r.records.iter().find(|d| d.name == name).cloned()
    })
}

pub fn reload(name: &str) -> Result<(), &'static str> {
    with_registry_mut(|r| {
        let driver = r
            .records
            .iter_mut()
            .find(|d| d.name.eq_ignore_ascii_case(name))
            .ok_or("driver: not found")?;

        if let Err(e) = run_reload_hook(driver.name.as_str()) {
            driver.status = DriverStatus::Faulted;
            driver.fault_count = driver.fault_count.saturating_add(1);
            driver.last_error = Some(e.to_string());
            event::publish(EventKind::DriverFaulted, "driver-manager", e);
            return Err(e);
        }
        driver.status = DriverStatus::Running;
        driver.reload_count = driver.reload_count.saturating_add(1);
        driver.last_error = None;
        event::publish(
            EventKind::DriverReloaded,
            "driver-manager",
            driver.name.as_str(),
        );
        r.refresh_devices();
        Ok(())
    })
}

pub fn start(name: &str) -> Result<(), &'static str> {
    with_registry_mut(|r| {
        let idx = r
            .records
            .iter()
            .position(|d| d.name.eq_ignore_ascii_case(name))
            .ok_or("driver: not found")?;

        let deps = r.records[idx].dependencies.clone();
        if !r.dependencies_ready(&deps) {
            r.records[idx].status = DriverStatus::Faulted;
            r.records[idx].fault_count = r.records[idx].fault_count.saturating_add(1);
            r.records[idx].last_error = Some("dependencies not ready".to_string());
            event::publish(
                EventKind::DriverFaulted,
                "driver-manager",
                "dependencies not ready",
            );
            return Err("driver: dependencies not ready");
        }

        let driver_name = r.records[idx].name.clone();
        if let Err(e) = run_start_hook(driver_name.as_str()) {
            r.records[idx].status = DriverStatus::Faulted;
            r.records[idx].fault_count = r.records[idx].fault_count.saturating_add(1);
            r.records[idx].last_error = Some(e.to_string());
            event::publish(EventKind::DriverFaulted, "driver-manager", e);
            return Err(e);
        }
        r.records[idx].status = DriverStatus::Running;
        r.records[idx].start_count = r.records[idx].start_count.saturating_add(1);
        r.records[idx].last_error = None;
        r.refresh_devices();
        Ok(())
    })
}

pub fn stop(name: &str) -> Result<(), &'static str> {
    with_registry_mut(|r| {
        let idx = r
            .records
            .iter()
            .position(|d| d.name.eq_ignore_ascii_case(name))
            .ok_or("driver: not found")?;

        let driver_name = r.records[idx].name.clone();
        if let Err(e) = run_stop_hook(driver_name.as_str()) {
            r.records[idx].status = DriverStatus::Faulted;
            r.records[idx].fault_count = r.records[idx].fault_count.saturating_add(1);
            r.records[idx].last_error = Some(e.to_string());
            return Err(e);
        }
        r.records[idx].status = DriverStatus::Stopped;
        r.records[idx].stop_count = r.records[idx].stop_count.saturating_add(1);
        r.records[idx].last_error = None;
        r.refresh_devices();
        Ok(())
    })
}

pub fn count() -> usize {
    with_registry(|r| r.records.len())
}
