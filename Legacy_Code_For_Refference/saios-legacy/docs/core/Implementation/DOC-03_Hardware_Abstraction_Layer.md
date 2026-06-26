# SAIOS Hardware Abstraction Layer Specification
**Document ID:** DOC-03_Hardware_Abstraction_Layer.txt
**Layer:** Hardware and Boot
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01; authoritative for hardware-specific decisions

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt HARDWARE COMPATIBILITY MATRIX; HARDWARE ABSTRACTION LAYER; PORTABILITY RULES - PENTIUM 4 TO CORE ULTRA. SAIOS_SSOT_Part2.txt PART XI, NUMA-AWARE INTERRUPT ROUTING.

## PORTABILITY CHALLENGE

The same SAIOS kernel line must boot on a Pentium 4 from 2001 and scale to Core Ultra-class processors from 2026 and beyond. The kernel makes zero compile-time assumptions about CPU features. Every capability is detected through CPUID at boot, recorded in a global CpuFeatures snapshot, and used for runtime selection.

The kernel never panics on missing hardware features. The kernel never refuses to boot because an optional feature is absent. The kernel never silently produces incorrect results. Missing features select a degraded implementation and emit a KDS event so the system can explain its performance and behaviour.

Restated: absent feature means fallback plus evidence, not panic. Unsupported acceleration means slower path plus evidence, not wrong result. Old processor means compatible tier, not refusal.

## CPUID FEATURE DETECTION

CpuFeatures records: FPU, TSC, MSR, PAE, local APIC, MCE, CMOV, CLFLUSH, MMX, SSE, SSE2, SSE3, SSSE3, SSE4.1, SSE4.2, POPCNT, AES-NI, AVX, x2APIC, AVX2, AVX-512F, BMI1, BMI2, AMX-BF16, AMX-Tile, NX bit, 1GB pages, RDTSCP, 64-bit long mode, invariant TSC, physical core count, logical core count, NUMA node count, and cache line size.

Each flag has exactly one authority: the HAL. No higher contract issues CPUID, RDMSR, WRMSR, IN, OUT, or any privileged hardware instruction directly.

## PORTABILITY TIERS

| Tier | Processor examples | Guaranteed features | Fallback strategy |
|---|---|---|---|
| Tier 0 | Pentium 4 / early Athlon | 32-bit baseline; no assumed SSE2, PAE, or NX | Byte-wise copies, 32-bit addressing, 4KB pages, PIC/PIT paths |
| Tier 1 | Core 2 / early Atom | SSE2, PAE, TSC | PAE for high physical memory, SSE2 copies, APIC if present |
| Tier 2 | Nehalem and later | 64-bit, SSE4.2, APIC, NX | 64-bit canonical addressing, NX protection, AVX optional |
| Tier 3 | Skylake onward | AVX2, invariant TSC, x2APIC | Fast calibrated TSC, APIC affinity, AVX2 optional paths |
| Tier 4 | Core Ultra / Granite Rapids | AVX-512, AMX, TDX-class capabilities | Optional analytics acceleration and TDX-aware memory management |

Tier selection is runtime data. It is not a compile-time build profile.

## RUNTIME FEATURE SELECTION PATTERN

Any code needing an accelerated path asks the HAL for the immutable CpuFeatures snapshot and selects the best available implementation in descending order. For memory movement, the selection is AVX2 or wider only outside kernel core when explicitly permitted, then SSE2/MMX where available, then byte-wise fallback. The fallback path is always present and tested. Selection emits a boot-time KDS feature event for degraded or disabled acceleration.

## HAL INTERFACE CONTRACT

| Interface | Provides |
|---|---|
| CpuFeatures | Read-only CPUID-derived feature snapshot and portability tier classification |
| TimeSource | Nanosecond timestamps, TSC/HPET/PIT selection, calibration, reliability status |
| InterruptController | PIC/APIC/x2APIC abstraction, vector programming, EOI, IRQ affinity |
| Iommu | VT-d/AMD-Vi abstraction, DMA remapping, allowed ranges, fault reporting |
| SerialConsole | Direct register-write-only output path available before Gate 1 completes |
| MachineCheckHandler | MCE registration, user-frame recovery, kernel-frame Red Ring escalation |

## TIME SOURCE HIERARCHY

TSC is preferred on Tier 2+ when invariant TSC is detected and calibration succeeds. HPET is the fallback for systems with unreliable TSC. PIT is the last-resort baseline path for Tier 0/1 systems. Boot calibration measures TSC against HPET or PIT, validates monotonicity, records frequency, and emits a KDS event with selected source and reliability.

## INTERRUPT CONTROLLER SELECTION

x2APIC is preferred on Tier 3+. APIC is used on Tier 2 and any earlier system where it is present and reliable. PIC is used on Tier 0/1 fallback systems. InterruptController owns EOI semantics, vector routing, and affinity programming.

## IOMMU AND DMA PROTECTION

The HAL exposes IOMMU capability and programming primitives. DeviceContract uses those primitives to restrict each device to its authorised DMA ranges. Out-of-bounds DMA is blocked by hardware and reported as IOMMU_FAULT with device_id, dma_address, allowed_range, and action.

## NUMA TOPOLOGY AND INTERRUPT ROUTING

HAL discovers NUMA topology from ACPI SRAT and SLIT. If firmware topology is unavailable, HAL falls back to CPUID-derived processor topology and treats memory as single-node when locality cannot be proven. HAL populates NumaTopology with node IDs, CPU membership, memory ranges, distances, and source reliability. Discovery emits NUMA_TOPOLOGY_DISCOVERED, NUMA_NODE_CPU, and NUMA_NODE_MEMORY.

DeviceContract queries HAL for device home node. HAL programs APIC or x2APIC affinity so device interrupts target CPUs on the device's home node. If no CPU on the home node is available, DeviceContract falls back to the nearest available node and emits DEVICE_IRQ_AFFINITY_FALLBACK.

## SERIAL CONSOLE

SerialConsole is direct register-write-only output. It is operational before any gate can fail, does not allocate, does not sleep, and is used for boot diagnostics and Red Ring output. It is not a formatted logging subsystem; structured evidence belongs in KDS once Gate 4 completes.

## COMPLETION CHECK

A HAL implementer can define CpuFeatures, classify the five tiers, implement TimeSource selection, expose interrupt/IOMMU/serial/MCE abstractions, and know exactly which missing features degrade instead of halting.
