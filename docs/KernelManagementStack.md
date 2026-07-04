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

- `ObjectId` (encoded): Type(16) + Namespace(16) + Sequence(32)
- Stable labels: `PROC-XXXXXXXX`, `DRV-XXXXXXXX`, `DEV-XXXXXXXX`, `VOL-XXXXXXXX`, ...
- `ObjectType`: `Kernel`, `Service`, `Process`, `Thread`, `Driver`, `Device`, `Timer`, `Event`, `Surface`, `Window`, `File`, `Directory`, `Volume`, `Filesystem`, `Mount`, `Socket`, `Pipe`
- `ObjectState`: `Created`, `Initializing`, `Ready`, `Stopping`, `Destroyed`
- `ObjectHandle` references objects by id
- `KernelObject` trait exists as the common interface

Common metadata tracked per object:

- id
- object_type
- name
- state
- flags
- parent
- children
- owner
- reference_count
- created_tick
- last_modified_tick
- capabilities
- properties

### Registry APIs

- `register(...)`
- `unregister(...)`
- `transition(...)`
- `acquire(...)`
- `release(...)`
- `set_parent(...)`
- `set_owner(...)`
- `set_property(...)`
- `clone_object(...)`
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
- Lifecycle transitions are validated against a unified state machine.
- Parent-child relationships are updated consistently in registry operations.
- Reference counting operations are centralized in KOM.

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
- Storage block devices and partitions are surfaced in device views through provider enumeration of device-manager records.

### Input/runtime hardening notes (2026-07-05)

- Shell prompt stability takes priority over aggressive runtime input probing in fallback mode.
- PS/2 input initialization remains in console bring-up.
- USB HID can be used as fallback input when available, but continuous prompt-loop recovery/rescan behavior is avoided in fallback mode.
- Prompt startup no longer auto-clears the screen; boot diagnostics remain visible until operator action.

## 4.1 Single-Core Storage Scan Mode

Current kernel mode is single-core correctness-first.

- Storage scan execution is synchronous (foreground) and deterministic.
- No background storage worker is required to complete object publication.
- Shell commands can rely on scan completion semantics before returning.

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
- Provide `spawn(name, args, env)` and `exec(name, args, env)` launch paths
- Provide explicit `exit(pid, code)` completion API
- Emit lifecycle events to Event Bus

### Current process commands

- `ps`
- `jobs`
- `kill <pid>`
- `wait <pid>`
- `spawn <program> [args...]`
- `exec [KEY=VALUE ...] <program> [args...]`

### External program runtime path (implemented slice)

- `exec` and unknown-command fallback now route through Process Manager runtime APIs.
- Program resolution checks explicit paths and `/bin/<name>` before launch.
- Process lifecycle is recorded per launch with PID allocation and exit-code completion.
- Event Bus receives process start/stop events with pid and exit metadata.
- Binary metadata path includes PIE load-bias metadata handling.
- Dynamic-link metadata path validates interpreter, needed libraries, and required symbols before launch.

Seeded `/bin` entries currently include:

- `hello`
- `calc`
- `editor`
- `shell`
- `ls`
- `cat`
- `cp`
- `mv`
- `rm`
- `mkdir`
- `ps`
- `kill`
- `top`
- `uname`
- `stress`
- `cc`

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
- `health_score`: normalized score + warnings for failed services, faulted drivers, reload churn, heap pressure
- `diagnose`: driver fault/error inspection
- `explain scheduler|memory`: architecture/context explanation
- `recover`: restart failed services, reload faulted/stopped drivers, clear stale events, rerun diagnostics

### Shell commands

- `sairu health`
- `sairu diagnose`
- `sairu explain scheduler`
- `sairu explain memory`
- `sairu recover`

## 11. Package Image (Phase 5/6)

Package image is implemented in `kernel/package_image.rs` and mounted during shell initialization.

### Responsibilities

- Ensure root namespace layout exists:
  - `/boot`, `/bin`, `/etc`, `/home`, `/proc`, `/dev`, `/tmp`, `/usr`, `/lib`, `/system`
- Seed `/boot/package.manifest` with profile, directories, binaries, and shared libraries.
- Write ELF64 stub binaries for every `/bin` entry so the process runtime path can detect and load them.
- Seed shared-library metadata under `/lib` for dynamic-link validation.

### Seeded `/bin` entries

- `hello`, `calc`, `editor`, `shell`
- `ls`, `cat`, `cp`, `mv`, `rm`, `mkdir`
- `ps`, `kill`, `top`, `uname`, `stress`
- `cc`, `taskman`, `diskpart`

### Seeded `/lib` entries

- `ld-saios.so`
- `libc.so`
- `libm.so`
- `libshell.so`
- `libui.so`

### Shell visibility

- `pkgimg` shows current package image status.
- `pkgimg remount` re-runs `mount_default()`.

## 12. SNSH Commands

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
- `spawn <program> [args...]`
- `exec [KEY=VALUE ...] <program> [args...]`

## 13. Syscall ABI (Phase 2/6)

Initial stable syscall ABI surface is now defined in code:

- `open`
- `read`
- `write`
- `close`
- `fork`
- `exec`
- `wait`
- `exit`
- `sleep`
- `getpid`

Current implementation status:

- ABI versioned as `1.0.0`
- Numeric syscall IDs frozen for the above set
- Dispatcher is wired with implemented paths for `sleep` and `getpid`
- Remaining calls return explicit `Unimplemented` error codes until subsystem handlers land

Shell visibility:

- `syscall abi`
- `syscall check <id>`
- `syscall invoke <name|id> [arg0]`

## 14. C Runtime Scaffold (Phase 3/6)

Initial C runtime foundation is now present as a kernel contract surface:

- CRT ABI versioned as `1.0.0`
- Startup block builder for `program`, `argc`, `argv`, and `envp`
- Declared libc surface flags for `crt0`, `argv/envp`, `malloc/free`, and `printf`

Process runtime integration:

- Process launch path prepares a startup block before execution
- Process-start event metadata now includes `argc` and `envc`

Shell visibility:

- `crt abi`
- `crt probe <program> [args...]`

## 15. Core Userland Programs (Phase 4/6)

The `/bin` runtime surface now includes user-launchable programs resolved through the process runtime path:

- `hello`
- `calc`
- `stress`
- `ls`
- `cat`
- `cp`
- `mv`
- `rm`
- `mkdir`
- `ps`
- `kill`
- `top`
- `uname`

Behavior notes:

- Programs are launched through Process Manager execution (`exec` and unknown-command fallback).
- `/bin/<name>` lookup remains the default resolution path for external commands.
- ELF64 detection is performed by the shell program fallback; package image seeds `/bin` entries as ELF stubs.
- SAIFS/VFS use binary-safe reads (`vfs::read_path`) to avoid UTF-8 lossy conversion for executables.
- Process-start events include `argc` and `envc` metadata from CRT startup block preparation.

## 6.4 Package Image Runtime (Phase 5/6)

Package-image style root provisioning is now integrated as a runtime profile mount step.

Implemented behavior:

- Default package profile: `saios-base`
- Root layout ensured at runtime:
  - `/boot`
  - `/bin`
  - `/etc`
  - `/home`
  - `/proc`
  - `/dev`
  - `/tmp`
  - `/usr`
- Manifest materialized at `/boot/package.manifest`
- `/etc/profile` and `/etc/hostname` seeded by profile mount
- `/bin` entries for core userland programs are ensured by package profile mount

Shell visibility/control:

- `pkgimg`
- `pkgimg mount`
- `pkgimg remount`

## 6.5 Self-hosting Development Scaffold (Phase 6/6)

Initial self-hosting workflow scaffold is now wired:

- `cc <source.c> [-o output]` user program compiles to a runnable SAIOS stub artifact
- Compiled output embeds startup metadata and printable entry message payload
- Path-based execution fallback allows `./program` style invocation for compiled stubs

Demo-oriented flow now available:

- `cc hello.c`
- `./hello`

Current scope:

- This is a toolchain scaffold, not a full C compiler/runtime yet.
- It establishes the shell/runtime contract needed for later real compiler integration.

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
- `dashboard` (health score + warnings)
- `graph services`
- `timeline [limit]`
- `recover`
- `sairu health`
- `sairu diagnose`
- `sairu explain <target>`
- `sairu recover`

Filtering support:

- `services running|failed|stopped|healthy|warning|critical|offline|<name-fragment>`
- `drivers running|loaded|stopped|faulted|<name-fragment>`
- `devices online|offline|faulted|<name/driver/class-fragment>`
- `events <limit> [source-fragment]`

Abbreviated command aliases:

- `dash` -> `dashboard`
- `st` -> `stats`
- `obj` -> `objects`
- `dev` -> `devices`
- `drv` -> `drivers`
- `svc` -> `service`
- `svcs` -> `services`
- `ev` -> `events`
- `gr` -> `graph`
- `tl` -> `timeline`
- `rcv` -> `recover`

Abbreviated options:

- `jobs r|w|e`
- `services r|f|s|h|w|c|o`
- `drivers r|l|s|f`
- `devices o|off|f`
- `service ls|st|sp|rs|i|h`

Sorting option:

- `sort=asc|desc` supported by `jobs`, `services`, `drivers`, `devices`, `events`
- examples:
  - `drivers f sort=desc`
  - `services r sort=asc`
  - `events 128 driver-manager sort=desc`

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
- Dashboard/Timeline/Recovery UX: done

## 13. Immediate Next Steps

- Add concrete unsubscribe/callback execution in event bus (currently publish + recent/counters).
- Extend process manager from logical records to scheduler-linked termination semantics.
- Add per-device and per-driver performance counters into telemetry.
