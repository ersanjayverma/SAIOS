//! Kernel provider subsystem.
//!
//! A provider exposes a slice of the kernel's object namespace as a collection
//! of [`ProviderObject`] entries. Each provider is identified by a
//! [`ProviderId`] and a namespace path (for example `/storage` or `/network`).
//! Providers are registered during KSF bootstrap and are queried by the shell
//! and object manager to present a unified view of hardware, services and
//! runtime state.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::driver::storage;
use crate::driver::{dhcp, ethernet, loopback, wifi};
use crate::kernel::device;
use crate::object_manager::{Health, ObjectStatus, ObjectType, Property, PropertyMap};
use crate::som::{ObjectId, ProviderId};
use crate::{pci, scheduler};

/// Broad category of a kernel provider.
///
/// The variant is exposed as a property on provider objects and is used by the
/// object manager to route queries to the correct subsystem.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProviderType {
    Core,
    Memory,
    Storage,
    Filesystem,
    Device,
    Driver,
    Process,
    Thread,
    Scheduler,
    Network,
    Security,
    User,
    Service,
    Event,
    Log,
    AI,
}

/// A single object exposed by a provider.
///
/// Provider objects are lightweight snapshots of kernel state. They carry a
/// human-readable path, type, status and a flat map of properties that the
/// shell and object manager can render without knowing provider-specific
/// details.
#[derive(Clone)]
pub struct ProviderObject {
    /// Namespace path of the object, relative to the provider root.
    pub path: String,
    /// Short display name of the object.
    pub name: String,
    /// SAIOS object type classification.
    pub object_type: ObjectType,
    /// Current operational status.
    pub status: ObjectStatus,
    /// Health assessment derived from status and subsystem state.
    pub health: Health,
    /// Path of the parent object, if any.
    pub parent_path: Option<String>,
    /// Key/value property bag exposed to user-space.
    pub properties: PropertyMap,
}

/// Interface implemented by every kernel provider.
///
/// Providers are the kernel's abstraction for browsing hardware and runtime
/// objects. The default implementations of [`Provider::initialize`],
/// [`Provider::shutdown`] and [`Provider::lookup`] are no-ops so simple
/// providers only need to implement identity and enumeration methods.
pub trait Provider {
    /// Returns the provider's unique identifier.
    fn id(&self) -> ProviderId;
    /// Returns the provider's short name (e.g. `"storage"`).
    fn name(&self) -> &str;
    /// Returns the provider's category.
    fn provider_type(&self) -> ProviderType;
    /// Returns the provider's namespace path (e.g. `"/storage"`).
    fn namespace(&self) -> &str;

    /// Called once when the provider is registered.
    ///
    /// The default implementation does nothing; providers that need to probe
    /// hardware or allocate state should override this.
    fn initialize(&mut self) {}
    /// Called when the provider is unregistered or the system shuts down.
    ///
    /// The default implementation does nothing.
    fn shutdown(&mut self) {}

    /// Returns all objects currently exposed by this provider.
    fn enumerate(&self) -> Vec<ProviderObject>;
    /// Looks up a single object by its object identifier.
    ///
    /// The default implementation returns `None`. Providers that maintain a
    /// stable object-id-to-object mapping should override this.
    fn lookup(&self, _id: ObjectId) -> Option<ProviderObject> {
        None
    }
}

/// Provider that enumerates detected storage volumes.
pub struct StorageProvider {
    id: ProviderId,
}

impl StorageProvider {
    /// Creates a new storage provider with the given identifier.
    pub const fn new(id: ProviderId) -> Self {
        Self { id }
    }
}

impl Provider for StorageProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn name(&self) -> &str {
        "storage"
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Storage
    }

    fn namespace(&self) -> &str {
        "/storage"
    }

    /// Enumerates all detected storage volumes, including the in-memory tmpfs
    /// root and any probed filesystem images.
    fn enumerate(&self) -> Vec<ProviderObject> {
        let mut out = Vec::new();
        let volumes = storage::volumes_cached();
        let disks = storage::disks_cached();

        if let Some(volume) = volumes.iter().find(|v| v.name == "tmpfs") {
            out.push(ProviderObject {
                path: "storage/tmpfs".to_string(),
                name: "tmpfs".to_string(),
                object_type: ObjectType::Volume,
                status: ObjectStatus::Online,
                health: Health::Healthy,
                parent_path: Some("storage".to_string()),
                properties: vec![
                    Property { key: "Driver".to_string(),     value: volume.filesystem.driver_name().to_string() },
                    Property { key: "Filesystem".to_string(), value: volume.filesystem.as_str().to_string() },
                    Property { key: "DriverMode".to_string(), value: volume.driver_mode.to_string() },
                    Property { key: "WriteMode".to_string(),  value: volume.write_mode.to_string() },
                    Property { key: "Backing".to_string(),    value: volume.backing.clone() },
                    Property { key: "Mounted".to_string(),    value: volume.mounted_at.clone().unwrap_or_else(|| "-".to_string()) },
                    Property { key: "Writable".to_string(),   value: if volume.writable { "true" } else { "false" }.to_string() },
                ],
            });
        }

        for disk in &disks {
            out.push(ProviderObject {
                path: format!("storage/{}", disk.name),
                name: disk.name.clone(),
                object_type: ObjectType::Device,
                status: ObjectStatus::Online,
                health: Health::Healthy,
                parent_path: Some("storage".to_string()),
                properties: vec![
                    Property { key: "Driver".to_string(),     value: if disk.hardware { "ahci" } else { "ramdisk" }.to_string() },
                    Property { key: "Backing".to_string(),    value: disk.backing.clone() },
                    Property { key: "SectorSize".to_string(), value: disk.sector_size.to_string() },
                    Property { key: "SizeBytes".to_string(),  value: disk.total_bytes.to_string() },
                    Property { key: "Partitions".to_string(), value: disk.partitions.len().to_string() },
                    Property { key: "Hardware".to_string(),   value: if disk.hardware { "true" } else { "false" }.to_string() },
                ],
            });

            for part_name in &disk.partitions {
                let Some(volume) = volumes.iter().find(|v| v.name.eq_ignore_ascii_case(part_name)) else {
                    continue;
                };

                let (status, health) = if volume.mounted_at.is_some() {
                    (ObjectStatus::Online, Health::Healthy)
                } else {
                    (ObjectStatus::Offline, Health::Warning)
                };

                out.push(ProviderObject {
                    path: format!("storage/{}/{}", disk.name, volume.name),
                    name: volume.name.clone(),
                    object_type: ObjectType::Volume,
                    status,
                    health,
                    parent_path: Some(format!("storage/{}", disk.name)),
                    properties: vec![
                        Property { key: "Driver".to_string(),     value: volume.filesystem.driver_name().to_string() },
                        Property { key: "Filesystem".to_string(), value: volume.filesystem.as_str().to_string() },
                        Property { key: "DriverMode".to_string(), value: volume.driver_mode.to_string() },
                        Property { key: "WriteMode".to_string(),  value: volume.write_mode.to_string() },
                        Property { key: "Backing".to_string(),    value: volume.backing.clone() },
                        Property { key: "SectorSize".to_string(), value: volume.sector_size.to_string() },
                        Property { key: "SizeBytes".to_string(),  value: volume.total_bytes.to_string() },
                        Property { key: "Mounted".to_string(),    value: volume.mounted_at.clone().unwrap_or_else(|| "-".to_string()) },
                        Property { key: "Writable".to_string(),   value: if volume.writable { "true" } else { "false" }.to_string() },
                    ],
                });
            }
        }

        if out.is_empty() {
            return out;
        }

        for volume in volumes {
            if volume.name == "tmpfs"
                || disks.iter().any(|d| d.name.eq_ignore_ascii_case(&volume.name))
            {
                continue;
            }
            if disks.iter().any(|d| {
                d.partitions.iter().any(|p| p.eq_ignore_ascii_case(&volume.name))
            }) {
                continue;
            }

            let (status, health) = if volume.mounted_at.is_some() {
                (ObjectStatus::Online, Health::Healthy)
            } else {
                (ObjectStatus::Offline, Health::Warning)
            };

            out.push(ProviderObject {
                path: format!("storage/{}", volume.name),
                name: volume.name,
                object_type: ObjectType::Volume,
                status,
                health,
                parent_path: Some("storage".to_string()),
                properties: vec![
                    Property {
                        key: "Driver".to_string(),
                        value: volume.filesystem.driver_name().to_string(),
                    },
                    Property {
                        key: "Filesystem".to_string(),
                        value: volume.filesystem.as_str().to_string(),
                    },
                    Property {
                        key: "Backing".to_string(),
                        value: volume.backing,
                    },
                    Property {
                        key: "SectorSize".to_string(),
                        value: volume.sector_size.to_string(),
                    },
                    Property {
                        key: "SizeBytes".to_string(),
                        value: volume.total_bytes.to_string(),
                    },
                    Property {
                        key: "Mounted".to_string(),
                        value: volume.mounted_at.unwrap_or_else(|| "-".to_string()),
                    },
                    Property {
                        key: "Writable".to_string(),
                        value: if volume.writable { "true" } else { "false" }.to_string(),
                    },
                ],
            });
        }

        out
    }
}

/// Provider that enumerates discovered hardware devices.
pub struct DeviceProvider {
    id: ProviderId,
}

impl DeviceProvider {
    /// Creates a new device provider with the given identifier.
    pub const fn new(id: ProviderId) -> Self {
        Self { id }
    }
}

impl Provider for DeviceProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn name(&self) -> &str {
        "devices"
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Device
    }

    fn namespace(&self) -> &str {
        "/devices"
    }

    /// Hardware probing is deferred until an explicit driver or shell command.
    fn initialize(&mut self) {}

    /// Enumerates PCI devices already known to the kernel without probing.
    fn enumerate(&self) -> Vec<ProviderObject> {
        let mut out = Vec::new();

        for dev in device::devices() {
            let (status, health) = match dev.status {
                device::DeviceStatus::Online => (ObjectStatus::Online, Health::Healthy),
                device::DeviceStatus::Offline => (ObjectStatus::Offline, Health::Warning),
                device::DeviceStatus::Faulted => (ObjectStatus::Faulted, Health::Critical),
            };

            let short_name = dev
                .name
                .trim_start_matches('/')
                .trim_start_matches("dev/")
                .to_string();
            let path_name = if short_name.is_empty() {
                dev.name.trim_start_matches('/').replace('/', "_")
            } else {
                short_name.replace('/', "_")
            };

            out.push(ProviderObject {
                path: format!("devices/{}", path_name),
                name: dev.name.clone(),
                object_type: ObjectType::Device,
                status,
                health,
                parent_path: Some("devices".to_string()),
                properties: vec![
                    Property {
                        key: "Driver".to_string(),
                        value: dev.driver,
                    },
                    Property {
                        key: "Class".to_string(),
                        value: dev.class,
                    },
                    Property {
                        key: "ObjectId".to_string(),
                        value: dev.object_id.0.to_string(),
                    },
                ],
            });
        }

        for (idx, dev) in pci::devices_snapshot().into_iter().enumerate() {
            out.push(ProviderObject {
                path: format!("devices/pci{}", idx),
                name: format!("pci{}", idx),
                object_type: ObjectType::Device,
                status: ObjectStatus::Online,
                health: Health::Healthy,
                parent_path: Some("devices".to_string()),
                properties: vec![
                    Property {
                        key: "Vendor".to_string(),
                        value: format!("0x{:04x}", dev.vendor_id),
                    },
                    Property {
                        key: "Device".to_string(),
                        value: format!("0x{:04x}", dev.device_id),
                    },
                    Property {
                        key: "Class".to_string(),
                        value: pci::class_name(dev.class).to_string(),
                    },
                    Property {
                        key: "Location".to_string(),
                        value: format!("{:02x}:{:02x}.{}", dev.bus, dev.device, dev.function),
                    },
                ],
            });
        }
        out
    }
}

/// Provider that enumerates running threads/processes.
pub struct ProcessProvider {
    id: ProviderId,
}

impl ProcessProvider {
    /// Creates a new process provider with the given identifier.
    pub const fn new(id: ProviderId) -> Self {
        Self { id }
    }
}

impl Provider for ProcessProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn name(&self) -> &str {
        "processes"
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Process
    }

    fn namespace(&self) -> &str {
        "/processes"
    }

    /// Enumerates scheduler threads as process objects.
    fn enumerate(&self) -> Vec<ProviderObject> {
        let mut out = Vec::new();
        for thread in scheduler::threads() {
            let (status, health) = match thread.state {
                scheduler::ThreadState::Dead => (ObjectStatus::Offline, Health::Critical),
                scheduler::ThreadState::Blocked | scheduler::ThreadState::Sleeping => {
                    (ObjectStatus::Busy, Health::Warning)
                }
                _ => (ObjectStatus::Online, Health::Healthy),
            };

            out.push(ProviderObject {
                path: format!("processes/{}", thread.id),
                name: format!("{}", thread.id),
                object_type: ObjectType::Process,
                status,
                health,
                parent_path: Some("processes".to_string()),
                properties: vec![Property {
                    key: "State".to_string(),
                    value: format!("{:?}", thread.state),
                }],
            });
        }
        out
    }
}

/// Provider that enumerates network interfaces.
pub struct NetworkProvider {
    id: ProviderId,
}

impl NetworkProvider {
    /// Creates a new network provider with the given identifier.
    pub const fn new(id: ProviderId) -> Self {
        Self { id }
    }
}

impl Provider for NetworkProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn name(&self) -> &str {
        "network"
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Network
    }

    fn namespace(&self) -> &str {
        "/network"
    }

    /// Enumerates loopback, ethernet and wireless network interfaces.
    fn enumerate(&self) -> Vec<ProviderObject> {
        let mut out = Vec::new();

        for iface in loopback::interfaces() {
            out.push(ProviderObject {
                path: format!("network/{}", iface.name),
                name: iface.name,
                object_type: ObjectType::NetworkInterface,
                status: ObjectStatus::Online,
                health: Health::Healthy,
                parent_path: Some("network".to_string()),
                properties: vec![
                    Property {
                        key: "Driver".to_string(),
                        value: "loopback".to_string(),
                    },
                    Property {
                        key: "Type".to_string(),
                        value: "loopback".to_string(),
                    },
                    Property {
                        key: "IPv4".to_string(),
                        value: iface.ipv4,
                    },
                    Property {
                        key: "Netmask".to_string(),
                        value: iface.netmask,
                    },
                ],
            });
        }

        for iface in ethernet::interfaces() {
            let lease = dhcp::lease_for(iface.name.as_str());
            out.push(ProviderObject {
                path: format!("network/{}", iface.name),
                name: iface.name,
                object_type: ObjectType::NetworkInterface,
                status: if iface.link_up {
                    ObjectStatus::Online
                } else {
                    ObjectStatus::Offline
                },
                health: if iface.link_up {
                    Health::Healthy
                } else {
                    Health::Warning
                },
                parent_path: Some("network".to_string()),
                properties: vec![
                    Property {
                        key: "Driver".to_string(),
                        value: "ethernet".to_string(),
                    },
                    Property {
                        key: "Type".to_string(),
                        value: "ethernet".to_string(),
                    },
                    Property {
                        key: "Backing".to_string(),
                        value: iface.backing,
                    },
                    Property {
                        key: "MAC".to_string(),
                        value: format!(
                            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                            iface.mac[0],
                            iface.mac[1],
                            iface.mac[2],
                            iface.mac[3],
                            iface.mac[4],
                            iface.mac[5]
                        ),
                    },
                    Property {
                        key: "Link".to_string(),
                        value: if iface.link_up { "up" } else { "down" }.to_string(),
                    },
                    Property {
                        key: "SpeedMbps".to_string(),
                        value: iface.speed_mbps.to_string(),
                    },
                    Property {
                        key: "IPv4".to_string(),
                        value: iface.ipv4.unwrap_or_else(|| "-".to_string()),
                    },
                    Property {
                        key: "Gateway".to_string(),
                        value: lease
                            .as_ref()
                            .map(|l| l.gateway.clone())
                            .unwrap_or_else(|| "-".to_string()),
                    },
                ],
            });
        }

        for iface in wifi::interfaces() {
            let lease = dhcp::lease_for(iface.name.as_str());
            out.push(ProviderObject {
                path: format!("network/{}", iface.name),
                name: iface.name,
                object_type: ObjectType::NetworkInterface,
                status: if iface.connected {
                    ObjectStatus::Online
                } else {
                    ObjectStatus::Offline
                },
                health: if iface.connected {
                    Health::Healthy
                } else {
                    Health::Warning
                },
                parent_path: Some("network".to_string()),
                properties: vec![
                    Property {
                        key: "Driver".to_string(),
                        value: "wifi".to_string(),
                    },
                    Property {
                        key: "Type".to_string(),
                        value: "wifi".to_string(),
                    },
                    Property {
                        key: "Backing".to_string(),
                        value: iface.backing,
                    },
                    Property {
                        key: "SSID".to_string(),
                        value: iface.ssid.unwrap_or_else(|| "-".to_string()),
                    },
                    Property {
                        key: "SignalDbm".to_string(),
                        value: iface.signal_dbm.to_string(),
                    },
                    Property {
                        key: "MAC".to_string(),
                        value: format!(
                            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                            iface.mac[0],
                            iface.mac[1],
                            iface.mac[2],
                            iface.mac[3],
                            iface.mac[4],
                            iface.mac[5]
                        ),
                    },
                    Property {
                        key: "IPv4".to_string(),
                        value: iface.ipv4.unwrap_or_else(|| "-".to_string()),
                    },
                    Property {
                        key: "Gateway".to_string(),
                        value: lease
                            .as_ref()
                            .map(|l| l.gateway.clone())
                            .unwrap_or_else(|| "-".to_string()),
                    },
                ],
            });
        }

        out
    }
}
