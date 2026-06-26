# SAIOS Boot Sequence - Sixteen Validation Gates
**Document ID:** DOC-04_Boot_Sequence_Specification.txt
**Layer:** Hardware and Boot
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 and DOC-03; authoritative over initialization order

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt BOOT SEQUENCE - SIXTEEN VALIDATION GATES; MEMORY MAP AND RESERVED REGIONS; IMPLEMENTATION CHECKLIST. SAIOS_SSOT_Part2.txt PART XI NUMA-AWARE KDS PLACEMENT; PART XIV month goals.

## GATE PRINCIPLE

Boot proceeds in strict order. Each gate must pass before the next begins. A failed gate halts boot with a serial diagnostic and, after Gate 4, a KDS event. This is law, not a suggestion. The system never continues in a partially initialised state.

## ASSEMBLY ENTRY POINT

The assembly stub disables interrupts, sets an initial stack below 1MB, clears the direction flag, preserves and passes the multiboot information pointer, verifies the multiboot magic before trusting bootloader data, and transfers control to Rust main. Invalid multiboot magic prints a serial diagnostic and halts.

## SIXTEEN VALIDATION GATES

| Gate | Name | Initialises | Verifies | Events and failure behaviour |
|---|---|---|---|---|
| 0 | Physical Memory Map Validated | Memory map parser, KDS reservation, kernel frame reservation, SAIRU reserved CPU choice, NUMA-aware KDS partition plan | Usable memory, KDS default 512MB or minimum 32MB, kernel frame exclusion | Before KDS: serial only. Failure halts. NUMA topology is queried before KDS allocation so per-node KDS segments can be reserved. |
| 1 | HAL Initialised | CpuFeatures, SerialConsole, TimeSource, InterruptController, IOMMU, MCE handler | CPUID snapshot, console output, calibrated time source | Emits hardware feature/degraded-path evidence after KDS becomes available; failure serial-halts. |
| 2 | Lock Order Validator Installed | ReliabilityContract boot validator | Global lock order registry | Violations halt boot with diagnostic. |
| 3 | ExecutionContract Initialised | Per-CPU state, GDT, TSS, idle PID 0 | Current slots, ring-zero stacks, GDT/TSS load | Failure halts. |
| 4 | KDS Write Path Validated | Per-CPU rings, recursion guards | KDS region sealed, ring write/read sanity | Emits BOOT_KDS_READY. This is the moment everything changes: every later meaningful action can produce structured evidence. |
| 5 | ProcessContract Initialised | PID allocator, PID 1 reservation | PID 0 idle and PID 1 init reservation | Emits BOOT_GATE_PASSED or BOOT_GATE_FAILED. |
| 6 | SchedulerContract Initialised | Shared runnable queue, scheduler ownership state, idle registration | Idle absent from queues, queue/current-slot invariants | Emits scheduler boot events. |
| 7 | MemoryContract and AddressSpaceContract Initialised | Frame allocator, heap, kernel address-space handle | KDS frames not in free pool, fixed mappings sealed | Flight Recorder Daemon may start after this gate. |
| 8 | InterruptContract Initialised | IDT, NMI Red Ring handler, timer IRQ | IDT vectors installed | Gate covers IDT readiness. |
| 9 | SyscallContract Initialised | 64-bit syscall MSRs or 32-bit INT 0x80 vector, per-CPU syscall state | Entry path, saved user frame layout | Emits syscall path readiness. |
| 10 | DriverContract Initialised | PCI enumeration, essential driver load | 30s init timeout per driver | Driver failure marks device offline and emits KDS; boot continues unless essential invariant fails. |
| 11 | VfsContract Initialised | Root filesystem, proc, sys | Root mount and pseudo-filesystems | FS_MOUNT for root/proc/sys. |
| 12 | ObservabilityContract Fully Operational | Event registry, streaming pipeline | All event types registered | Full observability pipeline active. |
| 13 | ProgressContract Initialised | Stall monitors | Threshold monitors configured | Monitors scheduler progress, KDS throughput, inversion, starvation, OOM, driver timeout, IRQ storm. |
| 14 | ReliabilityContract Initialised | Live validation | Red Ring paths active | Any contract invariant violation triggers Red Ring immediately. |
| 15 | SAIRU Initialised | Context, Tool, Skill, Task, Knowledge, Planning, Policy engines | KDS read path | CE ingestion and RAF aggregation start after this gate. |
| 16 | Init Process Launched | PID 1 execution, service tree | Init image loaded | BOOT_COMPLETE already emitted; normal operation begins. |

## MEMORY LAYOUT ON REPRESENTATIVE 16GB SYSTEM

| Region | Approximate range/use |
|---|---|
| IVT/BIOS data | First 4KB |
| EBDA/BIOS | Up to 640KB |
| Bootloader and kernel ELF | Next 16MB |
| Kernel BSS/data | Next 48MB |
| KDS reserved region | Default 512MB, reducible to 32MB |
| Kernel frame pool | Next 1GB |
| SAIRU stacks | 4MB per reserved core |
| General free pool | Remaining approximately 14GB |

KDS region rules: default 512MB, minimum 32MB, maximum 25% of physical RAM, physically contiguous, write-back cache policy with non-cacheable crash-durability subregions, restricted to ObservabilityContract, persists across panic, flushed to NVMe on Red Ring, verified and sealed before contracts initialise.

## GATE 0 NUMA-AWARE KDS PARTITIONING

Gate 0 discovers NUMA topology before any KDS allocation. For each NUMA node, the KDS reserved region is partitioned into a local segment. Per-CPU ring buffers later allocate from the segment local to the CPU's node. Single-node systems use one contiguous KDS region.

## GATE FAILURE HANDLING

Failure output includes gate number, gate name, and failure reason. After Gate 4, BOOT_GATE_FAILED is emitted before halt if the KDS path is healthy. All CPUs halt. The kernel does not attempt degraded continuation through a failed gate.

## IMPLEMENTATION CHECKLIST

Before code: no_std custom target; assembly stack and interrupt disable; HAL feature detection before advanced paths; KDS rings from reserved memory; BOOT_KDS_READY first KDS event; repr(C) schema-versioned events; per-CPU recursion guard; lock order validation; idle PID 0; init PID 1; GDT/TSS before context switch; IDT before interrupts; syscall path before user; IOMMU before DMA; MCE before memory trusted; serial before failure; KDS region sealed; SAIRU reserved execution path; NUMA topology before SchedulerContract; DeviceContract before bus scan; Flight Recorder Daemon after Gate 7; CE ingestion and RAF aggregation after Gate 15.

## FIRST-BOOT EXPECTED SERIAL OUTPUT

SAIOS boot
Gate 0: physical memory map validated
Gate 1: HAL initialised
Gate 2: lock order validator installed
Gate 3: execution contract initialised
Gate 4: KDS write path validated
BOOT_KDS_READY
Gate 5: process contract initialised
Gate 6: scheduler contract initialised
Gate 7: memory and address space contracts initialised
Gate 8: interrupt contract initialised
Gate 9: syscall contract initialised
Gate 10: driver contract initialised
Gate 11: VFS contract initialised
Gate 12: observability fully operational
Gate 13: progress contract initialised
Gate 14: reliability contract initialised
Gate 15: SAIRU initialised
BOOT_COMPLETE
Gate 16: init launched

## COMPLETION CHECK

An implementer can follow the exact gate order, know which subsystem initialises at each gate, what evidence is emitted, and what serial text appears on a successful first boot.
