# SAIOS — SELF-AWARE INTELLIGENCE OPERATING SYSTEM
SINGLE SOURCE OF TRUTH
Version 2.0.0 | Living Document | Authoritative Engineering Reference
**Governing Principle:** The Blackhatbadshah Principle — Failure leads to Understanding leads to Resolution
**Subordinate To:** SAIOS Kernel Constitution
**The Goal:** Build the operating system that understands itself.

## PART I — VISION AND PHILOSOPHY

### VISION STATEMENT

To build the world's first Intelligence Operating System: a platform that understands, explains, diagnoses, predicts, and improves its own behavior while leveraging the vast software ecosystems of Linux, Windows, and modern cloud infrastructure. SAIOS is founded on a simple belief: computers should not merely execute instructions — they should understand the systems they are running. Our long-term vision is a future where operating systems become active intelligence layers capable of transforming complexity into understanding. Instead of forcing users, developers, and operators to manually assemble information from countless tools, the operating system itself becomes a source of knowledge, insight, and guidance. SAIOS exists to bridge the gap between humans and increasingly complex technology by making computing systems understandable.

### WHY SAIOS EXISTS

Modern computing has reached extraordinary levels of complexity. A single application may involve multiple processes, distributed services, containers, virtual machines, cloud infrastructure, databases, networks, storage systems, and external APIs. When failures occur, the information required to understand them already exists but it is scattered across logs, metrics, traces, crash dumps, dashboards, documentation, and monitoring systems. The challenge is no longer collecting information. The challenge is understanding it. Traditional operating systems primarily answer what happened. SAIOS is designed to answer what happened, why it happened, what is affected, what will happen next, and what should be done about it.

### CORE MISSION

SAIOS exists to eliminate invisible technical failures. The platform continuously observes, correlates, and understands information from every layer of the computing stack: hardware, firmware, kernel, memory, storage, filesystems, processes, networking, containers, applications, services, cloud infrastructure, and user activity. The objective is to transform raw telemetry into actionable understanding. Data alone is not intelligence. Understanding is intelligence.

### THE FUNDAMENTAL DIFFERENCE

Windows is an Operating System. Linux is an Operating System. macOS is an Operating System. SAIOS is an Intelligence Operating System. Execution remains essential but execution is no longer the sole purpose of the platform. Understanding becomes a first-class operating system capability. Observability, diagnostics, correlation, prediction, and explanation are treated as core system services rather than external tools.

### BUILT ON EXISTING ECOSYSTEMS

SAIOS is not designed to replace the software ecosystems built by Linux, Windows, or the broader open-source community. Instead SAIOS seeks to leverage and enhance them. The goal is to provide compatibility, interoperability, and familiar development experiences while adding a native intelligence layer that existing operating systems lack. Developers should be able to use familiar tools, languages, frameworks, and workflows while benefiting from system-wide intelligence and observability capabilities. SAIOS does not compete with ecosystems. SAIOS amplifies them.

### THE SELF-AWARE SYSTEM

Every subsystem within SAIOS contributes to a continuously evolving understanding of the machine. Every event becomes structured knowledge. Every interaction contributes context. Every failure becomes an opportunity for learning and improvement. The system should be capable of answering: why did this process crash, why is performance degrading, which update introduced instability, which service is causing latency, why is memory pressure increasing, which dependency is responsible, and what are the most likely solutions. Users should not be required to manually assemble evidence from disconnected tools. SAIOS should perform that analysis automatically.

### NATIVE SYSTEMS INTELLIGENCE

The future of SAIOS is not a chatbot layered on top of an operating system. The intelligence must be built into the operating system itself. Core intelligence capabilities: Event Intelligence — every meaningful system action is represented as structured events including process creation, process termination, memory allocation, page faults, network failures, driver activity, and resource contention. Events become the foundation of system understanding. Diagnostic Intelligence — the platform continuously analyzes relationships between events to identify root causes and explain failures. Predictive Intelligence — the platform identifies patterns and trends that may indicate future problems before they become outages. Optimization Intelligence — the platform recommends and eventually automates improvements to performance, stability, and resource utilization. Infrastructure Intelligence — the platform understands relationships between applications, services, devices, networks, containers, and infrastructure components.

### AI-READY ARCHITECTURE

SAIOS is designed to be AI-model agnostic. The operating system itself generates structured knowledge through event systems, telemetry, tracing, flight recorders, correlation engines, knowledge graphs, and diagnostic services. This intelligence can be consumed by any capable AI system — whether OpenAI, Anthropic, Gemini, Qwen, DeepSeek, Llama, a local model, or a future technology that does not yet exist. The operating system remains the source of truth. The value of SAIOS is not a specific AI model. The value is the system's ability to understand itself.

### HUMAN-CENTERED COMPUTING

Computers should adapt to humans. Humans should not be forced to adapt to computers. SAIOS translates complex machine behavior into understandable explanations, meaningful recommendations, and actionable guidance. The operating system should explain itself. The operating system should assist users. The operating system should help people make better decisions.

### THE BLACKHATBADSHAH PRINCIPLE

Most organizations do not lose time because systems fail. They lose time because understanding those failures takes too long. SAIOS is built around a simple principle: Failure leads to Understanding leads to Resolution. The faster understanding occurs, the faster recovery becomes possible. Every component within SAIOS is designed to reduce the distance between these three stages.

### DEVELOPER-FIRST PHILOSOPHY

Developers remain central to the future of computing. SAIOS is designed to amplify developer capability through intelligent diagnostics, intelligent tracing, intelligent profiling, root-cause analysis, dependency analysis, performance insights, and system-wide observability. The goal is not to replace developers. The goal is to provide them with unprecedented visibility and understanding.

### LONG-TERM OBJECTIVE

The objective of SAIOS is not to surpass Windows, Linux, or macOS in feature count. The objective is to surpass them in understanding. A mature SAIOS platform should be capable of understanding its own behavior, explaining system decisions, diagnosing failures automatically, predicting future issues, recommending corrective actions, optimizing itself continuously, and improving reliability over time. When users encounter problems, the operating system should already possess the context required to explain them. At maturity SAIOS becomes more than an operating system. It becomes a systems intelligence platform capable of powering everything from personal computers to global-scale infrastructure.

## PART II — KERNEL CONSTITUTION

### DESIGN PHILOSOPHY AND NON-NEGOTIABLE CONSTRAINTS

Core Belief: The founding belief of SAIOS is that computers should not merely execute instructions — they should understand the systems they are running. Every kernel subsystem in SAIOS therefore serves two masters simultaneously: execution and understanding. Neither is secondary. Understanding is a first-class operating system capability, not an add-on bolted onto a traditional kernel.

The Blackhatbadshah Principle Operationalised: Most organisations do not lose time because systems fail; they lose time because understanding those failures takes too long. Every kernel action must reduce the distance between Failure, Understanding, and Resolution. If a subsystem cannot explain what it did, it has failed its intelligence mandate.

### NON-NEGOTIABLE INVARIANTS

Invariant 1: One process identifier is in exactly one execution place at all times — either one CPU current slot, the run queue, a blocked wait structure, the zombie list, or dead.
Invariant 2: A process identifier marked as on-CPU is present in exactly one current-slot and absent from all run queues.
Invariant 3: A queued process identifier is Ready, not on-CPU, and absent from all current-slots.
Invariant 4: The Task State Segment stack pointer for ring zero, the syscall CPU state kernel stack pointer, and the process kernel stack top are identical for the current process identifier on each CPU at all times.
Invariant 5: User GS and kernel GS are never inferred from the active model-specific register alone.
Invariant 6: CR3 is a hardware mirror of the current address-space handle and never the owner of address-space identity.
Invariant 7: Copy-on-write reference count metadata and page table entry copy-on-write flags change together, always, without exception.
Invariant 8: Process death releases address-space ownership exactly once.
Invariant 9: Lock acquisition order follows the global priority order, always, without exception.
Invariant 10: Knowledge Data Store reserved memory is never accessible to any path other than the ObservabilityContract.
Invariant 11: SAIRU never bypasses a contract, modifies state directly, or takes action without human approval.
Invariant 12: If the kernel is not in the fault zone, no user program can crash or hang it.

### RUST KERNEL RULES

The kernel is written in Rust using the no-standard-library profile. The panic strategy is abort because unwinding requires heap allocation which the kernel cannot provide during early boot or in interrupt context. Floating point is forbidden in kernel core because FPU state belongs to user processes. All event structures use the C representation to guarantee a stable application binary interface for Knowledge Data Store consumers. Volatile reads and writes are used for memory-mapped I/O and ring buffer pointers to prevent compiler reordering. The default atomic ordering is sequentially consistent until a weaker ordering can be proven safe for a specific path. Recursion is forbidden in Knowledge Data Store paths to prevent stack overflow in fault context. Dynamic allocation is forbidden in interrupt context because all buffers must be pre-reserved. Unsafe blocks are minimised and every unsafe block carries a safety comment explaining why the operation is sound.

### HARDWARE COMPATIBILITY MATRIX

SAIOS must boot on a Pentium 4 from 2001 and scale to an Intel Core Ultra from 2026 and beyond. This spans more than twenty-five years of x86 evolution. The fundamental rule is zero compile-time assumptions about CPU features. Every capability is detected at boot via CPUID and stored in a global processor features structure. The feature structure records the presence or absence of: FPU, TSC, MSR, PAE, local APIC, MCE, CMOV, CLFLUSH, MMX, SSE, SSE2, SSE3, SSSE3, SSE4.1, SSE4.2, POPCNT, AES-NI, AVX, x2APIC, AVX2, AVX-512F, BMI1, BMI2, AMX-BF16, AMX-Tile, NX bit, 1GB pages, RDTSCP, 64-bit long mode, invariant TSC, physical core count, logical core count, NUMA node count, and cache line size.

Portability Tiers:
Tier 0 (Pentium 4 / early Athlon): 32-bit, no SSE2, no PAE, no NX. Fallback: byte-wise copies, 32-bit addressing, THP disabled.
Tier 1 (Core 2 / early Atom): SSE2, PAE, TSC. Fallback: PAE for 4GB+ addressing, SSE2 copies.
Tier 2 (Nehalem and later): 64-bit, SSE4.2, APIC, NX. Fallback: 64-bit addressing, NX protection, AVX optional.
Tier 3 (Skylake onward): AVX2, invariant TSC, x2APIC. Fallback: fast TSC calibration, AVX2 SIMD paths.
Tier 4 (Core Ultra / Granite Rapids): AVX-512, AMX, TDX. Fallback: AMX-offloaded analytics, TDX-aware memory management.

Golden Rule: If a feature is not present, the kernel degrades gracefully. It never panics. It never refuses to boot. It never silently produces incorrect results. Every degraded path emits a KDS event at boot recording which features are absent so SAIRU can explain performance characteristics of the running system.

### HARDWARE ABSTRACTION LAYER

The Hardware Abstraction Layer is the lowest kernel tier. No code above it may issue CPUID, RDMSR, WRMSR, IN, OUT, or any other privileged instruction directly. Architecture-specific code lives here and only here. The HAL exposes the following interfaces: CpuFeatures — a read-only snapshot of CPUID results; TimeSource — an abstraction over TSC, HPET, PIT providing nanosecond timestamps with calibration; InterruptController — an abstraction over legacy PIC, APIC, and x2APIC; Iommu — an abstraction over VT-d and AMD-Vi providing DMA remapping; SerialConsole — a direct register-write-only output path used in early boot and Red Ring; MachineCheckHandler — MCE registration and recovery coordination.

### MEMORY MAP AND RESERVED REGIONS

On a representative 16GB system the physical memory layout begins with the real-mode IVT and BIOS data area in the first 4 kilobytes, followed by the EBDA and BIOS area totalling 640 kilobytes. The bootloader and kernel ELF image occupy the next 16 megabytes. Kernel BSS and data sections occupy the next 48 megabytes. The KDS reserved region occupies the next 512 megabytes, reducible to 32 megabytes on low-memory Pentium 4 systems. The kernel frame pool occupies the next 1 gigabyte. SAIRU stack occupies 4 megabytes per reserved core. The remaining approximately 14 gigabytes form the general free memory pool managed by the allocator.

KDS Region Specification: default size 512MB, minimum size 32MB for Pentium 4 systems, maximum size 25% of physical RAM. Must be physically contiguous. Cache policy is write-back with non-cacheable regions for crash durability. Access restricted to ObservabilityContract exclusively. Persists across kernel panic. Flushed to NVMe on Red Ring. Properties verified at boot and sealed before any contract initialises.

### BOOT SEQUENCE — SIXTEEN VALIDATION GATES

Boot proceeds in strict order. Each gate must pass before the next begins. A failed gate halts boot with a diagnostic on the serial console. This is not a suggestion; it is a law.

Entry Point: Boot begins in assembly code that disables interrupts, sets up an initial stack below 1MB, clears the direction flag, passes the multiboot information pointer to the Rust entry point, and calls the Rust main function. The Rust main function first verifies the multiboot magic number and halts with a diagnostic if invalid.

Gate 0 — Physical Memory Map Validated: The kernel parses the memory map, reserves the KDS region, reserves the kernel frame region, and selects the reserved CPU core for SAIRU. If the KDS region cannot be reserved at default size it falls back to minimum. If even the minimum cannot be satisfied, boot halts.

Gate 1 — HAL Initialised: Detects processor features, initialises the serial console, calibrates the TSC, initialises the interrupt controller (choosing between PIC and APIC or x2APIC based on detected features), initialises the IOMMU if present, and installs the MCE handler.

Gate 2 — Lock Order Validator Installed: The ReliabilityContract installs the lock order validator. From this point forward any lock acquisition violating the global priority order causes boot to halt with a diagnostic before the first user process starts.

Gate 3 — ExecutionContract Initialised: Initialises per-CPU execution state, loads the Global Descriptor Table, loads the Task State Segment, and creates the idle process with process identifier zero. The idle process is immortal.

Gate 4 — KDS Write Path Validated: The ObservabilityContract initialises per-CPU ring buffers from the KDS reserved region and initialises per-CPU recursion guards. Emits BOOT_KDS_READY recording TSC frequency, KDS size, and CPU count. From this point every meaningful kernel action can emit a structured event.

Gate 5 — ProcessContract Initialised: Initialises the process identifier allocator and reserves slot for PID 1, which will be init.

Gate 6 — SchedulerContract Initialised: Initialises per-CPU run queues and registers the idle processes.

Gate 7 — MemoryContract and AddressSpaceContract Initialised: MemoryContract initialises the frame allocator from the reserved kernel frame region. The kernel heap allocator is initialised — this is the first use of dynamic allocation in the entire boot sequence. AddressSpaceContract creates the kernel address space handle.

Gate 8 — InterruptContract Initialised: Loads the Interrupt Descriptor Table, installs the NMI handler for Red Ring broadcast, and registers the timer interrupt.

Gate 9 — SyscallContract Initialised: Configures syscall MSRs on 64-bit systems, or configures interrupt 0x80 vector on 32-bit systems for Pentium 4 compatibility. Allocates per-CPU syscall state.

Gate 10 — DriverContract Initialised: Enumerates PCI devices and begins loading essential drivers. Each driver has 30 seconds to initialise; failure marks the device offline and emits a KDS event but boot continues.

Gate 11 — VfsContract Initialised: Mounts the root filesystem and exposes proc and sys pseudo-filesystems for compatibility with existing tools.

Gate 12 — ObservabilityContract Fully Operational: Registers all event types and activates KDS streaming. From this point forward the full observability pipeline is operational.

Gate 13 — ProgressContract Initialised: Starts all stall monitors including scheduler forward progress, KDS write throughput, priority inversion duration, process starvation, OOM pressure trend, driver initialisation timeout, and IRQ storm detection.

Gate 14 — ReliabilityContract Initialised: Activates live validation. From this point any contract invariant violation triggers the Red Ring immediately.

Gate 15 — SAIRU Initialised: SAIRU initialises on the reserved core. All seven engines — Context, Tool, Skill, Task, Knowledge, Planning, and Policy — become operational. The KDS read path is verified. BOOT_COMPLETE is emitted recording boot duration, processor features summary, and KDS event count.

Gate 16 — Init Process Launched: PID 1 is launched from the root filesystem. Service tree construction begins. Normal operation commences.

Gate Failure Handling: If any gate fails, the kernel emits a KDS event if KDS is ready (gates 4 and above), prints a diagnostic to the serial console identifying the gate number, gate name, and failure reason, and halts all CPUs. The system does not continue in a partially-initialised state.

### EVENT INTELLIGENCE SUBSYSTEM AND KNOWLEDGE DATA STORE

The KDS is the nervous system of SAIOS. It is the single source of evidence for all observability, diagnostics, prediction, and SAIRU intelligence functions. Without the KDS, SAIOS is just another operating system. With it, SAIOS becomes an intelligence operating system.

Fundamental Properties: First, append-only — no subsystem ever modifies or deletes an existing KDS record. Second, crash-safe — KDS write paths are hardened against all fault conditions. A fault mid-write may leave the last in-flight event incomplete but all prior events are intact. Third, physically reserved — KDS memory is allocated at boot from a dedicated reserved region; no kernel subsystem, no user process, and no allocator may use this region for any other purpose. Fourth, SAIRU-accessible always — the KDS remains readable by SAIRU after kernel halt; the read path requires no scheduler, no VFS, no syscall layer. Fifth, self-describing — every KDS event carries its own schema version so consumers require no out-of-band schema knowledge to interpret any event.

Event Schema: Every KDS event contains: event identifier (UUID v7, time-ordered and unique), event type (typed enumeration: PROCESS, MEMORY, NET, FS, SCHED, HW, DRIVER, SECURITY), timestamp in nanoseconds since kernel epoch (never zero), source contract (enumeration identifying which contract owns the emitting subsystem), severity (DEBUG / INFO / WARN / ERROR / CRITICAL), CPU identifier, process identifier (zero means kernel context), correlation identifier (optional UUID v7 linking causally related events), schema version (16-bit integer), payload (typed subsystem-specific structure), and context tags (up to 8 key-value pairs for additional indexable metadata).

Mandatory Event Categories: ProcessContract emits PROCESS_CREATE on fork with PID, parent PID, executable path, argv hash, env hash, and cgroup; emits PROCESS_TERMINATE on exit with PID, exit code, signal, CPU time, and memory peak. MemoryContract emits MEMORY_ALLOC on allocation with PID, virtual address, size, flags, and stack trace hash; emits PAGE_FAULT on fault with PID, address, fault type, and resolution time. SchedulerContract emits SCHED_CONTEXT_SWITCH on every switch with from-PID, to-PID, CPU, reason, and latency; emits SCHED_PREEMPT with PID, preempted-by, priority, and run queue depth. Networking emits NET_CONNECT with PID, source and destination IP, port, protocol, and latency; emits NET_ERROR with PID, error type, retries, and packet loss rate. VfsContract emits FS_OPEN with PID, path, flags, and latency; emits FS_WRITE with PID, inode, bytes, latency, and dirty page count. DriverContract emits DRIVER_ERROR with driver ID, device ID, error code, and recovery action. InterruptContract emits IRQ_HANDLER with IRQ number, CPU, handler time, and frequency. SecurityContract emits SECURITY_SYSCALL_DENIED with PID, syscall number, policy ID, and action. ReliabilityContract emits RESOURCE_CONTENTION with resource type, contending PIDs, and wait time.

Per-CPU Lock-Free Ring Buffer: The KDS write path uses a per-CPU Single-Producer Single-Consumer ring buffer. This is lock-free and never blocks except for CRITICAL events under overflow. Each ring buffer has a write head updated only by the owning CPU, a read tail updated only by the Flight Recorder daemon or SAIRU, a buffer base pointer into the KDS reserved region, capacity in bytes, fixed slot size of 256 bytes for cache-line alignment, slot count, overflow counter, and critical loss counter. The write operation loads the write head with relaxed ordering and the read tail with acquire ordering. If the ring is full and the event is not CRITICAL, the event is dropped and the overflow counter is incremented. If the ring is full and the event is CRITICAL, the writer blocks for up to 1 millisecond waiting for the reader to catch up; if still full, a critical loss is recorded and the Red Ring is triggered. If not full, the slot address is computed, the event is serialised into the slot, and the write head is incremented with release ordering.

Recursion Guard: Every CPU maintains a per-CPU boolean kds_emitting flag. Before any KDS write, the kernel checks this flag. If true, the write is silently suppressed. If false, the flag is set to true, the write proceeds, and the flag is set back to false. This is not a lock. It is a per-CPU flag requiring no synchronisation. It is the only mechanism preventing infinite recursion when a KDS write would itself trigger another KDS write.

Public Emit API: emit_event is the primary entry point for all kernel observability. Every meaningful kernel action must call this function. It checks the recursion guard, fills automatic fields (timestamp, CPU ID, PID, event ID), obtains the per-CPU ring, and attempts the write. emit_critical is used for CRITICAL severity events; it behaves identically except the underlying ring write handles critical overflow by blocking and potentially triggering the Red Ring.

Event Delivery Guarantees: Events are delivered in order within a single CPU core via the per-core ring buffer. Cross-core ordering is best-effort with logical timestamps via Lamport clocks. Zero event loss is guaranteed for CRITICAL and ERROR severity through a blocking ring with overflow protection. Lossy sampling is permitted for DEBUG and INFO under memory pressure. All events are persisted to the Flight Recorder before acknowledgement for CRITICAL severity.

### EXECUTIONCONTRACT

Ownership: the current CPU, current process per CPU, kernel stack, saved kernel stack pointer, user register context, CR3 activation, TSS ring-zero stack pointer, and GS and TLS boundaries.

Per-CPU State Structure (cache-line aligned to avoid false sharing): CPU identifier, current PID (zero means idle), pointer to current process control block, kernel stack top for current process, saved kernel stack pointer for context switch, TSS ring-zero stack pointer (must match kernel stack top), user GS base for TLS, kernel GS base pointing to this per-CPU state, interrupt nesting depth, preemption disabled count, KDS emitting flag (recursion guard), and padding to a full cache line.

Invariants: TSS ring-zero stack pointer, syscall CPU state kernel stack pointer, and process kernel stack top are identical for the current PID on each CPU. User GS and kernel GS are never inferred from the active MSR alone. CR3 is a hardware mirror of the current address-space handle and never the owner of address-space identity. No two CPUs share the same current slot.

Failure Modes: current slot null outside idle path triggers Red Ring critical; two CPUs reporting the same current PID triggers Red Ring critical; TSS ring-zero stack pointer mismatch with kernel stack top triggers Red Ring critical; kernel stack overflow triggers Red Ring non-recoverable; non-canonical user stack pointer on syscall entry delivers SIGSEGV to the process; non-canonical user instruction pointer on return from interrupt delivers SIGSEGV to the process; CR3 mismatch with current address space handle triggers Red Ring critical.

Interrupt-Context Constraint: ExecutionContract paths never sleep, never allocate, and never acquire locks below Priority One in the global order.

GDT and TSS: GDT contains null entry, kernel code segment, kernel data segment, user code segment (ring 3), user data segment (ring 3), and TSS entry split across two descriptor slots for 64-bit mode. TSS for 64-bit mode contains the ring-zero stack pointer (kept synchronised with current process kernel stack top), three IST entries for NMI / double fault / critical exceptions, and an I/O map base.

### PROCESSCONTRACT

Ownership: process lifecycle state machine, PID allocation and release, process creation, zombie publication, dead cleanup, credentials, sessions, process groups, and file descriptor table inheritance.

State Machine: exactly six states — New, Ready, Running, Blocked, Zombie, Dead. New to Ready on admission. Ready to Running when scheduled. Running to Blocked when waiting on a resource. Blocked to Ready when woken. Running to Zombie on exit, fault, or fatal signal. Zombie to Dead when reaped by parent. No other transitions permitted.

Invariants: A PID is in exactly one state at all times. Zombie entered exactly once per PID. Dead entered exactly once per PID. No PID transitions from Dead to any other state. Waiters woken exactly once on Zombie entry and never twice.

Failure Modes: PID in two states simultaneously triggers Red Ring critical; Zombie entered twice for the same PID triggers Red Ring critical; waiter woken twice for the same PID death triggers Red Ring high; PID leaked and never reaching Dead causes ResourceContract via ProgressContract to produce a SAIRU diagnosis and alert; credential change without audit event triggers Red Ring high; fork producing child with non-unique PID triggers Red Ring critical.

Corner Cases: process exiting while holding a kernel lock triggers Red Ring because locks must never outlive their owning process. OOM killer selecting a Zombie process skips it and emits a KDS event. OOM killer selecting a process in kernel context sends SIGKILL and the process must check signals on every kernel exit boundary. Process group leader exiting before members causes members to be re-parented and an audit event is emitted so there is no orphan. PID 1 exiting triggers Red Ring non-recoverable. Fork returning in the child with no available memory for the stack means the COW fault at first stack access triggers OOM handling which sends SIGKILL to the child while parent is unaffected.

### MEMORYCONTRACT AND ADDRESSSPACECONTRACT

MemoryContract Ownership: frame ownership, frame reference counts, mapping authority, COW lifecycle, mmap, brk, execution mapping, unmap, and stack growth.

AddressSpaceContract Ownership: address-space handles, CR3 transitions, page-table construction and destruction, and page-table mutation APIs.

MemoryContract Invariants: COW reference count metadata and page table entry COW flags change together, never one without the other. A frame is owned by exactly one entity at any time — process, kernel, KDS, or free pool. Frame reference count reaching zero means the frame is immediately available for reuse with no deferred free. KDS reserved frames are never in the free pool, verified at boot and sealed.

AddressSpaceContract Invariants: CR3 is a hardware mirror of the current address-space handle. Page-table destruction executes only after the address space is no longer current on any CPU. Fork produces a new address-space handle and the parent's handle is unchanged.

Failure Modes: double-freed frame triggers Red Ring critical; frame reference count and PTE COW flag mismatch triggers Red Ring high; OOM invokes OOM killer and emits KDS event; OOM killer finding no victim triggers Red Ring with SAIRU diagnosis; allocation failure in interrupt context returns null and the caller must handle it; KDS frame allocated to non-KDS path triggers Red Ring immediately; fragmentation preventing large allocation attempts compaction and returns failure if compaction fails; faulty RAM in user frame causes MCE handler to poison the frame, deliver SIGBUS, and blacklist the frame permanently; faulty RAM in kernel frame is non-recoverable and triggers Red Ring; NUMA migration racing with address space destruction causes migration cancellation and destruction to proceed; COW fault during fork teardown blocks until teardown completes; stack growth overlapping another mapping returns SIGSEGV and kernel is unaffected.

OOM Killer Selection Algorithm: ProgressContract identifies memory pressure trend. MemoryContract enumerates all processes by OOM score. Score computed as memory footprint plus penalty for privilege plus penalty for runtime. Processes in kernel context are skipped. Zombie processes are skipped. Highest-scoring eligible process receives SIGKILL. If no eligible process exists, Red Ring triggered with SAIRU diagnosis. Every step emits a KDS event.

Page Sizes and Portability: On Tier 0 without PSE only 4KB pages are used. On processors with PSE, 2MB THP is available but defaults to madvise policy so processes must opt in. On processors with 1GB page support, 1GB huge pages are available for explicit allocations. The kernel never assumes any page size beyond 4KB without runtime feature detection.

### SCHEDULERCONTRACT

Ownership: run queue membership, CPU assignment, block, wake, exit handoff, process selection, and finish-switch bookkeeping.

Invariants: A PID marked as on-CPU is in exactly one current slot and absent from all run queues. A queued PID is Ready, not on-CPU, and absent from all current-slots. The idle process is never in the run queue. The finish-switch operation executes for every switch-to with no exceptions.

Base Algorithm: Based on CFS, extended with intelligence hints from the Optimization Intelligence Subsystem. Supports scheduling classes: Deadline real-time, FIFO real-time, Round-Robin real-time, CFS for normal processes, and Idle. NUMA awareness is first-class, with NUMA topology queried from HAL at boot.

Telemetry: SCHED_CONTEXT_SWITCH and SCHED_PREEMPT emitted on every switch. Per-CPU run queue depth exposed as real-time metric. Per-task scheduling latency tracked with nanosecond precision.

Failure Modes: PID in run queue and on-CPU simultaneously triggers Red Ring critical; run queue corrupted with cycle or null node triggers Red Ring critical; finish-switch not executed after switch-to triggers Red Ring critical; CPU going offline with a PID in its current slot causes migration to another CPU and Red Ring if migration fails; all CPUs in scheduler simultaneously with no Ready process and no idle causes ProgressContract to detect livelock and SAIRU to produce a diagnosis; scheduler stall with no forward progress causes ProgressContract to detect and emit SCHED_STALL with SAIRU diagnosis; SIGKILL to idle is silently dropped because idle is immortal.

Priority Inversion: occurs when a high-priority process is blocked on a lock held by a low-priority process that cannot be scheduled. ProgressContract monitors high-priority processes blocked beyond threshold. SchedulerContract resolves via priority inheritance where the lock holder temporarily inherits the priority of the highest-priority waiter. KDS records PRIORITY_INVERSION_DETECTED with blocked PID, lock owner PID, and inherited priority.

Starvation: occurs when a Ready process receives no CPU time beyond the starvation threshold. ProgressContract detects it. SchedulerContract resolves via aging where starved processes receive a temporary priority boost. KDS records SCHEDULER_STARVATION with PID, wait duration, and boost applied.

### INTERRUPTCONTRACT

Ownership: IDT adapter entry, fault and IRQ classification, end-of-interrupt policy, scheduler handoff, and exception recovery.

Invariants: Every IDT handler completes end-of-interrupt before returning. Faults that cannot be recovered deliver a signal to the process or trigger Red Ring — never silently ignored. NMI handlers never acquire any lock because they are the delivery mechanism for Red Ring broadcast.

Failure Modes: double fault triggers Red Ring; triple fault causes hardware CPU reset and SAIRU reads last KDS state post-reset; spurious interrupt during CR3 update handled safely because CR3 update is atomic at hardware level; NMI during contract lock held with interrupts disabled handled safely because NMI handler reads only per-CPU data with no lock acquisition; IRQ storm with all CPUs saturated detected by ProgressContract which emits IRQ_STORM and SAIRU produces a diagnosis with rate limiting applied; MCE in user frame causes frame poisoning, SIGBUS delivery, and permanent blacklisting; MCE in kernel frame is non-recoverable and triggers Red Ring; page fault in kernel context with invalid kernel pointer triggers Red Ring; page fault in user context attempts resolution and delivers SIGSEGV on failure.

### SYSCALLCONTRACT

Ownership: syscall entry validation, dispatch, signal processing, outcome selection, syscall exit, and per-CPU syscall state.

Invariants: On entry, GS is kernel-active, GS segment offset zero points to this CPU's syscall state, current PID resolves through ExecutionContract, and saved user frame is complete. On exit, return image is canonical, pending signals are processed exactly once, GS-active state matches chosen return path, and kernel stack mirrors the current process. Every syscall has exactly one return path — either sysret or iretq — never both, never neither.

Failure Modes: GS not kernel-active on entry is a security violation; SyscallContract sends SIGKILL and emits audit event. Incomplete saved user frame triggers Red Ring. Signal processed twice on the same syscall exit triggers Red Ring high. Syscall number out of range returns ENOSYS. Non-returning syscall path exiting is intended process termination. Syscall invoked from kernel context triggers Red Ring if detected.

Portability: On 64-bit systems SyscallContract configures LSTAR, CSTAR, SFMASK, and related MSRs for the syscall and sysret instructions. On 32-bit systems including Pentium 4, SyscallContract configures the INT 0x80 vector as the syscall entry point. Both paths converge on the same internal dispatch logic after saving the user frame.

### DRIVERCONTRACT

Ownership: driver registration, driver lifecycle, resource attribution, driver telemetry, and driver diagnostics.

Lifecycle States: Unregistered, Registered, Initialised, Started, optionally Suspended, Stopped, Unregistered. A driver must register with the Device Registry before any other lifecycle transition. A driver that fails initialisation never reaches Started. A driver emits DRIVER_REGISTER, DRIVER_START, DRIVER_STOP, DRIVER_ERROR, and DRIVER_RESET KDS events at every corresponding lifecycle boundary.

Failure Modes: driver emitting a KDS event from interrupt context requiring sleep must use the lock-free KDS write path; driver calling VfsContract from its own teardown is forbidden and creates circular dependencies — all VfsContract handles must be released before teardown initiates; DMA writes with corrupted data into kernel buffer are blocked at hardware level by IOMMU restricted to the driver's registered memory region with a KDS event; driver failing to stop within timeout is force-stopped, marked offline, emits KDS event, and SAIRU produces a diagnosis; driver registering a duplicate device ID is rejected and the existing driver is unaffected.

### VFSCONTRACT

Ownership: namespace selection, path resolution, permission policy, mount operations, and inode dispatch.

Invariants: Every permission check is performed by the VfsContract and never by the filesystem implementation. Path resolution never crosses namespace boundaries without explicit authorisation. Mount operations are atomic — a partial mount is not visible to any caller.

Failure Modes: bypassed permission check triggers Red Ring high and emits audit event; path resolution crossing a namespace without authorisation returns EPERM and emits audit event; filesystem returning inode not matching the path returns ESTALE and emits KDS event; partial mount with process crash causes VfsContract to roll back the mount; symlink cycle detected returns ELOOP after configurable follow limit (default 40); path component exceeding NAME_MAX returns ENAMETOOLONG.

Filesystem Intelligence: For ext4, monitors journal state, detects fsck precursors, and scores fragmentation. For XFS, monitors log buffers, tracks inode clusters, and detects metadata contention. For Btrfs, tracks COW fragmentation, balance status, and subvolume I/O attribution. For tmpfs, correlates memory pressure and tracks eviction. For overlayfs, attributes container layer I/O and tracks layer merge cost. For NFS and CIFS, attributes network-induced latency and detects stale mounts.

### OBSERVABILITYCONTRACT

Ownership: KDS event schema, telemetry tiers, aggregate providers, trace correlation, diagnostic outputs, resource-attribution evidence, validation evidence, and freeze-recorder inputs.

Core Rule: The ObservabilityContract observes. It never repairs state by side effect. A diagnostic output is never authoritative for state — the owning contract is. This is the fundamental separation between understanding and execution.

Failure Modes: ring buffer full for non-CRITICAL event drops the event and emits a KDS overflow metric; ring buffer full for CRITICAL event blocks up to overflow timeout then emits KDS_CRITICAL_LOSS and triggers Red Ring diagnostic; schema version mismatch on read rejects the event, emits schema error metric, and never corrupts existing records; diagnostic output contradicting contract state acknowledges the diagnostic output is wrong while the contract state is canonical and emits DIAGNOSTIC_MISMATCH; observability path causing side effect on kernel state triggers Red Ring from any detecting contract.

### PROGRESSCONTRACT

Ownership: cross-subsystem progress attribution — distinguishing healthy high-utilisation work from stalls with no scheduler, KDS, queue, or subsystem forward progress.

Monitored Signals with Thresholds and KDS Events: scheduler forward progress (any process advancing) — threshold 5 seconds — emits SCHED_STALL; KDS write throughput with non-empty queue — threshold 30 seconds — emits KDS_WRITE_STALL; priority inversion duration — threshold 500 milliseconds — emits PRIORITY_INVERSION_DETECTED; process starvation (Ready but unscheduled) — threshold 10 seconds — emits SCHEDULER_STARVATION; OOM pressure trend (memory above 85% for 60 seconds) — emits OOM_PRESSURE_TREND; driver initialisation timeout — threshold 30 seconds — emits DRIVER_INIT_TIMEOUT; IRQ storm (CPU utilisation by IRQs above 80%) — threshold 5 seconds — emits IRQ_STORM.

Non-Intervention: The ProgressContract never intervenes. It emits KDS evidence. SAIRU reads that evidence and produces diagnosis and guidance. Intervention requires human approval. This is a deliberate design choice: automated intervention without human approval has caused more outages than it has prevented.

### RELIABILITYCONTRACT AND THE RED RING

Ownership: lock acquisition order and contract violation detection.

Global Lock Acquisition Order (Deadlock Prevention Law): Every multi-contract transition must acquire locks in this order with no exceptions. Acquiring a lower-priority lock while holding a higher-priority lock is permitted. The reverse is forbidden and is itself a contract violation.
Priority 1 — ObservabilityContract (acquired first)
Priority 2 — DiagnosticsContract
Priority 3 — ResourceContract
Priority 4 — ExplainabilityContract
Priority 5 — ReliabilityContract
Priority 6 — SecurityContract
Priority 7 — CompatibilityContract
Priority 8 — PerformanceContract
Priority 9 — UXContract
Priority 10 — CosmeticsContract (acquired last)

Constitutional contracts (ExecutionContract, ProcessContract, SchedulerContract, SyscallContract, MemoryContract, AddressSpaceContract, InterruptContract, DriverContract, VfsContract) are interrupt-context contracts that never sleep, never block on a lower-priority lock, never allocate memory while holding a lock, and acquire KDS write access through the lock-free per-CPU path only.

Lock Order Validation: At boot, before any subsystem initialises, the lock order is validated by the ReliabilityContract. Any registered lock acquisition that would violate the order causes boot to halt with a diagnostic before the first user process starts.

The Red Ring: The Red Ring is the SAIOS signal that the kernel has encountered a condition it cannot safely recover from. It is not an error handler. It is a controlled halt with maximum evidence preservation.

Trigger Conditions: any contract invariant violation as defined in the Constitution; any kernel panic; any non-recoverable hardware fault (MCE, double fault, triple fault); lock acquisition order violation detected at runtime; KDS reserved memory region accessed by a non-KDS path; SAIRU Policy Engine rejecting a proposed action and the calling path attempting to proceed anyway; any subsystem attempting to directly mutate canonical state owned by another contract.

Red Ring Sequence:
Step 1 — Detection: the detecting contract calls the ReliabilityContract red-ring entry point with the cause and evidence event ID; this is the only valid Red Ring entry point.
Step 2 — Broadcast Halt: the ReliabilityContract sends NMI to all CPUs; each CPU completes its current in-flight KDS write if one is active, halts all execution below the KDS write path, does not acquire any new locks, and does not service any further interrupts except NMI.
Step 3 — KDS Seal: the ObservabilityContract seals the KDS by marking it as post-halt read-only, emitting one final RED_RING_SEALED event with timestamp, trigger cause, triggering CPU, triggering PID, and evidence event ID, and accepting no further writes.
Step 4 — SAIRU Activation: SAIRU activates on the sealed KDS without starting a new kernel execution context; SAIRU reads the KDS using its physically reserved memory and its own execution path established at boot independently of the kernel scheduler.
Step 5 — Red Ring Display: the UX layer displays the Red Ring signal; SAIRU's Context Engine begins reconstructing system state from KDS history; SAIRU's Knowledge Engine builds the causal chain to the trigger event; SAIRU's Diagnosis Engine produces a confidence-scored explanation; all output is available for human query.
Step 6 — Human Query Surface: SAIRU answers questions about what the system was doing, what failed and why, the causal chain of events, and what should be done to prevent recurrence; no action is taken without human approval; SAIRU never self-recovers from a Red Ring.

What Stays Alive After Red Ring: KDS in read mode stays alive because it is in physically reserved memory with an independent read path. SAIRU stays alive because it was established at boot on an independent execution path. The Red Ring display stays alive because it is driven by SAIRU, not the kernel scheduler. All kernel contracts, all user processes, all drivers, the scheduler, and the network stack are halted.

What SAIRU Produces Post-Red Ring: the trigger (contract violation, kernel panic, or hardware fault), the owning contract that detected the violation, the specific invariant violated, the KDS event ID of the triggering event, timestamp in nanoseconds, CPU ID at time of trigger, PID in context, and a confidence score from 0 to 100%. The causal chain as an ordered list of event IDs with descriptions. A human-readable root cause explanation. A list of affected contracts and subsystems in the causal chain. An ordered list of specific actionable recommended steps. A prevention recommendation describing what contract or validation change would prevent recurrence. Full KDS history is available for query.

### SECURITYCONTRACT

Principles: Security is non-negotiable. Observability never weakens security boundaries. All intelligence data access is subject to capability-based access control. AI models are consumers of intelligence — they never influence kernel policy directly. The principle of least privilege applies to all kernel services including intelligence services. Telemetry data is classified and some fields are restricted to privileged consumers. The AI Gateway enforces authentication and authorisation before any query executes. Sensitive payload fields such as process arguments and network data are encrypted at rest in the Flight Recorder. Security events are CRITICAL severity and never dropped. Diagnostic recommendations never include actions that would weaken security posture.

Security Monitoring Events: SECURITY_SYSCALL_DENIED — mandatory access control or RBAC denied syscall with PID, syscall number, policy ID, and action. SECURITY_PRIVILEGE_ESCALATION — setuid, capability grant, and namespace enter attempts. SECURITY_NAMESPACE_ESCAPE — container namespace violation detection. SECURITY_INTEGRITY_VIOLATION — IMA and EVM integrity check failure. SECURITY_AUDIT_EXEC — execution of a binary with the audit flag set. SECURITY_NETWORK_POLICY_DENY — network egress or ingress policy violation.

## PART III — NUMA-AWARE ARCHITECTURE

### NUMA TOPOLOGY DISCOVERY

SAIOS discovers NUMA topology at Gate 1 (HAL Initialised) via ACPI SRAT (System Resource Affinity Table) and SLIT (System Locality Information Table) on UEFI systems, CPUID topology enumeration as fallback, and runtime re-query after hotplug events. The HAL exposes a NumaTopology structure containing: node count, per-node CPU mask, per-node physical memory ranges, node-to-node distance matrix (from SLIT, units are arbitrary but comparable), and per-node online state. On Tier 0 and Tier 1 processors without NUMA, the topology reports one node covering all CPUs and all memory. All NUMA-aware subsystems must gracefully handle the single-node case without special-casing.

KDS events emitted during topology discovery: NUMA_TOPOLOGY_DISCOVERED with node count, total CPU count, and whether SLIT data is available. NUMA_NODE_MEMORY with node ID, memory range base, and memory range size, one event per node. NUMA_NODE_CPU with node ID and CPU mask, one event per node.

### NUMA SCHEDULER POLICY

The SchedulerContract extends CFS with NUMA balancing. Policy: a process is local to the NUMA node where it was last scheduled. The scheduler prefers to run a process on a CPU in its local node. If all local CPUs are saturated and a remote CPU is idle, the scheduler may migrate, but emits NUMA_REMOTE_SCHEDULE with PID, from-node, to-node, and reason. The NUMA balancer runs as a kernel thread per node. It periodically scans the run queues of all nodes and identifies imbalances — defined as a local-to-remote load ratio exceeding a configurable threshold (default 1.25). When imbalance is detected, it migrates processes from overloaded nodes to underloaded nodes, preferring processes whose working set is already cached on the target node (determined by scanning PTE access bits). The balancer emits NUMA_REBALANCE with from-node, to-node, process count migrated, and load delta.

Priority: scheduler invariants (Invariants 1, 2, 3) take absolute precedence over NUMA preference. A process never remains on a NUMA-wrong CPU if a NUMA-correct CPU is idle and the migration would not violate a scheduling class constraint. NUMA balancing is suspended during Red Ring.

### NUMA MEMORY POLICY AND BALANCING

Default policy for all new allocations is NUMA_LOCAL — allocate frames from the node where the requesting CPU resides. Additional policies configurable per address-space range: NUMA_BIND — restrict to a specific node set; NUMA_INTERLEAVE — round-robin across a node set for bandwidth-intensive workloads; NUMA_PREFERRED — prefer a node but fall back to others if the preferred node is out of frames.

Policy is stored in the address-space as a per-VMA (Virtual Memory Area) attribute. VMA inherits the process-level default policy unless explicitly overridden. KDS event MEMORY_NUMA_POLICY_SET emitted on any policy change with PID, VMA base, VMA size, and new policy.

NUMA Balancing (Memory Migration): The NUMA balancer thread also performs memory page migration. When a process has a stable NUMA affinity (determined by observing scheduling history over a configurable window, default 100ms), the balancer checks whether the process's pages are resident on its preferred node. Pages resident on remote nodes are candidates for migration. Migration procedure: the balancer locks the PTE, issues a NUMA fault to pause the process on that page, allocates a new frame on the local node, copies the page content, updates the PTE to point to the new frame, releases the old frame, and unlocks. The process resumes. KDS event NUMA_PAGE_MIGRATED emitted with PID, old-node, new-node, page count, and migration latency.

### NUMA MIGRATION AND LOCALITY METRICS

SAIRU continuously tracks the NUMA locality score for each process — defined as the fraction of memory accesses that hit the process's preferred node. Score is computed from hardware performance counters (if available on Tier 2 and above) or approximated from PTE access bit scanning (Tier 0 and Tier 1). Score range: 0.0 (all accesses remote) to 1.0 (all accesses local). If a process's locality score falls below a configurable threshold (default 0.6) for a sustained period (default 5 seconds), SAIRU proactively recommends migration of both the process's memory and scheduling affinity. KDS event NUMA_LOCALITY_DEGRADED emitted with PID, current score, threshold, and sustained duration.

### NUMA FAILURE MODES

If NUMA topology discovery produces inconsistent data (CPU claims to be on node N but node N's CPU mask does not include it), the HAL emits NUMA_TOPOLOGY_INCONSISTENT, treats the system as UMA for safety, and SAIRU produces a diagnosis flagging the inconsistency as a firmware bug. If a NUMA node goes offline (hotplug remove), the SchedulerContract migrates all processes off that node's CPUs, the MemoryContract migrates all frames from that node to remaining nodes, and KDS emits NUMA_NODE_OFFLINE with the node ID. If NUMA migration fails (destination node out of frames), migration is abandoned, KDS emits NUMA_MIGRATION_FAILED with the reason, and the process continues running non-locally with a degraded locality score. If the NUMA balancer thread itself stalls, the ProgressContract detects it via the scheduler forward progress monitor and SAIRU produces a diagnosis.

## PART IV — DEVICE MODEL AND DRIVER ARCHITECTURE

### DEVICE MODEL HIERARCHY

Every physical and virtual device in SAIOS is represented in a unified device model. The hierarchy is:

Bus → Device → Driver

A Bus represents a communication fabric (PCI, USB, I2C, SPI, Platform). A Device represents a single addressable entity on a bus. A Driver is the software component that controls a Device and exposes its capabilities to the kernel and userspace.

Device Structure: each Device has a device ID (globally unique, assigned at registration), a device class (network, storage, graphics, audio, HID, serial, generic), a bus type, a bus address (PCI BDF, USB path, etc.), a parent device ID (for hierarchical devices), a device state (Absent, Present, Claimed, Active, Suspended, Faulted, Removed), resource list (IRQ lines, MMIO ranges, I/O ports, DMA channels), power state, telemetry handle, and a driver binding (null if unbound).

### DEVICE CONTRACT

DeviceContract Ownership: device registration, device state machine, resource arbitration, driver binding, power state management, device telemetry, and hotplug coordination.

Device Registration: A driver must call DeviceContract::register with a complete DeviceDescriptor before any other operation. The DeviceDescriptor includes device class, bus type, bus address, human-readable name and model, firmware version, resource requirements, power capabilities, and telemetry categories the device will emit. Registration is atomic — either the device is fully registered or it is not registered. No partial registration state is visible. On success, KDS emits DEVICE_REGISTERED with device ID, class, bus type, and bus address.

Device State Machine: Absent → Present (device detected on bus). Present → Claimed (driver matched and bound). Claimed → Active (driver fully initialised). Active → Suspended (driver acknowledged power-down). Suspended → Active (driver acknowledged power-up). Active → Faulted (driver reported unrecoverable error). Faulted → Active (driver reset and recovered, with SAIRU approval). Active → Removed (hotplug removal detected). Removed is terminal — a removed device ID is never reused. KDS event emitted on every state transition.

### BUS ARCHITECTURE

Every bus type implements a BusContract. BusContract defines: scan — enumerate all devices present on the bus and return their bus addresses; match — given a device descriptor, determine if a given driver module can control it; probe — initiate driver binding for a specific device; remove — initiate driver unbinding for a specific device. PCI bus: devices are enumerated by scanning bus/device/function tuples. MSI and MSI-X are preferred over legacy IRQ lines for devices that support them. PCIe AER (Advanced Error Reporting) events are captured and forwarded to the device's fault handler. USB bus: devices are enumerated via hub enumeration. USB descriptors are parsed to determine device class and capabilities. Each USB device is assigned a stable device ID based on its bus address and serial number. Platform bus: devices are enumerated via ACPI or device tree. Platform devices are non-discoverable and must be described by firmware.

### RESOURCE MANAGEMENT

Resources (IRQ lines, MMIO ranges, I/O ports, DMA channels) are managed by the DeviceContract ResourceManager. Allocation: a driver requests resources via DeviceContract::allocate_resources. The ResourceManager validates that requested resources do not overlap with already-allocated resources or with kernel-reserved regions. On success, the resources are marked as owned by the device ID and returned to the driver. On failure, KDS emits RESOURCE_ALLOCATION_FAILED with device ID, resource type, and reason, and the driver is not bound. Release: resources are automatically released when the device transitions to Removed state. A driver may not release its own resources before device removal — premature resource release triggers Red Ring high. IOMMU Integration: for DMA-capable devices, the DeviceContract programs the IOMMU with the device's allowed DMA ranges on binding and revokes them on removal. Any DMA outside the allowed range is blocked by hardware and generates IOMMU_FAULT in KDS.

### POWER STATE MANAGEMENT

Device power states: D0 (fully on), D1 (light sleep), D2 (deeper sleep), D3hot (power-down preserving bus context), D3cold (power completely removed). Power state transitions are coordinated by DeviceContract in cooperation with a system-wide PowerManager. The PowerManager queries all devices for their minimum acceptable power state given current system load before issuing a system-wide power transition. A device may veto a power state transition by returning an error from its suspend callback. Veto causes KDS event POWER_VETO with device ID, requested state, and reason. SAIRU tracks power state history per device and can diagnose spurious wakeups and power regression across software updates.

### DEVICE TELEMETRY

Every device registered with the DeviceContract automatically receives a telemetry handle. The telemetry handle is pre-allocated from KDS reserved memory at device registration time. Device drivers emit telemetry through the handle using the same lock-free per-CPU ring buffer mechanism as kernel contracts. Telemetry categories per device: error counters (ECC errors, CRC failures, timeout counts), performance metrics (throughput, latency histograms, queue depths), health indicators (temperature, voltage, wear level for NVMe), and state events (link up/down, power state changes, resets). Telemetry is queryable by SAIRU and by userspace via the intelligence query interface. Telemetry history is retained in the Flight Recorder.

## PART V — EVENT CORRELATION ENGINE

### CORRELATION ENGINE OVERVIEW

The Event Correlation Engine (CE) is a subsystem of SAIRU. It consumes the continuous stream of KDS events and maintains a live causal model of system behaviour. The CE is not part of the kernel scheduler or execution path — it runs on the SAIRU reserved core and reads KDS via the independent SAIRU read path. The CE has two persistent outputs: the Knowledge Graph Service (KGS), which is the live queryable model of machine state and relationships, and the causal chain log, which records every inferred causal link between events with associated confidence scores.

The CE operates in three phases: Ingestion — raw KDS events are consumed from per-CPU ring buffers in timestamp order, deduplicated by event ID, and normalised into a common internal representation. Correlation — events are matched against a library of correlation rules. Each rule specifies: antecedent event pattern (matching on event type, severity, payload fields, and context tags), consequent event pattern (similarly specified), maximum temporal window between antecedent and consequent, minimum confidence threshold, causal relationship type (causes, enables, precedes, concurrent-with). Analysis — correlated event pairs are assembled into causal chains and inserted into the Knowledge Graph.

### KNOWLEDGE GRAPH SERVICE

The KGS is an in-memory directed graph. Nodes represent entities: processes, devices, files, network connections, memory regions, kernel subsystems, users, and time intervals. Edges represent relationships: caused, used, accessed, blocked-by, depends-on, produced, consumed, and co-occurred-with. Edges carry a confidence score (0.0 to 1.0) and a timestamp range.

Node Types and Properties:
ProcessNode — PID, executable path, parent PID, start time, end time (if terminated), CPU time, memory peak, NUMA node, SAIRU-computed risk score.
DeviceNode — device ID, class, driver, state, health score derived from telemetry.
FileNode — inode, path, filesystem, size, access pattern (sequential/random).
NetworkConnectionNode — source IP:port, destination IP:port, protocol, state, latency percentile, error rate.
MemoryRegionNode — virtual address range, owner PID, type (stack, heap, code, mmap), COW depth, NUMA node.
SubsystemNode — contract name, current state, last event ID.
UserNode — UID, GID, active session, resource consumption.
TimeIntervalNode — start timestamp, end timestamp, label, events in interval.

Edge Types: CAUSED — A directly caused B with confidence C. ENABLED — A's existence enabled B to occur. BLOCKED_BY — A is blocked waiting for B to release a resource. DEPENDS_ON — A requires B to function. PRODUCED — A produced or created B. CONSUMED — A consumed B (e.g., process consuming memory frame). CO_OCCURRED_WITH — A and B happened in the same time window without clear causation.

### CAUSAL CHAIN CONSTRUCTION

When the CE detects a high-severity event (ERROR or CRITICAL), it triggers a backwards causal chain search from that event. The search walks edges in the KGS backwards from the trigger event, following CAUSED, ENABLED, and DEPENDS_ON edges. The search is breadth-first with a depth limit (default 20 hops). At each step, the chain confidence is the product of all edge confidences along the path. Chains with confidence below 0.1 are pruned. The result is an ordered list of (event ID, description, confidence) tuples representing the most likely causal sequence leading to the trigger event.

### CONFIDENCE SCORING ALGORITHM

Each correlation rule carries a base confidence derived from empirical or heuristic knowledge. The CE adjusts base confidence at runtime using: temporal proximity (events closer in time score higher, with a half-life of 1 second for most rule types), co-occurrence frequency (rule matches that occur repeatedly in similar contexts score higher over time via a rolling Bayesian update), and contradiction penalty (if an event would simultaneously support two mutually exclusive causal hypotheses, both confidences are reduced proportionally). Confidence is stored as a 16-bit fixed-point number (range 0.0 to 1.0, resolution 0.0000152) to avoid floating-point in the CE hot path.

### EVENT RELATIONSHIP MODEL

The CE maintains a set of built-in correlation rules for all contracts. Examples:

OOM-kill-precursor chain: MEMORY_ALLOC events with increasing size trend over 30 seconds → OOM_PRESSURE → OOM_KILL. Confidence: 0.85 if trend is monotonic, 0.60 if bursty.

Scheduler stall from IRQ storm: IRQ_HANDLER events from one device at >10,000/sec for >5 seconds → SCHED_STALL. Confidence: 0.90.

Process crash from memory corruption: MEMORY_ALLOC followed by PAGE_FAULT (fault type: protection violation) on the same address range within 1 second, followed by PROCESS_TERMINATE (signal: SIGSEGV). Confidence: 0.75.

Network congestion from storage pressure: FS_WRITE stall duration >500ms on NFS mount → NET_CONGESTION on same network interface. Confidence: 0.55.

Driver fault from hardware error: IOMMU_FAULT for device D → DRIVER_ERROR for device D within 100ms. Confidence: 0.92.

Custom rules can be added at runtime via the intelligence query interface. Custom rules are subject to validation by the CE (checking for circular causation and minimum evidence requirements) and are assigned lower base confidence than built-in rules until empirical data accumulates.

### QUERY INTERFACE — SGQL

SAIOS Graph Query Language (SGQL) is a Cypher-inspired query language for the KGS. SGQL queries are submitted via the intelligence query interface by userspace processes with appropriate capabilities or by SAIRU's own engines.

Example queries:

Find all processes that caused network errors in the last 5 minutes:
MATCH (p:Process)-[:CAUSED]->(e:Event {type: "NET_ERROR"}) WHERE e.timestamp > NOW() - 300s RETURN p.pid, p.executable, COUNT(e) ORDER BY COUNT(e) DESC

Find causal chain for a specific event:
MATCH path = (e:Event {id: "uuid-here"})<-[:CAUSED*1..20]-(root:Event) RETURN path, REDUCE(conf = 1.0, r IN RELATIONSHIPS(path) | conf * r.confidence) AS chain_confidence ORDER BY chain_confidence DESC LIMIT 1

Find processes blocked by a specific device:
MATCH (p:Process)-[:BLOCKED_BY]->(d:Device {id: "device-id-here"}) RETURN p.pid, p.executable, d.device_class

SGQL queries are parsed into a logical plan by the SGQL Parser, optimised by the Query Optimiser (which uses KGS node cardinality estimates), and executed by the Graph Executor against the in-memory KGS. Query results are returned as JSON. Queries that would access restricted telemetry fields are rejected by the SecurityContract capability check before execution.

## PART VI — COMPATIBILITY ARCHITECTURE

### LINUX ABI COMPATIBILITY

The SAIOS compatibility architecture is the bridge between the intelligence-native SAIOS kernel and the enormous ecosystem of existing Linux, POSIX, and Windows software. Without compatibility, SAIOS is a hobby OS. With compatibility, SAIOS is deployable. The compatibility architecture does not compromise the SAIOS kernel design — it is an additive layer, not a replacement.

Linux ABI compatibility means SAIOS can run unmodified Linux binaries compiled for x86 and x86-64. This is achieved by implementing the Linux system call interface via the SyscallContract. The SyscallContract maintains a Linux syscall dispatch table mapping Linux syscall numbers to SAIOS internal implementations. SAIOS implements the full Linux syscall ABI as specified by the Linux kernel for x86-64 (amd64), including the calling convention (syscall number in RAX, arguments in RDI, RSI, RDX, R10, R8, R9, return value in RAX, error in negative RAX). Linux-specific system calls that have no direct SAIOS equivalent are emulated via compatibility shims. Shim coverage priority: process management (clone, fork, vfork, execve, wait4, exit), memory management (mmap, munmap, mprotect, mremap, brk, madvise), file operations (open, read, write, close, stat, fcntl, ioctl), networking (socket, bind, connect, send, recv, poll, epoll), signals (kill, sigaction, sigprocmask), and synchronisation (futex, pipe, eventfd, timerfd).

### POSIX COMPLIANCE LAYER

SAIOS provides a POSIX compliance layer that implements the interfaces required by Single Unix Specification v4 and POSIX.1-2017. This layer sits between the Linux ABI compatibility layer and the SAIOS kernel contracts. POSIX-required behaviours that are implied but not explicitly in the syscall ABI (such as signal delivery ordering, timer semantics, and process group behaviour) are implemented here. The POSIX compliance layer emits KDS events at every POSIX-mandated state transition to ensure SAIRU can explain POSIX-level behaviour (signal delivery, process group changes, session creation) as part of causal chains.

### ELF LOADER

The ELF Loader is responsible for loading and executing ELF binaries for both native SAIOS and Linux-ABI-compatible execution. ELF64 and ELF32 are supported. Loading procedure: validate the ELF magic number and architecture; parse the program headers and identify LOAD, INTERP, TLS, GNU_STACK, and GNU_RELRO segments; create a new address space via AddressSpaceContract; map LOAD segments with appropriate permissions (R, RW, RX) using MemoryContract; if an INTERP segment is present, load the dynamic linker; set up the initial stack with argc, argv, envp, and the auxiliary vector (AT_PHDR, AT_PHENT, AT_PHNUM, AT_BASE, AT_ENTRY, AT_UID, AT_GID, AT_RANDOM, AT_HWCAP, AT_PAGESZ, AT_CLKTCK); transfer control to the entry point or dynamic linker. KDS event PROCESS_EXEC emitted with PID, executable path, ELF architecture, interpreter path (if dynamic), and memory layout.

### CONTAINER SUPPORT

SAIOS provides native container support through namespace isolation. Namespace types: PID namespace (isolated process ID numbering), network namespace (isolated network stack), mount namespace (isolated filesystem view), UTS namespace (isolated hostname and domain name), IPC namespace (isolated System V IPC and POSIX message queues), user namespace (isolated UID and GID mappings), cgroup namespace (isolated cgroup hierarchy view). Namespace creation is gated by the SecurityContract — creating user namespaces requires either root privileges or explicit user namespace policy approval. Container-specific KDS events: CONTAINER_CREATE (with container ID, root PID, and namespace set), CONTAINER_DESTROY (with container ID and exit status), SECURITY_NAMESPACE_ESCAPE (if a process attempts to access resources outside its namespace). The CE has built-in correlation rules for container-level analysis: resource pressure in a container's cgroup namespace is correlated with memory and CPU events to produce container-granular health scores queryable via SGQL.

Container image support uses the overlayfs filesystem (managed by VfsContract with overlayfs intelligence) to layer container images. OCI (Open Container Initiative) image format is the native container image format.

### WINDOWS COMPATIBILITY LAYER

The Windows Compatibility Layer (WCL) is a future subsystem that enables execution of Windows PE (Portable Executable) binaries on SAIOS. WCL is not required for SAIOS v1.0. It is defined here to ensure the architecture does not preclude it. WCL implementation approach: a PE loader analogous to the ELF Loader that maps PE sections into a new address space; a Win32 API translation layer that maps Win32 API calls to SAIOS equivalents (analogous to Wine on Linux); a Windows NT kernel API emulation layer for system calls issued directly by the NTDLL (analogous to the Linux-ABI compatibility layer); a Windows filesystem namespace translator mapping Windows drive letters and path separators to SAIOS VFS paths. All WCL execution is tracked in KDS at the same granularity as native Linux ABI execution. SAIRU can produce causal chain analyses spanning both Windows-compatibility and native execution.

### PACKAGE ECOSYSTEM STRATEGY

SAIOS supports the following package ecosystems: Debian/Ubuntu packages (deb format) via a compatibility layer that extracts package contents and installs them into the VFS without requiring the Debian packaging tools to run with elevated privileges; RPM packages (rpm format) similarly; Flatpak and AppImage for application-level sandboxed distribution; native SAIOS packages in a yet-to-be-defined SAIOS package format (SPKG) that takes advantage of SAIOS-native KDS integration and intelligence metadata. The SAIOS package manager (saipkg) is a future deliverable. Its design principle: every package installation, removal, and update operation must be fully reversible and must emit KDS events so SAIRU can correlate system behaviour changes with package changes. Specifically: SAIRU should be able to answer "which package update caused this performance regression or this crash."

## PART VII — RESOURCE ACCOUNTING FRAMEWORK

### ACCOUNTING PRINCIPLES

The Resource Accounting Framework (RAF) is a cross-cutting subsystem that tracks the consumption of every accountable resource by every accountable entity. Accountable resources: CPU time, memory (physical frames, virtual mappings, swap), network bandwidth (ingress and egress bytes, packets, connections), storage I/O (bytes read, bytes written, IOPS, latency), and power (thermal design power draw, RAPL energy counters where available). Accountable entities: individual processes (PIDs), containers (cgroup hierarchies), users (UIDs), services (service unit identifiers), and the kernel itself. The fundamental accounting invariant: the sum of resource consumption by all entities must equal the total system resource consumption at all times. If this invariant is violated, the RAF emits ACCOUNTING_INVARIANT_VIOLATED and SAIRU produces a diagnosis identifying the unaccounted consumption.

### CPU ACCOUNTING

CPU time is tracked per PID per CPU using TSC deltas. On context switch, the SchedulerContract records the TSC value at switch-out and the TSC value at switch-in. The delta is accumulated in the per-PID CPU accounting structure. CPU accounting structures are per-CPU to avoid contention. Aggregation to per-process, per-container, and per-user totals is performed by the RAF aggregation thread which runs on a low-priority kernel thread. CPU accounting at nanosecond precision is available on Tier 2 and above (invariant TSC). On Tier 0 and Tier 1, microsecond precision using the PIT or HPET is used as fallback. KDS event CPU_ACCOUNT_PERIOD emitted every accounting period (default 1 second) per active entity with PID or entity ID, user time, system time, steal time, IRQ time, and voluntary and involuntary context switch counts.

### MEMORY ACCOUNTING

Memory accounting tracks physical frame ownership via the MemoryContract frame ownership mechanism. Each physical frame has a single owner — the accounting entity whose virtual address space contains a mapping to that frame. Shared frames (COW, shared memory) are attributed fractionally: each mapping entity is attributed 1/N of the frame where N is the number of current mappings. Proportional Set Size (PSS) is the standard attribution metric. Additionally, the RAF tracks: resident set size (RSS — frames currently in RAM), virtual set size (VSS — virtual address space size including swap-backed pages), swap consumption (frames evicted to swap attributable to each entity), and kernel memory attribution (slab objects allocated on behalf of a process are attributed to that process). KDS event MEMORY_ACCOUNT_PERIOD emitted per active entity per accounting period with RSS, PSS, VSS, swap, and kernel memory.

### NETWORK ACCOUNTING

Network accounting is implemented at two levels: socket level (per-PID, tracking bytes sent and received per socket) and interface level (per-network-interface, tracking total bytes, packets, errors, and drops). Socket-level attribution is performed by the networking stack when a packet is queued for send or accepted from receive. Interface-level totals are read from network interface hardware counters where available. The RAF reconciles socket-level attribution with interface-level totals to identify unattributed traffic (e.g., kernel-generated traffic such as ARP, ICMP). KDS event NETWORK_ACCOUNT_PERIOD emitted per active socket per accounting period with PID, socket tuple, bytes sent, bytes received, packets sent, packets received, retransmit count, and latency percentiles.

### POWER AND THERMAL ACCOUNTING

On processors with RAPL (Running Average Power Limit) support (Tier 2 and above), SAIOS reads RAPL energy counters via MSRs to compute package-level and per-domain (core, uncore, DRAM) power consumption. Power is attributed to processes proportionally by CPU time within each RAPL measurement period. Thermal accounting tracks per-CPU and per-package temperatures via PECI or MMIO interfaces where available. If a thermal throttle event is detected (processor reducing frequency due to thermal limits), the InterruptContract emits THERMAL_THROTTLE with the affected CPU, duration, and estimated frequency reduction. SAIRU correlates thermal throttle events with process CPU time to identify thermally-expensive workloads.

### STORAGE IO ACCOUNTING

Storage I/O is tracked at the block layer. Every I/O request is tagged with the PID that initiated it. I/O attributed to the kernel on behalf of a process (e.g., readahead, writeback) is attributed to the process that owns the file. Metrics per entity per period: bytes read, bytes written, read IOPS, write IOPS, read latency percentiles (p50, p95, p99), write latency percentiles, and queue depth. KDS event STORAGE_ACCOUNT_PERIOD emitted per active entity per accounting period.

### ATTRIBUTION AND EXPLAINABILITY

Every resource consumption report produced by the RAF is accompanied by an attribution chain — a reference to the KDS events that justify the attribution. This means SAIRU can not only report that process P consumed X bytes of network bandwidth, but can reconstruct the sequence of send() calls, socket state transitions, and network events that produced that consumption. Attribution chains are the primary mechanism by which SAIOS transforms raw resource metrics into explainable intelligence. When SAIRU is asked "why is process P consuming so much CPU", the answer is not just a number — it is a causal narrative derived from KDS events, scheduler records, and RAF attribution data.

## PART VIII — INTELLIGENCE LAYER

### SAIRU — THE INTELLIGENCE INTERFACE

SAIRU is the mechanism through which SAIOS understands and explains itself. It is always present, always reading from the KDS, never interfering with normal execution. SAIRU has five responsibilities: to explain by translating KDS evidence into human-readable causal narratives; to diagnose by identifying root cause from event correlation with confidence scoring; to predict by identifying trends in KDS metrics that indicate future failure; to guide by producing recommended action plans without executing them; and to orchestrate by coordinating approved multi-step workflows through contract APIs, where subsystems execute and SAIRU coordinates.

SAIRU is not a chatbot or large language model wrapper. It is not a log viewer or monitoring dashboard. It is not an override authority. It is not a post-mortem agent that only activates at crash time. It is not a subsystem that owns execution, storage, scheduling, security, or memory.

SAIRU Authority Boundary: SAIRU may observe all KDS evidence, explain any system state, diagnose any failure, predict from any trend, recommend any action, and orchestrate approved workflows through contract APIs. SAIRU may not bypass any contract, modify kernel state directly, ignore validation gates, ignore safety constraints, or ignore subsystem ownership.

Phase One Constraint: Phase One is deterministic and model-free. SAIRU must be fully functional with no AI model installed. The Context Engine, Tool Engine, Skill Engine, Task Engine, Knowledge Engine, Planning Engine, and Policy Engine all operate on KDS evidence using deterministic logic. AI model integration is a future capability that assists SAIRU but is never required for it to function.

### SAIRU ENGINES

Context Engine: reconstructs system state at any point in time from KDS event history. Given a timestamp T and an optional scope (PID, device, subsystem), the Context Engine replays the relevant KDS event subsequence to produce a complete snapshot of system state at T. This is the primary mechanism for post-mortem analysis and for SAIRU's own internal state reconstruction.

Tool Engine: exposes contract APIs to SAIRU's orchestration layer as callable tools. Each contract that supports orchestrated actions registers a set of tools with the Tool Engine. Tools are typed (name, parameter schema, return schema), subject to Policy Engine approval before execution, and their execution produces KDS events. Examples: ProcessContract.kill(PID, signal), SchedulerContract.repin(PID, cpu_mask), DriverContract.reset(device_id), MemoryContract.reclaim(target_kb).

Skill Engine: a library of named diagnostic and recovery skill sequences. A Skill is a named sequence of Knowledge Engine queries and Tool Engine actions that collectively diagnose or resolve a class of problems. Example skills: oom_diagnosis_skill (queries memory trend, identifies largest consumers, produces attribution report), scheduler_stall_diagnosis_skill (identifies IRQ storms or priority inversions causing the stall), driver_fault_recovery_skill (resets a faulted device and re-probes the driver).

Task Engine: executes multi-step orchestrated workflows through the contract layer. A Task is an instantiation of a Skill with specific parameters and execution state. Tasks are created by user request or by SAIRU's Planning Engine. Task execution is logged step-by-step in KDS. A Task may be paused, resumed, or cancelled. Cancelled Tasks emit KDS events recording which steps completed, which did not, and the reason for cancellation.

Knowledge Engine: queries the KDS and KGS, builds causal chains (via the Correlation Engine), and scores confidence. The Knowledge Engine is the primary data access layer for all other SAIRU engines. It provides: event query (by type, severity, time range, PID, device, correlation ID), graph query (SGQL), causal chain search (as described in Part V), trend analysis (detecting monotonic or periodic patterns in event metrics over configurable time windows), and anomaly detection (flagging metrics that deviate from a rolling baseline by more than a configurable threshold).

Planning Engine: produces stepwise recovery and repair plans. Given a diagnosis produced by the Knowledge Engine and Correlation Engine, the Planning Engine selects applicable Skills from the Skill Engine and assembles them into a plan — an ordered list of steps with prerequisites, expected outcomes, and rollback actions. Plans are presented to the human operator for approval before any step is executed.

Policy Engine: validates that any proposed action respects all safety and ownership contracts before the Tool Engine executes it. Validation checks: does the action modify canonical state owned by the correct contract (ownership check); does the action require a capability the current SAIRU session has been granted (capability check); would the action create a Red Ring trigger condition (safety pre-check); is the action reversible, and if not, has human approval been explicitly confirmed (reversibility check). Any validation failure causes the proposed action to be rejected and SAIRU to present the reason to the operator.

### FLIGHT RECORDER ARCHITECTURE

The Flight Recorder (FR) is the persistent archive of KDS events. The per-CPU ring buffers in the KDS reserved region are the in-memory stage. The Flight Recorder is the durable stage. Architecture: a dedicated kernel thread (FR Daemon) runs at low priority on the SAIRU reserved core. It drains per-CPU ring buffers in round-robin order, serialises events to a compressed binary format, and writes them to a designated Flight Recorder partition or file on the fastest available durable storage (NVMe preferred, falling back to HDD, falling back to a persistent memory-mapped file on RAM). The FR Daemon writes in append-only blocks of 64KB. Each block is checksummed (CRC-32C). On Red Ring, the FR Daemon completes its current block write, writes a final block containing the sealed KDS contents, and halts. Flight Recorder data is retained for a configurable duration (default 7 days). SAIRU can query Flight Recorder data for events older than what is in the in-memory ring buffers. The Flight Recorder query interface supports the same SGQL queries as the live KGS, with the addition of time-range-based filtering. Sensitive fields in Flight Recorder data are encrypted using a per-boot key derived from the system's TPM (if available).

### DIAGNOSTIC INTELLIGENCE SUBSYSTEM

The DIS is the SAIRU subsystem responsible for producing structured diagnoses of observed failures. A diagnosis is produced whenever: a Red Ring occurs, a CRITICAL KDS event is emitted, a ProgressContract threshold is breached, or a user explicitly requests a diagnosis for a time range or entity. A structured diagnosis contains: the primary event (the triggering KDS event, with its ID, type, timestamp, and payload), the causal chain (as produced by the CE, ordered from root cause to symptom), the confidence score (the chain confidence as described in Part V), the affected entities (all PIDs, devices, and subsystems present in the causal chain), the human-readable explanation (a plain-text narrative describing what happened and why, generated deterministically from the causal chain structure and the entity metadata), the recommended actions (an ordered list of actions from the Planning Engine, each with expected outcome and reversibility flag), and the prevention recommendation (a description of what design, configuration, or validation change would prevent recurrence).

### PREDICTIVE INTELLIGENCE SUBSYSTEM

The PIS runs continuously in the background on the SAIRU reserved core. It consumes aggregated KDS metrics from the RAF and monitors them against configurable prediction models. Built-in prediction models: OOM prediction — if RSS growth rate for a process or container exceeds a threshold and projected exhaustion time is within a configurable window (default 10 minutes), SAIRU emits a predictive alert PREDICT_OOM with the entity, projected exhaustion time, and confidence score. Disk exhaustion prediction — if filesystem fill rate projects exhaustion within a configurable window (default 24 hours), SAIRU emits PREDICT_FS_FULL. TSC divergence — on multi-socket systems, if per-CPU TSC skew exceeds a threshold, SAIRU emits PREDICT_TSC_DIVERGENCE which may indicate a hardware problem. Driver health degradation — if a device's error counter rate shows a monotonically increasing trend over a configurable window (default 1 hour), SAIRU emits PREDICT_DRIVER_DEGRADED with the device ID and projected time-to-fault. Memory fragmentation — if the kernel's free frame list shows increasing fragmentation (large allocation failure rate increasing), SAIRU emits PREDICT_MEMORY_FRAGMENTATION.

### OPTIMIZATION INTELLIGENCE SUBSYSTEM

The OIS analyses system behaviour over time and produces optimisation recommendations. These recommendations are advisory — they are never automatically applied. OIS recommendations: NUMA affinity recommendation — if a process shows consistently poor NUMA locality score (below threshold for more than a configurable window), OIS recommends pinning the process to a specific NUMA node. Scheduler class recommendation — if a process shows consistent scheduling latency above a threshold but is running under the CFS class, OIS may recommend promoting it to a real-time scheduling class. Memory policy recommendation — if a process's access pattern to a memory region shows strongly sequential access, OIS recommends madvise(MADV_SEQUENTIAL) for that region. Driver interrupt affinity — if a network interface is generating IRQs that are being handled by a non-local NUMA node's CPU, OIS recommends reconfiguring IRQ affinity. Huge page recommendation — if a process's page fault rate for a specific mapping is high and the mapping is large enough to benefit from huge pages, OIS recommends enabling THP for that mapping.

## PART IX — CROSS-CUTTING CONCERNS

### CROSS-SUBSYSTEM EVENT TAXONOMY

Boot Events: BOOT_KDS_READY (KDS becomes operational), BOOT_GATE_PASSED (each successful boot gate), BOOT_GATE_FAILED (boot gate failure), BOOT_COMPLETE (all 16 gates passed and init is about to launch), NUMA_TOPOLOGY_DISCOVERED, DEVICE_REGISTERED, DRIVER_REGISTER.

Process Events: PROCESS_CREATE, PROCESS_EXEC, PROCESS_TERMINATE, PROCESS_SIGNAL (all emitted by ProcessContract at corresponding lifecycle boundaries).

Memory Events: MEMORY_ALLOC, MEMORY_FREE, PAGE_FAULT, OOM_PRESSURE, OOM_KILL, MEMORY_LEAK_DETECTED, NUMA_PAGE_MIGRATED, IOMMU_FAULT.

Scheduler Events: SCHED_CONTEXT_SWITCH, SCHED_PREEMPT, SCHED_STALL, PRIORITY_INVERSION_DETECTED, SCHEDULER_STARVATION, NUMA_REBALANCE, NUMA_REMOTE_SCHEDULE.

Network Events: NET_CONNECT, NET_ERROR, NET_CONGESTION, NETWORK_ACCOUNT_PERIOD.

Filesystem Events: FS_OPEN, FS_WRITE, FS_MOUNT, FS_ERROR.

Driver and Device Events: DRIVER_REGISTER, DRIVER_INIT, DRIVER_START, DRIVER_STOP, DRIVER_ERROR, DRIVER_RESET, DEVICE_REGISTERED, DEVICE_STATE_CHANGE, POWER_VETO, THERMAL_THROTTLE.

Interrupt Events: IRQ_HANDLER, IRQ_STORM, MCE_USER_FRAME, MCE_KERNEL_FRAME.

Security Events: SECURITY_SYSCALL_DENIED, SECURITY_PRIVILEGE_ESCALATION, SECURITY_NAMESPACE_ESCAPE, SECURITY_INTEGRITY_VIOLATION, CONTAINER_CREATE, CONTAINER_DESTROY.

Syscall Events: SYSCALL_ENTER, SYSCALL_EXIT.

Reliability Events: RED_RING_SEALED, CONTRACT_VIOLATION, LOCK_ORDER_VIOLATION.

KDS Self-Events: KDS_OVERFLOW, KDS_CRITICAL_LOSS, KDS_WRITE_STALL.

NUMA Events: NUMA_TOPOLOGY_DISCOVERED, NUMA_NODE_MEMORY, NUMA_NODE_CPU, NUMA_TOPOLOGY_INCONSISTENT, NUMA_NODE_OFFLINE, NUMA_MIGRATION_FAILED, NUMA_LOCALITY_DEGRADED.

Accounting Events: CPU_ACCOUNT_PERIOD, MEMORY_ACCOUNT_PERIOD, NETWORK_ACCOUNT_PERIOD, STORAGE_ACCOUNT_PERIOD, ACCOUNTING_INVARIANT_VIOLATED.

Intelligence Events: PREDICT_OOM, PREDICT_FS_FULL, PREDICT_TSC_DIVERGENCE, PREDICT_DRIVER_DEGRADED, PREDICT_MEMORY_FRAGMENTATION, DIAGNOSTIC_MISMATCH, NUMA_LOCALITY_DEGRADED.

### PORTABILITY RULES — PENTIUM 4 TO CORE ULTRA

Address Space: On 32-bit Pentium 4 without PAE the address space is 32 bits. On processors with PAE the address space is extended to 36 bits. On 64-bit processors the address space is 48-bit canonical. The kernel uses the usize type throughout and compiles for both 32-bit and 64-bit targets, with compile-time configuration selecting the appropriate path.

Atomic Operations: Pentium 4 supports only lock cmpxchg for atomic operations. Modern processors support cmpxchg16b and umonitor. The kernel uses the core synchronisation atomic module with fallbacks for operations not available on older processors.

Time Source: Pentium 4 TSCs may be unsynchronised across cores. Modern processors have invariant TSC plus HPET plus ART. The kernel calibrates the TSC at boot and uses the best available time source, falling back to PIT or HPET if the TSC is not reliable.

SIMD: Pentium 4 has no guaranteed SIMD beyond SSE. Core Ultra has AVX-512 and AMX. The kernel never uses SIMD in kernel core. SIMD is optional in userspace SDKs and only enabled per-function after runtime detection confirms support.

The Golden Rule Restated: If a feature is not present, the kernel degrades gracefully. It never panics. It never refuses to boot. It never silently produces incorrect results.

### IMPLEMENTATION CHECKLIST

Before writing code, the implementer must ensure: Cargo configuration uses no-standard-library and a custom target JSON. Entry point sets up the stack and disables interrupts. HAL detects CPU features before enabling advanced paths. KDS ring buffers allocated from reserved memory. First event emitted is BOOT_KDS_READY. All event structures use C representation and are schema versioned. Recursion guard implemented per CPU. Lock order validated at boot. Idle process created with PID 0. Init process slot reserved with PID 1. GDT and TSS loaded before any context switch. IDT loaded before any interrupt can fire. Syscall entry path configured before any user process runs. IOMMU enabled before any driver performs DMA. MCE handler installed before any memory is trusted. Serial console operational before any gate can fail. KDS reserved region verified before sealing. SAIRU execution path established on a reserved core before any contract initialises. NUMA topology queried before SchedulerContract initialises. DeviceContract initialised before any bus scan. RAF aggregation thread started after Gate 15. CE ingestion started after Gate 15. Flight Recorder Daemon started after Gate 7 (so KDS write path is valid).

### GLOSSARY

ADR — Architecture Decision Record. A formal documentation of a significant design choice.
AMX — Advanced Matrix Extensions. Intel's matrix acceleration instruction set.
APIC — Advanced Programmable Interrupt Controller. The modern interrupt controller replacing the legacy PIC.
AVX — Advanced Vector Extensions. Intel's SIMD instruction set extension.
BPF / eBPF — Extended Berkeley Packet Filter. In-kernel safe sandboxed program execution.
BTF — BPF Type Format. DWARF-like type information for CO-RE eBPF programs.
CE — Correlation Engine. The SAIRU component that builds causal graphs from event streams.
CFS — Completely Fair Scheduler. The Linux default CPU scheduling algorithm used as the SAIOS base scheduler.
CO-RE — Compile Once Run Everywhere. A portable eBPF compilation strategy.
COW — Copy-On-Write. The memory sharing mechanism used by fork.
CR3 — The x86 register holding the physical address of the top-level page table.
DeviceContract — The kernel contract owning device registration, state machine, resource arbitration, driver binding, power management, and device telemetry.
DIS — Diagnostic Intelligence Subsystem. The SAIRU component responsible for structured diagnosis production.
DMA — Direct Memory Access. Device-initiated memory transfers.
EIS — Event Intelligence Subsystem. The structured event emission and delivery system (KDS and per-CPU ring buffers).
FR — Flight Recorder. The persistent durable archive of KDS events.
FRTS — Flight Recorder and Telemetry Service. The FR plus the telemetry query interface.
GS — The x86 segment register used for per-CPU data and thread-local storage.
GDT — Global Descriptor Table. The x86 segment descriptor table.
HAL — Hardware Abstraction Layer. The lowest kernel tier owning all hardware-specific code.
IDT — Interrupt Descriptor Table. The x86 interrupt vector table.
IOMMU — Input-Output Memory Management Unit. Hardware that restricts device DMA to authorised memory regions.
IRQ — Interrupt Request. A hardware interrupt signal.
KDS — Knowledge Data Store. The append-only crash-safe event store that is the nervous system of SAIOS.
KGS — Knowledge Graph Service. The live in-memory directed graph of system entities and their relationships.
LSM — Linux Security Module. The kernel security framework used by SELinux and AppArmor.
MCE — Machine Check Exception. The hardware signal for CPU-detected errors.
MSR — Model-Specific Register. Processor configuration registers accessed via rdmsr and wrmsr.
NMI — Non-Maskable Interrupt. An interrupt that cannot be disabled by software.
NUMA — Non-Uniform Memory Access. The multi-socket memory topology where memory access latency depends on which node holds the frame.
OIS — Optimization Intelligence Subsystem. The SAIRU component producing advisory optimisation recommendations.
OOM — Out Of Memory. The condition where physical and swap memory are exhausted.
PAE — Physical Address Extension. The x86 feature enabling more than 4GB of physical memory on 32-bit processors.
PIC — Programmable Interrupt Controller. The legacy 8259 interrupt controller.
PID — Process Identifier. The unique number identifying a process.
PIS — Predictive Intelligence Subsystem. The SAIRU trend analysis and forecasting component.
PIT — Programmable Interval Timer. The legacy 8253/8254 timer used for calibration on Tier 0 and Tier 1.
POSIX — Portable Operating System Interface. The Unix API compatibility standard.
PSS — Proportional Set Size. The memory attribution metric that attributes shared frames fractionally.
RAF — Resource Accounting Framework. The SAIOS subsystem tracking resource consumption by entity.
RAPL — Running Average Power Limit. Intel's energy measurement and control interface.
Red Ring — The SAIOS signal that the kernel has encountered a non-recoverable condition and has halted with maximum evidence preservation. Named for the visual indicator presented to the user.
SAIRU — Self-Aware Intelligence Reasoning Unit. The intelligence interface of SAIOS that translates kernel evidence into explanations, diagnoses, predictions, recommendations, and orchestrated recovery workflows.
SAIOS — Self-Aware Intelligence Operating System. This system.
SGQL — SAIOS Graph Query Language. The Cypher-inspired graph query language for the KGS.
SIMD — Single Instruction Multiple Data. Parallel processing instruction sets.
SLIT — System Locality Information Table. ACPI table describing inter-node memory access latencies.
SPKG — SAIOS Package. The native SAIOS package format (future deliverable).
SRAT — System Resource Affinity Table. ACPI table describing NUMA topology.
SSE — Streaming SIMD Extensions. Intel's original SIMD instruction set.
THP — Transparent Huge Pages. Automatic 2MB page promotion.
TSC — Time Stamp Counter. The x86 cycle counter.
TSS — Task State Segment. The x86 structure holding privilege-level stack pointers.
USE Method — Utilization, Saturation, Errors. A resource analysis methodology compatible with SAIOS telemetry.
VFS — Virtual Filesystem Switch. The kernel abstraction layer over all filesystem implementations.
VMA — Virtual Memory Area. A contiguous range of a process's virtual address space with uniform attributes.
WCL — Windows Compatibility Layer. The future SAIOS subsystem for running Windows PE binaries.
XDP — eXpress Data Path. High-performance network packet processing via eBPF.

### DOCUMENT AUTHORITY AND CONFLICT RESOLUTION

This document is the Single Source of Truth for SAIOS. It derives authority from and is subordinate to the SAIOS Kernel Constitution. In any conflict, the Constitution wins over this document, this document wins over all subsystem architecture documents below it, and implementation must conform to both. Any proposed change to this document that contradicts the Constitution requires a Constitutional amendment, not a SSOT update. Any proposed change to this document requires: identification of the specific invariant or contract rule being changed; a formal Architecture Decision Record explaining why; evidence that no new deadlock is introduced via lock order analysis; evidence that all failure modes for the changed path are covered; and evidence that no happy-path assumption has been introduced.

The Blackhatbadshah Principle: Most organisations do not lose time because systems fail. They lose time because understanding those failures takes too long. Failure leads to Understanding leads to Resolution. SAIOS exists to make that journey as short as possible.

The Goal: Build the operating system that understands itself.
