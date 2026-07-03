# ACPI (Advanced Configuration and Power Interface) Support

Status: Implemented and integrated
Owner: Kernel/Hardware Abstraction
Last updated: 2026-07-03

## Overview

ACPI support provides a comprehensive interface to platform firmware for hardware discovery, power management, and system control. The SAIOS kernel now includes full ACPI table parsing, processor enumeration, and power management infrastructure.

## Architecture

### ACPI Subsystem Structure

```
Boot Firmware (UEFI)
        |
        +-- RSDP Address in configuration tables
        |
    Kernel Boot Phase (seed/saios/src/main.rs)
        |
        +-- kernel::acpi::init(rsdp_address)
        |
    ACPI Manager (kernel/acpi.rs)
        |
        +-- RSDP Parsing
        |   +-- Locate RSDP from physical address
        |   +-- Validate signature and checksums
        |
        +-- Root Table Discovery
        |   +-- RSDT (ACPI 1.0) or XSDT (ACPI 2.0+)
        |   +-- Parse table array
        |
        +-- System Table Parsing
        |   +-- DSDT (Differentiated System Description Table)
        |   +-- FADT (Fixed ACPI Description Table)
        |   +-- MADT (Multiple APIC Description Table)
        |   +-- SRAT/SLIT (Resource affinity tables)
        |
        +-- Hardware Discovery
            +-- Processor enumeration via MADT
            +-- Interrupt routing configuration
            +-- Local APIC address identification
            +-- APIC/x2APIC support information
```

## Implementation Details

### Core Components

#### 1. **AcpiManager** (kernel/acpi.rs)
- Central ACPI coordination point
- Owns RSDP address and table state
- Manages processor discovery
- Tracks device enumeration
- Global instance with thread-safe access

#### 2. **Table Structures**
- `RsdpDescriptor` - Root System Description Pointer
- `AcpiTableHeader` - Generic table header
- `Fadt` - Fixed ACPI Description Table
- `Madt` - Multiple APIC Description Table
- `Rsdt`/`Xsdt` - Root table pointers

#### 3. **Processor Information**
```rust
pub struct ProcessorInfo {
    pub acpi_processor_id: u8,    // ACPI-assigned processor ID
    pub apic_id: u8,               // APIC identifier
    pub flags: u32,                // Enabled, online status
}
```

### Initialization Sequence

1. **UEFI Phase** (`boot/uefi/efi_main/src/acpi.rs`)
   - RSDP located via UEFI config tables
   - RSDP address passed to kernel via `SaiosBootInfo`
   - Checksum validation performed

2. **Kernel Boot Phase** (`seed/saios/src/main.rs`, line 102-125)
   ```
   Timeline: Services -> [ACPI init] -> Ready
   ```
   - Heap initialized first (required for Vec allocation)
   - `kernel::acpi::init(rsdp_address)` called
   - RSDP validation and table discovery
   - Processor enumeration completed
   - ACPI mark added to kernel timeline

3. **Runtime Access**
   ```rust
   if let Some(acpi_mgr) = kernel::acpi::get_manager() {
       let proc_count = acpi_mgr.processor_count();
       let local_apic = acpi_mgr.local_apic_address();
   }
   ```

### Table Parsing

#### RSDP (Root System Description Pointer)
- Located at firmware-provided address
- 20-byte base structure (ACPI 1.0)
- 36-byte extended structure (ACPI 2.0+)
- Checksum validation (basic and extended)
- Points to RSDT (32-bit) or XSDT (64-bit)

#### RSDT/XSDT (Root Tables)
- Contains array of pointers to other tables
- RSDT uses 32-bit pointers (ACPI 1.0)
- XSDT uses 64-bit pointers (ACPI 2.0+)
- Checksum validated
- Enumerated to find DSDT, FADT, MADT

#### FADT (Fixed ACPI Description Table)
- Power management configuration
- PM event/timer block addresses
- SMI command and enable/disable codes
- S-state information (S0-S5)
- Local APIC address in extended fields
- Reset register address (for reboot)

#### MADT (Multiple APIC Description Table)
- Local APIC base address
- Array of APIC structures:
  - **Type 0**: Processor Local APIC (x86/x64)
  - **Type 1**: IO APIC
  - **Type 2**: Interrupt Source Override
  - **Type 4**: Local APIC NMI
  - **Type 9**: Processor x2APIC (x86-64)
  - **Type 10**: x2APIC NMI

### Processor Enumeration

```
MADT Entry Parsing:
  1. Scan MADT entries by type
  2. For Type 0 (Processor Local APIC):
     - Extract ACPI Processor ID
     - Extract APIC ID (for interrupt routing)
     - Check enabled flag (bit 0)
     - Store in processors vector if enabled
  3. Accessible via: acpi_mgr.processors()
```

### Validation

All ACPI tables undergo strict validation:
- **Signature check**: First 4 bytes must match table type
- **Checksum validation**: Sum of all bytes ≡ 0 (mod 256)
- **Extended checksum**: For ACPI 2.0+ RSDP
- **Pointer validation**: No null pointer dereferences
- **Length consistency**: Table boundaries respected

## Shell Integration

### ACPI Command

```bash
acpi                # Display ACPI info
acpi info           # Show ACPI system information
acpi proc           # Show ACPI processors
acpi tables         # Show discovered ACPI tables
acpi status         # Show ACPI subsystem status
acpi shutdown       # Shutdown system (future)
acpi help           # Show ACPI help
```

#### Example Output

```
ACPI System Information
=======================
ACPI Version:     2
OEM ID:           QEMU
Status:           Enabled
Processors:       2
Local APIC Addr:  0xfee00000

Discovered Tables:
  DSDT, SSDT     - Differentiated/Secondary System Description Tables
  FADT           - Fixed ACPI Description Table
  MADT           - Multiple APIC Description Table
```

## Supported Features

### Fully Implemented ✓
- RSDP discovery and validation
- RSDT/XSDT parsing (both ACPI 1.0 and 2.0+)
- FADT table parsing
- MADT table parsing
- Processor enumeration (Type 0 entries)
- IO APIC detection
- Interrupt source override parsing
- Checksum validation (basic and extended)
- OEM information extraction
- Processor count and details
- Local APIC address retrieval

### Partially Implemented ~
- Device enumeration (basic structure, no AML)
- Power state information (parsed but not executable)
- SRAT/SLIT resource affinity tables

### Future Implementation (Requires AML Interpreter)
- Sleep states (S0-S5 transitions)
- Power button event handling
- Lid switch detection
- Hibernate/resume cycles
- Dynamic device hotplug
- Control Method Battery support
- Thermal zone management
- CPU dynamic frequency scaling
- System shutdown via \_S5 method

## Kernel Architecture Integration

Per ADR-0014 (Kernel Managers/Providers/Services):

- **Category**: Manager (owns ACPI state and discovery)
- **Responsibility**: Hardware discovery and power management coordination
- **Lifecycle**: Initialized during boot phase (after heap, before services)
- **Dependency**: HAL (x86_64 processor access), VMM (table mapping)
- **Consumers**: Scheduler (processor discovery), Timer (ACPI PM timer), PCI (interrupt routing)

## Technical Specifications

### ACPI Versions Supported
- ACPI 1.0 (RSDT-based)
- ACPI 2.0 (XSDT-based)
- ACPI 3.0+ (extended FADT fields)

### Architecture Support
- x86_64 (primary)
- APIC and x2APIC interrupt models

### Data Structure Sizes
```
RsdpDescriptor:    36 bytes
AcpiTableHeader:   36 bytes
Fadt (minimum):    244 bytes
Madt (variable):   36 + entries
ProcessorInfo:     12 bytes per processor
```

### Memory Requirements
- Heap allocation: ~10KB (processor vectors, device storage)
- Fixed memory: No driver buffers (uses firmware-owned tables)
- Table mapping: Via kernel VMM (no additional allocation)

## Error Handling

The ACPI subsystem returns `Result<(), &'static str>` with descriptive errors:

```
"ACPI: No RSDP address provided"
"ACPI: RSDP pointer is null"
"ACPI: Invalid RSDP signature"
"ACPI: RSDP checksum validation failed"
"ACPI: No valid RSDT or XSDT found"
"ACPI: Invalid root table signature"
"ACPI: Root table checksum validation failed"
"ACPI: MADT pointer is null"
"ACPI: Invalid MADT signature"
"ACPI: MADT checksum validation failed"
```

Errors during ACPI initialization log to kernel serial and continue boot; ACPI simply remains disabled.

## Performance Characteristics

- **Initialization time**: < 10ms (typical system with 4-8 processors)
- **Processor lookup**: O(1) via vector index
- **Table discovery**: O(n) linear scan of root table (n ≈ 10-20 tables typical)
- **Memory overhead**: ~1KB per processor + ~5KB for table structures

## Testing

### Unit Tests
Located in `kernel/acpi.rs#[cfg(test)]`:
- ACPI manager creation
- Signature constant validation
- Table structure size verification

### Integration Testing
- Boot on real hardware (QEMU, physical x86_64 systems)
- Verify processor enumeration matches expected count
- Confirm APIC addresses match MADT values
- Shell command execution and output validation

## Future Work

### Phase 1: AML Interpreter (High Priority)
- Implement minimal AML parser for power control methods
- Support S-state transitions (_PTS, _GTS, _BFS)
- System shutdown via \_S5 method

### Phase 2: Device Enumeration (High Priority)
- Parse DSDT/SSDT device objects
- Build device tree
- Query device capabilities and status

### Phase 3: Power Management (Medium Priority)
- Power button event handling
- Sleep state transitions
- ACPI timer for clock source

### Phase 4: Advanced Features (Medium Priority)
- CPU frequency scaling (P-states)
- Thermal zone management
- Battery/AC adapter events
- Lid switch events

### Phase 5: Native Drivers (Low Priority)
- EC (Embedded Controller) driver
- SMBus driver for sensor access
- GPIO resource management

## References

- ACPI Specification 6.4 (https://uefi.org/sites/default/files/resources/ACPI_Spec_6_4_Jan22.pdf)
- APIC Architecture (Intel 82489DX and variants)
- x2APIC Architecture (Intel Core i7 and later)
- ADR-0014: Kernel Managers/Providers/Services Architecture

## Code Location

```
Implementation:
  - kernel/acpi.rs          Main ACPI manager and table parsing
  - boot/uefi/efi_main/src/acpi.rs  RSDP discovery

Integration:
  - seed/saios/src/main.rs  Boot-time ACPI initialization
  - shell/commands/acpi.rs  Shell command handler

Boot Info Handoff:
  - boot/uefi/efi_main/src/lib.rs  SaiosBootInfo.acpi field
```
