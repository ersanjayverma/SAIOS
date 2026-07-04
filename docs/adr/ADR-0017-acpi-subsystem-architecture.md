# ADR-0017: ACPI (Advanced Configuration and Power Interface) Subsystem Architecture

- Status: Accepted
- Date: 2026-07-03
- Complements: ADR-0014 (Kernel Managers/Providers/Services)
- Replaces: (None)

## Context

SAIOS kernel requires hardware discovery and power management capabilities. The system firmware (UEFI) provides this information via the ACPI standard. Previously, ACPI was stub-only; the bootloader collected RSDP but the kernel did not parse or use ACPI tables.

To enable proper processor enumeration, interrupt routing, and future power management, the kernel must:

1. Initialize ACPI tables during boot
2. Parse RSDP, RSDT/XSDT, FADT, MADT
3. Enumerate processors and discover interrupt configuration
4. Expose ACPI information via shell commands and kernel APIs
5. Provide foundation for future AML interpreter and power control

## Decision

Implement ACPI as a Kernel Manager subsystem following ADR-0014 architecture with the following design:

### Structure

```
ACPI Manager (kernel/acpi.rs)
├── RSDP Parser
│   ├── Signature validation
│   ├── Checksum validation (basic + extended)
│   └── Version detection (1.0 vs 2.0+)
├── Table Discovery
│   ├── RSDT (ACPI 1.0) or XSDT (ACPI 2.0+)
│   └── Table enumeration and signature matching
├── System Tables
│   ├── DSDT (Differentiated System Description Table)
│   ├── FADT (Fixed ACPI Description Table)
│   └── MADT (Multiple APIC Description Table)
├── Processor Enumeration
│   ├── MADT Type 0 (Processor Local APIC)
│   └── ProcessorInfo vector
└── Hardware State
    ├── Local APIC address
    ├── Interrupt routing info
    └── Power management configuration
```

### Lifecycle

- **Initialization**: During kernel boot phase, after heap/VMM, before services
- **Ownership**: Global static instance with atomic init guard
- **Access**: Via `get_manager()` and `get_manager_mut()` functions
- **Termination**: Persists for system lifetime (power management needed until shutdown)

### Manager Responsibilities (Per ADR-0014)

The ACPI Manager:

- **Owns** RSDP address, discovered table metadata, and processor information
- **Manages** table validation state and checksum verification
- **Enforces** invariants on processor enumeration
- **Exposes** via provider interface (kernel shell integration)
- **Does not** interpret AML or execute power control methods (future phase)

### Integration Points

1. **Bootloader Handoff** (`boot/uefi/efi_main/src/acpi.rs`)
   - RSDP located via UEFI configuration tables
   - Passed to kernel via `SaiosBootInfo.acpi`

2. **Kernel Boot** (`seed/saios/src/main.rs`)
   - Initialization after `ksf::bootstrap()`, before `interrupt::enable()`
   - Error handling: log failure but continue boot (graceful degradation)
   - Timeline mark: "ACPI" after successful initialization

3. **Shell Integration** (`shell/commands/acpi.rs`)
   - `acpi` command: Display system information
   - `acpi info`: OEM, version, processor count
   - `acpi proc`: Detailed processor list
   - `acpi tables`: Discovered table info
   - `acpi status`: Subsystem capability report

4. **Scheduler Integration** (Future)
   - Processor enumeration used for SMP bring-up
   - Affinity hints from SRAT table

5. **Interrupt Manager Integration** (Future)
   - MADT interrupt source overrides
   - APIC configuration
   - IRQ routing decisions

### Key Design Decisions

#### 1. Strict Table Validation

**Decision**: Every ACPI table undergoes signature + checksum validation.

**Rationale**:
- Firmware ACPI tables are often buggy
- Corrupted tables cause kernel panics or silent failures
- Validation catches issues early in boot

**Implementation**:
- `validate_checksum()`: Sum all bytes ≡ 0 (mod 256)
- Signature matching on all discovered tables
- Length bounds checking

#### 2. Immutable After Boot

**Decision**: ACPI state is immutable after initialization.

**Rationale**:
- ACPI tables don't change during normal operation
- Eliminates locking overhead
- Clear initialization boundary

**Implementation**:
- Read-only public API (`&self` only)
- No `get_manager_mut()` after boot
- One-time global initialization via atomic guard

#### 3. No AML Interpreter (Phase 1)

**Decision**: Defer AML (ACPI Machine Language) interpreter to Phase 2.

**Rationale**:
- AML is complex (bytecode VM, namespace, method calls)
- Table parsing alone provides immediate value (processor discovery, info)
- Can implement iteratively without architectural rework

**Constraints**:
- Power state transitions not available in v0.1
- Device enumeration limited (structure parsed, behavior not accessible)
- Will require separate interpreter subsystem in future

#### 4. Graceful Degradation

**Decision**: If ACPI initialization fails, kernel continues to boot.

**Rationale**:
- Some legacy systems may lack ACPI
- Single point of failure should not prevent boot
- Telemetry captured for debugging

**Implementation**:
- `init()` returns `Result<(), &'static str>`
- Failures logged but don't panic
- Shell still available to debug

#### 5. Memory Allocation Timing

**Decision**: Heap must be initialized before ACPI (for Vec allocations).

**Rationale**:
- Processor vector requires dynamic allocation
- Boot sequence already requires heap for other subsystems

**Implementation**:
- ACPI init placed after `heap::init()` in boot sequence
- No pre-allocation of static storage needed

### Trade-offs

**Positive**:
- Comprehensive hardware discovery without external tools
- Foundation for future power management and SMP
- Explicit processor enumeration for scheduler
- Follows established kernel architecture (ADR-0014)
- Graceful degradation on broken firmware

**Negative**:
- AML interpreter deferred (some firmware logic unavailable)
- No dynamic device hotplug in v0.1
- Power state transitions require future work
- Adds ~15KB kernel code footprint

### Testing Strategy

#### Unit Tests
- Table structure size validation
- Signature constant correctness
- Manager creation and initialization

#### Integration Tests
- Boot with ACPI disabled (legacy BIOS)
- Boot on QEMU (UEFI + ACPI)
- Boot on real hardware (when available)
- Shell `acpi` command execution

#### Validation
- Processor count matches expected value
- APIC addresses match MADT table
- Checksum validation catches corrupted tables

## Consequences

### Short Term (v0.1)
- Processor count visible in `acpi` shell command
- APIC addresses and IRQ routing info available
- Foundation for interrupt routing implementation
- Early detection of ACPI-related hardware issues

### Medium Term (v0.2)
- AML interpreter enables power states and device methods
- Scheduler uses SRAT affinity for NUMA-aware scheduling
- Interrupt routing uses MADT information
- Thermal and frequency scaling foundations

### Long Term (v0.3+)
- Full power management (sleep, hibernate, shutdown)
- Device hotplug and dynamic reconfiguration
- Processor virtualization support
- Battery management and thermal zones

## Alternatives Considered

### A1: Defer ACPI Until Later Phases
**Rejected** because:
- Interrupt routing needed sooner
- Processor enumeration blocks SMP implementation
- Architectural readiness now makes integration clean

### A2: Use Third-Party ACPI Library
**Rejected** because:
- No suitable no_std Rust ACPI crate exists
- Custom implementation allows tailored validation
- Educational value of understanding ACPI deeply
- License constraints of some alternatives

### A3: Implement Full AML Interpreter Immediately
**Rejected** because:
- AML is complex (1000+ lines for minimal interpreter)
- Blocks shipping processor discovery
- Can be added incrementally

### A4: Make ACPI Optional at Compile Time
**Rejected** because:
- Adds feature flag complexity
- Most systems need ACPI eventually
- Graceful runtime degradation sufficient

## Implementation Details

### Structures Defined

```rust
AcpiInfo {
    rsdp: u64,
    revision: u8,
    rsdt: u64,
    xsdt: u64,
    oem_id: [u8; 6],
}

AcpiTableHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

ProcessorInfo {
    acpi_processor_id: u8,
    apic_id: u8,
    flags: u32,
}

AcpiManager {
    rsdp_address: u64,
    revision: u8,
    processors: Vec<ProcessorInfo>,
    devices: Vec<AcpiDevice>,
    madt_address: u64,
    fadt_address: u64,
    dsdt_address: u64,
    local_apic_addr: u32,
    enabled: bool,
}
```

### Public API

```rust
pub fn init(rsdp_address: u64) -> Result<(), &'static str>
pub fn get_manager() -> Option<&'static AcpiManager>
pub fn get_manager_mut() -> Option<&'static mut AcpiManager>

impl AcpiManager {
    pub fn initialize(&mut self) -> Result<(), &'static str>
    pub fn processors(&self) -> &[ProcessorInfo]
    pub fn local_apic_address(&self) -> u32
    pub fn is_enabled(&self) -> bool
    pub fn revision(&self) -> u8
    pub fn processor_count(&self) -> usize
    pub fn oem_info(&self) -> Result<(String, u8), &'static str>
    pub fn enter_sleep_state(&self, state: SleepState) -> Result<(), &'static str>
    pub fn shutdown(&self) -> Result<(), &'static str>
    pub fn reboot(&self) -> Result<(), &'static str>
}
```

## References

- ACPI Specification 6.4: https://uefi.org/sites/default/files/resources/ACPI_Spec_6_4_Jan22.pdf
- ADR-0014: Kernel Managers/Providers/Services Architecture
- APIC Architecture (Intel x86 documentation)
- x2APIC Architecture (Intel Core i7+)

## Related Issues

- Processor discovery for SMP bring-up
- Interrupt routing for device drivers
- Power management state transitions
- Thermal and frequency scaling

## Sign-Off

- Kernel Architecture: Accepted
- Hardware Abstraction: Accepted
- System Integration: Accepted
