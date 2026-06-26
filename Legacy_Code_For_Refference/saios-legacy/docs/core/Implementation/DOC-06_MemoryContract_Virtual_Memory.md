# SAIOS MemoryContract and Virtual Memory Specification
**Document ID:** DOC-06_MemoryContract_Virtual_Memory.txt
**Layer:** Core Kernel Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01; authoritative over frame ownership, virtual memory, COW, VMA lifecycle, OOM, slab, and page cache ownership

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt MEMORYCONTRACT AND ADDRESSSPACECONTRACT; NUMA MEMORY POLICY AND BALANCING; NUMA MIGRATION AND LOCALITY METRICS. SAIOS_SSOT_Part2.txt VIRTUAL MEMORY SUBSYSTEM; NUMA-AWARE SLAB ALLOCATOR; NUMA-AWARE PAGE CACHE.

## OWNERSHIP

MemoryContract owns frame ownership, frame reference counts, mapping authority, COW lifecycle, mmap, brk, execution mapping, unmap, stack growth, OOM killing, slab allocation, and page-cache frame ownership.

AddressSpaceContract owns address-space handles, CR3 transitions, page-table construction and destruction, and page-table mutation APIs.

## VIRTUAL ADDRESS SPACE LAYOUT

64-bit layout: user space spans 0x0000000000001000 to 0x00007FFFFFFFFFFF. The zero page is never mapped. Kernel space spans 0xFFFF800000000000 to 0xFFFFFFFFFFFFFFFF. The physmap begins at 0xFFFF800000000000. vmalloc lives above physmap. SAIRU and KDS reserved regions have fixed Gate 0 mappings and are never modified after sealing.

32-bit Tier 0 layout: user space spans 0x00001000 to 0xBFFFFFFF. Kernel space spans 0xC0000000 to 0xFFFFFFFF. PAE uses a 3-level table for up to 36-bit physical addressing while virtual addressing remains 32-bit.

## PAGE TABLES

64-bit systems use PML4, PDPT, PD, and PT. LA57-capable processors may use 5-level paging when detected. 32-bit systems use 2-level paging or PAE 3-level paging. Page-table pages are zero-initialised before use and are never dual-used for non-page-table data.

## VMA DATA MODEL AND OPERATIONS

Each VMA records base, size, protection flags, VMA type, backing object handle, backing object offset, COW flag, NUMA policy, THP eligibility flag, and KDS telemetry handle. VMA types are anonymous, file-backed, device-backed, stack, heap, and vDSO.

mmap creates a non-overlapping VMA after permission validation. mprotect changes protection and splits VMAs when needed. munmap removes ranges and releases exclusively owned frames. madvise updates THP eligibility, NUMA policy, or access hints. mremap moves or resizes a VMA. Every operation emits VMA_OPERATION with pid, operation, range, protection, VMA type, and result.

## PAGE FAULT DISPATCHER

The page fault handler classifies exactly six cases: COW fault, anonymous demand fault, file-backed demand fault, stack growth fault, NUMA placement fault, and unresolvable fault. All cases emit PAGE_FAULT with pid, address, fault_type, fault_class, resolution, and latency_ns.

COW faults allocate a private frame, copy content, update PTE and refcount together, and retry. Demand faults allocate zeroed frames. File-backed faults read through VFS page cache. Stack growth extends the stack VMA downward when within limit. NUMA faults record placement and may migrate. Unresolvable user faults deliver SIGSEGV; invalid kernel faults trigger Red Ring.

## INVARIANTS

COW reference count metadata and PTE COW flags change together always. A frame is owned by exactly one entity: process, kernel, KDS, IPC object, VFS page cache, or free pool. A frame refcount reaching zero is immediately reusable. KDS frames are never in the free pool, verified at boot and sealed.

CR3 is a mirror, not the owner. Page-table destruction occurs only after the address space is not current on any CPU. Fork produces a new handle; the parent handle is unchanged.

## FAILURE MODES

Double-free is Red Ring critical. Refcount/PTE COW mismatch is Red Ring high. OOM invokes OOM killer. OOM killer finding no victim triggers Red Ring. Interrupt-context allocation failure returns null. KDS frame allocated by non-KDS path triggers Red Ring. Fragmentation attempts compaction then fails visibly. User-frame MCE poisons frame and delivers SIGBUS. Kernel-frame MCE triggers Red Ring. NUMA migration racing with address-space destruction cancels migration. COW fault during fork teardown waits for teardown. Stack growth overlap returns SIGSEGV.

## OOM KILLER

ProgressContract identifies pressure. MemoryContract scores processes by memory footprint plus privilege penalty plus runtime penalty. Kernel-context and zombie processes are skipped. Highest eligible victim receives SIGKILL. If no victim exists, Red Ring is triggered. OOM_PRESSURE, OOM_KILL, and candidate-skip evidence are emitted.

## SLAB ALLOCATOR

Slab caches maintain per-CPU partial slabs, per-node full slabs, and per-node empty slabs. Allocation checks the owning CPU partial slab, then local node empty list, then remote nodes. No frame allocator calls occur in interrupt context; interrupt exhaustion returns null. SLAB_PRESSURE fires when per-node empty count falls below threshold. Remote allocation emits SLAB_REMOTE_ALLOC. Free returns to the node owning the backing frame. Cross-node free emits SLAB_CROSS_NODE_FREE. slabdefrag runs per node and emits SLAB_DEFRAG_COMPLETE; it pauses during OOM.

## PAGE CACHE

A page cache entry maps inode plus offset to one physical frame. States are Clean, Dirty, and Writeback. Writeback runs periodically and when dirty pages exceed 20 percent. PAGE_CACHE_WRITEBACK includes inode, page_count, and reason. PAGE_CACHE_EVICT includes inode, page_count, and dirty flag.

NUMA page cache placement follows first toucher. Multi-node shared files use NUMA_INTERLEAVE. Process scheduler migration may make file-backed page-cache frames candidates for migration; NUMA_PAGE_CACHE_MIGRATED records inode, old_node, new_node, and page_count.

## NUMA MEMORY POLICY

Per-VMA policies are NUMA_LOCAL, NUMA_BIND, NUMA_INTERLEAVE, and NUMA_PREFERRED. Policy changes emit MEMORY_NUMA_POLICY_SET. Locality score ranges from 0.0 to 1.0 and is computed from hardware counters on Tier 2+ or PTE access scanning on Tier 0/1. Sustained score below 0.6 for 5 seconds emits NUMA_LOCALITY_DEGRADED.

## COMPLETION CHECK

A developer can implement the VMA struct, page fault dispatcher, OOM killer, slab fast path, and page-cache ownership model with testable invariant violations and Red Ring triggers.
