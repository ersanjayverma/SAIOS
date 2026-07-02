# SAIOS Kernel Management Stack

This document tracks the current implementation status for:

- Kernel Object Manager (KOM)
- Kernel Service Manager (KSM)
- Device Manager
- Driver Manager (advanced)
- Process Manager
- Event Bus
- Telemetry
- SAIRU

It also lists the SNSH commands that expose each layer.

## 1. Architecture (Current)

```text
Hardware
  -> Drivers
    -> Device Manager
      -> Kernel Object Manager (KOM)
        -> SNSH

Kernel Service Manager (KSM)
  -> boots and manages services (memory/scheduler/vfs/device-manager/driver-manager/...)
  -> visible from SNSH

Event Bus
  -> receives lifecycle events from driver/device/process managers
  -> feeds telemetry + SAIRU

Telemetry
  -> snapshots CPU/memory/heap/scheduler/IRQ/drivers/processes/mounts/events

SAIRU
  -> reads KOM + services + events + telemetry for health/diagnose/explain
```

## 2. KOM (Implemented)

### Core model

- `ObjectId`
- `ObjectType`: `Kernel`, `Process`, `Thread`, `Driver`, `Device`, `Mount`
- `ObjectState`: `Created`, `Running`, `Stopped`, `Faulted`
- `ObjectHandle` references objects by id
- `KernelObject` trait exists as the common interface

### Registry APIs

- `register(...)`
- `unregister(...)`
- `find(id)`
- `find_by_name(...)`
- `find_by_type(...)`
- `enumerate()`
- `count()`
- `stats()`
- `events(limit)`
- `inspect(id)`

### Notes

- KOM is now the source of truth for driver/device object presence.
- Static object seeding is minimal (kernel/process/mount). Driver/device objects are registered by their managers.

## 3. KSM (Implemented)

KSM (currently backed by `ksf.rs`) manages service lifecycle and dependencies.

### Service metadata

- Name
- Version
- State
- Health
- Dependencies

### Lifecycle

- Initialize/start at bootstrap with dependency ordering
- Runtime commands:
  - start
  - stop
  - restart
  - info

### Managed services currently present

- `console`
- `memory`
- `object`
- `provider`
- `sif`
- `timer`
- `scheduler`
- `event`
- `health`
- `input`
- `vfs`
- `driver-manager`
- `device-manager`
- `process-manager`
- `ipc`
- `network`
- `sairu`
- `shell`

## 4. Device Manager (Phase 3)

Device Manager is implemented in `kernel/device.rs`.

### Responsibilities

- Register devices through Device Manager first
- Mirror device objects into KOM
- Keep a stable list of device metadata:
  - name
  - driver
  - class
  - status
  - linked KOM object id

### Runtime behavior

- No static fake seeding
- Runtime idempotent registration (`ensure_device`) from subsystem bring-up
  - COM1 and keyboard during console init
  - framebuffer device when framebuffer attaches

## 5. Driver Manager (Phase 4 Advanced)

Driver Manager is implemented in `kernel/driver.rs`.

### Driver metadata

- name
- version
- author
- status (`Loaded`, `Running`, `Stopped`, `Faulted`)
- dependencies
- attached devices
- linked KOM object id

### Advanced lifecycle

- dependency-aware `start(name)`
- `stop(name)`
- `reload(name)`
- hook dispatch for concrete drivers:
  - serial: serial init hook
  - pci: PCI init hook

### Device linkage

- Driver Manager refreshes attached devices from Device Manager records.

## 6. Process Manager (Phase 5)

Process manager is implemented in `kernel/process.rs`.

### Responsibilities

- Track kernel-managed process records
- Expose process table to SNSH jobs flow
- Provide `kill(pid)` and `wait(pid)` APIs
- Emit lifecycle events to Event Bus

### Current process commands

- `ps`
- `jobs`
- `kill <pid>`
- `wait <pid>`

## 7. Virtual Filesystem Shell Surface (Phase 6)

VFS/SAIFS is surfaced in shell compatibility + native commands.

### Commands

- `mount`
- `ls`
- `pwd`
- `cd`
- `mkdir`
- `touch`
- `cat`
- `rm`
- `tree [path]`

`tree` performs recursive SAIFS listing from current namespace or provided path.

## 8. Event Bus (Phase 7)

Event bus is implemented in `kernel/event.rs`.

### Event coverage currently wired

- Driver loaded/reloaded/faulted
- Device attached
- Process started/stopped

### APIs

- `publish(kind, source, detail)`
- `recent(limit)`
- `counters()`

### Shell visibility

- `events` now prints object-manager events and event-bus events.

## 9. Telemetry (Phase 8)

Telemetry snapshot is implemented in `kernel/telemetry.rs`.

### Snapshot fields

- CPU logical processors
- RAM MB
- Heap total/used
- Scheduler thread count
- IRQ total (timer ticks)
- Driver count
- Process count
- Mount count
- Event total

### Shell visibility

- `stats` now reports telemetry + KOM totals.
- `irq` reports aggregate IRQ counter.

## 10. SAIRU (Phase 9)

SAIRU facade is implemented in `kernel/sairu.rs`.

### Capabilities

- `health`: summary from telemetry + services + recent events
- `diagnose`: driver fault/error inspection
- `explain scheduler|memory`: architecture/context explanation

### Shell commands

- `sairu health`
- `sairu diagnose`
- `sairu explain scheduler`
- `sairu explain memory`

## 11. SNSH Commands

### KOM

- `objects`
- `objects <type>`
- `objects types`
- `inspect <id>`

### Services (KSM)

- `services`
- `service`
- `service <name>`
- `service start <name>`
- `service stop <name>`
- `service restart <name>`
- `service info <name>`
- `restart <service>`

### Devices

- `devices`
- `inspect keyboard0`
- `inspect fb0`
- `inspect COM1`

### Drivers

- `drivers`
- `driver <name>`
- `driver start <name>`
- `driver stop <name>`
- `driver reload <name>`
- `reload <name>`

### Processes

- `ps`
- `jobs`
- `kill <pid>`
- `wait <pid>`

`jobs` supports filtering:

- `jobs running`
- `jobs waiting`
- `jobs exited`
- `jobs <name-fragment>`

### Filesystem

- `mount`
- `ls`
- `tree [path]`

### Events/Telemetry/SAIRU

- `events`
- `stats`
- `irq`
- `sairu health`
- `sairu diagnose`
- `sairu explain <target>`

Filtering support:

- `services running|failed|stopped|healthy|warning|critical|offline|<name-fragment>`
- `drivers running|loaded|stopped|faulted|<name-fragment>`
- `devices online|offline|faulted|<name/driver/class-fragment>`
- `events <limit> [source-fragment]`

## 12. Current Milestone State

- Boot: done
- Memory: done
- Graphics/Console: done
- SNSH: done
- KOM: done (foundational)
- KSM: done (service-managed stack)
- Device Manager: done (runtime registration path)
- Driver Manager: done (advanced metadata + lifecycle hooks)
- Process Manager: done (jobs/kill/wait)
- VFS shell surface: done (`tree` added)
- Event Bus: done (lifecycle event stream + counters)
- Telemetry: done (snapshot + shell visibility)
- SAIRU: done (health/diagnose/explain)

## 13. Immediate Next Steps

- Add concrete unsubscribe/callback execution in event bus (currently publish + recent/counters).
- Extend process manager from logical records to scheduler-linked termination semantics.
- Add per-device and per-driver performance counters into telemetry.
