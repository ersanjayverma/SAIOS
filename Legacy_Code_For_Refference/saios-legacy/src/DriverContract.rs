//! Canonical driver lifecycle authority.

use spin::Mutex;

const DEVICE_REGISTRY_CAPACITY: usize = 64;
const DEVICE_RESOURCE_SLOTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverState {
    New,
    Initialized,
    Started,
    Suspended,
    Stopped,
}

pub trait DriverLifecycle {
    fn init(&mut self) -> Result<(), &'static str>;
    fn start(&mut self) -> Result<(), &'static str>;
    fn stop(&mut self) -> Result<(), &'static str>;
    fn suspend(&mut self) -> Result<(), &'static str>;
    fn resume(&mut self) -> Result<(), &'static str>;
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    Network = 1,
    Storage = 2,
    Graphics = 3,
    Audio = 4,
    Hid = 5,
    Serial = 6,
    Generic = 7,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceBusType {
    Pci = 1,
    Usb = 2,
    I2c = 3,
    Spi = 4,
    Platform = 5,
    Virtual = 6,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Absent = 1,
    Present = 2,
    Claimed = 3,
    Active = 4,
    Suspended = 5,
    Faulted = 6,
    Removed = 7,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePowerState {
    D0 = 1,
    D1 = 2,
    D2 = 3,
    D3Hot = 4,
    D3Cold = 5,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceResourceKind {
    Irq = 1,
    Mmio = 2,
    IoPort = 3,
    DmaRange = 4,
    ReservedMemory = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceResource {
    pub kind: DeviceResourceKind,
    pub base: u64,
    pub length: u64,
    pub flags: u64,
}

impl DeviceResource {
    pub const EMPTY: Self = Self {
        kind: DeviceResourceKind::ReservedMemory,
        base: 0,
        length: 0,
        flags: 0,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceDescriptor {
    pub device_id: u64,
    pub class: DeviceClass,
    pub bus_type: DeviceBusType,
    pub bus_address: u64,
    pub parent_device_id: u64,
    pub name: &'static str,
    pub model: &'static str,
    pub firmware_version: &'static str,
    pub resources: [DeviceResource; DEVICE_RESOURCE_SLOTS],
    pub power_state: DevicePowerState,
    pub driver_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceRegistryRecord {
    pub descriptor: DeviceDescriptor,
    pub state: DeviceState,
    pub telemetry_handle: u64,
    pub active: bool,
}

impl DeviceRegistryRecord {
    const EMPTY: Self = Self {
        descriptor: DeviceDescriptor {
            device_id: 0,
            class: DeviceClass::Generic,
            bus_type: DeviceBusType::Platform,
            bus_address: 0,
            parent_device_id: 0,
            name: "",
            model: "",
            firmware_version: "",
            resources: [DeviceResource::EMPTY; DEVICE_RESOURCE_SLOTS],
            power_state: DevicePowerState::D0,
            driver_id: 0,
        },
        state: DeviceState::Absent,
        telemetry_handle: 0,
        active: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverCapabilityView {
    pub driver_lifecycle: bool,
    pub device_registry: bool,
    pub duplicate_device_rejection: bool,
    pub resource_attribution: bool,
    pub resource_overlap_arbitration: bool,
    pub iommu_dma_protection: bool,
    pub power_state_management: bool,
    pub telemetry_handle_preallocation: bool,
    pub hotplug_coordination: bool,
    pub numa_irq_affinity: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverDiagnosticView {
    pub registered_devices: usize,
    pub registry_capacity: usize,
    pub resource_accounting_events: u64,
}

static DEVICE_REGISTRY: Mutex<[DeviceRegistryRecord; DEVICE_REGISTRY_CAPACITY]> =
    Mutex::new([DeviceRegistryRecord::EMPTY; DEVICE_REGISTRY_CAPACITY]);

pub struct DriverContract;

impl DriverContract {
    fn emit_driver_event(
        reason: &'static str,
        outcome: crate::observability_contract::ObservationOutcome,
        evidence: [u64; 4],
    ) {
        crate::observability_contract::ObservabilityContract::emit_as_kds_event(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::Driver,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason,
                outcome,
                resource: crate::observability_contract::ResourceClass::Driver,
                owner: crate::observability_contract::ResourceOwner::Unknown,
                cpu: Some(crate::process::table::cpu_idx()),
                pid: crate::process::current_pid(),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence,
            },
            match outcome {
                crate::observability_contract::ObservationOutcome::Failed => {
                    crate::kds::KdsEventType::CompatibilityFailure
                }
                _ => crate::kds::KdsEventType::State,
            },
            match outcome {
                crate::observability_contract::ObservationOutcome::Failed => {
                    crate::kds::KdsSeverity::Error
                }
                _ => crate::kds::KdsSeverity::Info,
            },
        );
    }

    pub fn record_register(driver_id: u64, resource_id: u64) {
        Self::emit_driver_event(
            "driver.register",
            crate::observability_contract::ObservationOutcome::Success,
            [driver_id, resource_id, 0, 0],
        );
    }

    pub fn record_resource(driver_id: u64, resource_id: u64, flags: u64) {
        let _ = crate::resource_contract::ResourceContract::charge(
            crate::resource_contract::AttributionChain {
                accountable: crate::resource_contract::AccountableEntity {
                    kind: crate::resource_contract::AccountableEntityKind::Service,
                    id: driver_id,
                },
                acting_pid: crate::process::current_pid(),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence_event_id: 0,
            },
            crate::resource_contract::ResourceKind::DriverResources,
            1,
        );
        Self::emit_driver_event(
            "driver.resource",
            crate::observability_contract::ObservationOutcome::Success,
            [driver_id, resource_id, flags, 0],
        );
    }

    pub fn record_failure(driver_id: u64, code: u64, evidence: u64) {
        Self::emit_driver_event(
            "driver.failure",
            crate::observability_contract::ObservationOutcome::Failed,
            [driver_id, code, evidence, 0],
        );
    }

    pub fn capability_view() -> DriverCapabilityView {
        DriverCapabilityView {
            driver_lifecycle: true,
            device_registry: true,
            duplicate_device_rejection: true,
            resource_attribution: true,
            resource_overlap_arbitration: false,
            iommu_dma_protection: false,
            power_state_management: false,
            telemetry_handle_preallocation: false,
            hotplug_coordination: false,
            numa_irq_affinity: false,
        }
    }

    pub fn diagnostic_view() -> DriverDiagnosticView {
        DriverDiagnosticView {
            registered_devices: Self::registered_device_count(),
            registry_capacity: DEVICE_REGISTRY_CAPACITY,
            resource_accounting_events: crate::kds::count_events(
                crate::kds::KdsEventType::ResourceAccountPeriod,
            ),
        }
    }

    pub fn register_device(descriptor: DeviceDescriptor) -> Result<u64, &'static str> {
        if descriptor.device_id == 0 {
            Self::emit_driver_event(
                "device.registration.failed",
                crate::observability_contract::ObservationOutcome::Failed,
                [
                    descriptor.device_id,
                    descriptor.class as u64,
                    descriptor.bus_type as u64,
                    1,
                ],
            );
            return Err("device: id zero is reserved");
        }

        let mut registry = DEVICE_REGISTRY.lock();
        if registry
            .iter()
            .any(|record| record.active && record.descriptor.device_id == descriptor.device_id)
        {
            Self::emit_driver_event(
                "device.registration.failed",
                crate::observability_contract::ObservationOutcome::Failed,
                [
                    descriptor.device_id,
                    descriptor.class as u64,
                    descriptor.bus_type as u64,
                    2,
                ],
            );
            return Err("device: duplicate id");
        }

        let Some(slot) = registry.iter_mut().find(|record| !record.active) else {
            Self::emit_driver_event(
                "device.registration.failed",
                crate::observability_contract::ObservationOutcome::Failed,
                [
                    descriptor.device_id,
                    descriptor.class as u64,
                    descriptor.bus_type as u64,
                    3,
                ],
            );
            return Err("device: registry full");
        };

        *slot = DeviceRegistryRecord {
            descriptor,
            state: DeviceState::Present,
            telemetry_handle: descriptor.device_id,
            active: true,
        };
        drop(registry);

        Self::emit_driver_event(
            "device.registered",
            crate::observability_contract::ObservationOutcome::Success,
            [
                descriptor.device_id,
                descriptor.class as u64,
                descriptor.bus_type as u64,
                descriptor.bus_address,
            ],
        );
        Ok(descriptor.device_id)
    }

    pub fn registered_device_count() -> usize {
        DEVICE_REGISTRY
            .lock()
            .iter()
            .filter(|record| record.active)
            .count()
    }

    pub fn transition(from: DriverState, to: DriverState) -> Result<DriverState, &'static str> {
        use DriverState::*;

        match (from, to) {
            (New, Initialized)
            | (Initialized, Started)
            | (Started, Suspended)
            | (Suspended, Started)
            | (Suspended, Stopped)
            | (Started, Stopped)
            | (Stopped, Initialized) => Ok(to),
            _ if from == to => Ok(to),
            _ => Err("driver: invalid lifecycle transition"),
        }
    }

    pub fn transition_or_panic(
        from: DriverState,
        to: DriverState,
        tag: &'static str,
    ) -> DriverState {
        match Self::transition(from, to) {
            Ok(next) => {
                Self::emit_driver_event(
                    driver_transition_name(from, next),
                    crate::observability_contract::ObservationOutcome::Success,
                    [from as u64, next as u64, tag.as_ptr() as u64, 0],
                );
                next
            }
            Err(reason) => {
                Self::emit_driver_event(
                    "driver.failure",
                    crate::observability_contract::ObservationOutcome::Failed,
                    [from as u64, to as u64, tag.as_ptr() as u64, 0],
                );
                crate::observability_contract::ObservabilityContract::contract_violation(
                    crate::observability_contract::ContractOwner::Driver,
                    tag,
                    reason,
                    crate::observability_contract::ResourceClass::Driver,
                    crate::observability_contract::ResourceOwner::Unknown,
                    [from as u64, to as u64, 0, 0],
                );
                Self::dump_transition(from, to, tag, reason);
                panic!("[driver-contract] {} violation: {}", tag, reason);
            }
        }
    }

    pub fn dump_transition(
        from: DriverState,
        to: DriverState,
        tag: &'static str,
        reason: &'static str,
    ) {
        crate::serial_println!(
            "[driver-contract] dump tag={} reason={} from={:?} to={:?} cpu={} current_pid={:?}",
            tag,
            reason,
            from,
            to,
            crate::process::table::cpu_idx(),
            crate::process::current_pid()
        );
    }
}

fn driver_transition_name(from: DriverState, to: DriverState) -> &'static str {
    match (from, to) {
        (DriverState::New, DriverState::Initialized)
        | (DriverState::Stopped, DriverState::Initialized) => "driver.init",
        (DriverState::Initialized, DriverState::Started)
        | (DriverState::Suspended, DriverState::Started) => "driver.start",
        (DriverState::Started, DriverState::Stopped)
        | (DriverState::Suspended, DriverState::Stopped) => "driver.stop",
        _ => "driver.resource",
    }
}
