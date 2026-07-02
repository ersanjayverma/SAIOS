use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::object_manager::{Health, ObjectStatus, ObjectType, Property, PropertyMap};
use crate::som::{ObjectId, ProviderId};
use crate::{pci, scheduler};
use crate::driver::storage;

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

#[derive(Clone)]
pub struct ProviderObject {
    pub path: String,
    pub name: String,
    pub object_type: ObjectType,
    pub status: ObjectStatus,
    pub health: Health,
    pub parent_path: Option<String>,
    pub properties: PropertyMap,
}

pub trait Provider {
    fn id(&self) -> ProviderId;
    fn name(&self) -> &str;
    fn provider_type(&self) -> ProviderType;
    fn namespace(&self) -> &str;

    fn initialize(&mut self) {}
    fn shutdown(&mut self) {}

    fn enumerate(&self) -> Vec<ProviderObject>;
    fn lookup(&self, _id: ObjectId) -> Option<ProviderObject> {
        None
    }
}

pub struct StorageProvider {
    id: ProviderId,
}

impl StorageProvider {
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

    fn enumerate(&self) -> Vec<ProviderObject> {
        let mut out = Vec::new();
        for volume in storage::volumes() {
            let (status, health) = if volume.mounted_at.is_some() {
                (ObjectStatus::Online, Health::Healthy)
            } else {
                (ObjectStatus::Offline, Health::Offline)
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

pub struct DeviceProvider {
    id: ProviderId,
}

impl DeviceProvider {
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

    fn initialize(&mut self) {
        pci::init();
    }

    fn enumerate(&self) -> Vec<ProviderObject> {
        let mut out = Vec::new();
        for (idx, dev) in pci::devices().into_iter().enumerate() {
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

pub struct ProcessProvider {
    id: ProviderId,
}

impl ProcessProvider {
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
