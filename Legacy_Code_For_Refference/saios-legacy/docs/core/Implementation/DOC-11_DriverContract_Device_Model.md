# SAIOS DriverContract and Device Model Specification
**Document ID:** DOC-11_DriverContract_Device_Model.txt
**Layer:** Subsystem Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 and DOC-03

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt DRIVERCONTRACT; DEVICE MODEL HIERARCHY; DEVICE CONTRACT; BUS ARCHITECTURE; RESOURCE MANAGEMENT; POWER STATE MANAGEMENT; DEVICE TELEMETRY. SAIOS_SSOT_Part2.txt NUMA-AWARE INTERRUPT ROUTING.

## DEVICE MODEL

The hierarchy is Bus -> Device -> Driver. A bus enumerates addresses. A device is a discovered or platform-described hardware function. A driver binds to a compatible device and owns hardware operation through the contract boundary.

Device fields: device_id globally unique, device_class, bus_type, bus_address, parent_device_id, device_state, resource_list, power_state, telemetry_handle, and driver_binding.

## DEVICE STATE MACHINE

Absent -> Present -> Claimed -> Active -> Suspended -> Active is the normal lifecycle. Active may transition to Faulted. Faulted may return to Active only with approved recovery. Active may transition to Removed. Removed is terminal. Every transition emits DEVICE_STATE_CHANGE.

## DEVICECONTRACT OWNERSHIP

DeviceContract owns device registration, state machine, resource arbitration, driver binding, power state management, device telemetry, hotplug coordination, IOMMU integration, and NUMA-aware interrupt routing.

Device registration is atomic. A complete DeviceDescriptor is required. Success emits DEVICE_REGISTERED. Failure emits DEVICE_REGISTRATION_FAILED with reason and leaves no partial device visible.

## BUS CONTRACT INTERFACE

BusContract provides scan, match, probe, and remove. PCI uses BDF enumeration, prefers MSI/MSI-X over legacy IRQ, and captures PCIe AER. USB uses hub enumeration and stable device IDs from bus address plus serial number. Platform devices come from ACPI or device tree and are not discoverable by scanning.

## RESOURCE MANAGEMENT

ResourceManager validates that MMIO, IO ports, IRQs, DMA ranges, and reserved memory do not overlap existing resources or kernel-reserved regions. Resources are marked owned by device_id and released automatically on Removed. Premature release triggers Red Ring high.

IOMMU programming allows only authorised DMA ranges. Out-of-bounds DMA emits IOMMU_FAULT and is blocked at hardware level.

## POWER MANAGEMENT

Power states are D0, D1, D2, D3hot, and D3cold. PowerManager coordinates transitions. A device may veto suspend; POWER_VETO includes device_id, requested_state, and reason.

## DEVICE TELEMETRY

Every registered device receives a pre-allocated KDS telemetry handle. Categories are error counters, performance metrics, health indicators, and state events. Health data includes temperature, voltage, wear level, link state, reset history, and queue depth where applicable.

## DRIVERCONTRACT

DriverContract owns driver registration, lifecycle, resource attribution, telemetry, and diagnostics. Lifecycle: Unregistered -> Registered -> Initialised -> Started -> Suspended optional -> Stopped -> Unregistered. Failure to initialise prevents Started.

Driver events: DRIVER_REGISTER, DRIVER_INIT, DRIVER_START, DRIVER_STOP, DRIVER_ERROR, DRIVER_RESET. Required payloads include driver_id, device_id, state, error_code where relevant, duration_ns where relevant, and recovery_action for failures.

## FAILURE MODES

KDS event from interrupt context must use lock-free KDS path. VfsContract call from teardown is forbidden; release VFS handles before teardown. DMA out of bounds is blocked by IOMMU and emits IOMMU_FAULT. Driver failing to stop within timeout is force-stopped, marked offline, emits DRIVER_ERROR, and remains unavailable. Duplicate device ID is rejected and existing device remains unaffected.

## NUMA-AWARE INTERRUPT ROUTING

DeviceContract determines device home node from DMA memory allocation node and uses HAL InterruptController to route IRQs to CPUs on that node. DEVICE_IRQ_AFFINITY_SET includes device_id, irq_number, home_node, and cpu_mask. If no home-node CPU is available, DEVICE_IRQ_AFFINITY_FALLBACK includes reason and selected fallback node.

## COMPLETION CHECK

A developer can implement an NVMe lifecycle with registration, IOMMU-protected DMA, NUMA IRQ affinity, error handling, stop, unregister, and correct KDS evidence.
