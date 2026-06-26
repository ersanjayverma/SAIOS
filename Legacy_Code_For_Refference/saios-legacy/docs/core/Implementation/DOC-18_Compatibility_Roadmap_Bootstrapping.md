# SAIOS Compatibility Roadmap and Bootstrapping Path
**Document ID:** DOC-18_Compatibility_Roadmap_Bootstrapping.txt
**Layer:** Operations
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 through DOC-17

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt LINUX ABI COMPATIBILITY; POSIX COMPLIANCE LAYER; ELF LOADER; CONTAINER SUPPORT; WINDOWS COMPATIBILITY LAYER; PACKAGE ECOSYSTEM STRATEGY. SAIOS_SSOT_Part2.txt COMPATIBILITY EXECUTION ROADMAP; BOOTSTRAPPING PATH; COMPATIBILITY INVARIANTS.

## SEQUENCING PRINCIPLE

Compatibility is built outside-in for users, but implementation proceeds inside-out. Each phase has concrete deliverables and a runnable or observable completion criterion. No Phase N+1 starts until Phase N completion criterion is met.

## PHASE 1 - MONTH 0 TO 6 - NATIVE SAIOS BASELINE

Goal: QEMU/KVM boot to Gate 16 and native SAIOS shell. Deliverables: sixteen gates, KDS write path, PID 1 native init, native shell with fork/exec/wait/exit/read/write, core contracts, Red Ring, Flight Recorder event persistence. Not included: ELF loader, POSIX, networking, full VFS beyond minimal RAM FS, NUMA, SAIRU reasoning.

Completion criterion: native shell starts, executes a native binary that forks, child exits, parent wait reaps it, and KDS event sequence is visible in Flight Recorder output.

Expected first boot serial output matches DOC-04 through BOOT_COMPLETE and Gate 16 init launch.

## PHASE 2 - MONTH 6 TO 12 - ELF64 AND POSIX SUBSET

Goal: run BusyBox-class static Linux-style utilities through SAIOS ELF and POSIX subset. Deliverables: ELF64 loader, process syscalls, memory syscalls, basic VFS, initramfs/ext4 read-only, page cache, slab allocator, musl cross-compile path. Not included: dynamic linker, networking, multi-user, containers, SAIRU reasoning.

Completion criterion: BusyBox runs; developer can type ls, cat, grep, and sh in the SAIOS shell.

## PHASE 3 - MONTH 12 TO 18 - FULL LINUX SYSCALL ABI

Goal: broad Linux userspace ABI. Deliverables: Linux syscall table, dynamic linker support, signals, futexes, mmap, file APIs, sockets baseline, Python 3, SQLite, nginx, CE ingesting KDS, SGQL queryable from command line. Not included: container namespaces, Windows compatibility, full SAIRU diagnostic output.

Target completion criterion: Python 3 starts, SQLite creates and queries a database, nginx serves an HTTP response under SAIOS.

Current evidence note, 2026-06-20: Phase 3 is in progress and is not complete from source presence alone. The current tree has source-implemented process, memory, signal, pipe, socketpair, syscall ABI, and capability-enforcement slices, plus runtime validation manifest scenarios. Phase 3 completion still requires retained runtime transcripts for the quick validation suite, `syscallabitest`, `capabilitytest`, ProcessContract and MemoryContract KDS evidence, top-25 syscall gap closure or explicit conservative scoping, and the Python/SQLite/nginx workload transcript.

## PHASE 4 - MONTH 18 TO 24 - SAIRU PHASE ONE AND CONTAINERS

Goal: first practical differentiation from Linux. Deliverables: deterministic SAIRU engines, container namespaces, OCI image path, saios-intel CLI, Red Ring diagnosis within 5 seconds, OOM/scheduler stall/process crash/driver timeout diagnosis. Completion criterion: Docker-compatible container workflow runs and Red Ring produces structured diagnosis within 5 seconds.

## PHASE 5 - MONTH 24 TO 36 - FULL LINUX USERSPACE ON REAL HARDWARE

Goal: durable real-hardware operation. Deliverables: 72-hour continuous run, apt or apk working, PIS and OIS operational, NUMA-aware KDS placement, verifiable accounting invariants, external hardware validation. Completion criterion: 72-hour run on real hardware with package manager install/update and no accounting invariant violations.

## PHASE 6 - MONTH 36 PLUS - WCL AND AI MODEL INTEGRATION

Goal: Windows compatibility layer and AI-model gateway. Deliverables: PE loader, Win32 API translation, NT kernel API emulation, Windows filesystem namespace translator, AI Gateway enforcing CAP_SAIOS_INTELLIGENCE and CAP_SAIOS_POLICY. Completion criterion: Notepad.exe opens, accepts typed text, saves a file, and the file is accessible from SAIOS VFS.

Current evidence note, 2026-06-20: Windows compatibility is a future/scaffold layer only. No Windows application execution is claimed before a retained Phase 6 WCL transcript proves PE loading, Win32/NT translation, namespace translation, and VFS-visible file output.

## COMPATIBILITY INVARIANTS

1. Shims never bypass SecurityContract.
2. Shims emit the same KDS events as native operations.
3. Resource accounting applies equally to compatibility and native code.
4. No shim introduces a new happy-path assumption; semantic deviations emit COMPAT_SEMANTIC_DEVIATION at INFO severity.

## LINUX ABI

x86-64 Linux syscall convention: syscall number in RAX; arguments in RDI, RSI, RDX, R10, R8, R9; return in RAX; errors as negative RAX. Implementation priority: process management, memory, file, networking, signals, synchronisation.

Current status language: Linux ABI work is compatibility work, not SAIOS identity. Use Runtime Proven only for behavior with retained transcripts. Use Source Implemented for code paths that build but lack retained runtime proof. Use Compatibility Scoped for intentionally narrow support tiers or future compatibility layers.

## ELF LOADER

Procedure: validate magic; parse ELF header; parse program headers; create address space; map loadable segments with permissions; zero BSS; load interpreter if dynamic; set stack; build auxiliary vector; emit PROCESS_EXEC; transfer to entry point.

## CONTAINERS AND WCL

Containers use seven namespaces, OCI images, overlayfs layering, and container-specific KDS events. CE rules compute container health from namespace and resource evidence.

WCL phase reference includes PE loader, Win32 API translation, NT API emulation, and namespace translation. Smoke test is Notepad.exe open/type/save/read from SAIOS VFS.

## PACKAGE ECOSYSTEM

Support deb, RPM, Flatpak, AppImage, and future SPKG. saipkg principle: every package operation is reversible and KDS-emitting so behaviour changes can be correlated with package changes.

## DEAD PROJECT RISK MITIGATION

Mitigations: milestone visibility through serial gate output; test-driven invariants by writing Red Ring tests before enforcement code; phase gating; external verification at Phase 3.

## MONTH AND YEAR GOALS

Month 1: first boot. Month 3: 24-hour stable, 1000 forks/sec. Month 6: Phase 1 complete, about 30k lines, 500 unit tests, 50 integration tests, CI. Month 12: BusyBox. Month 18: Python. Month 24: first SAIRU diagnoses. Year 3: real hardware demonstration of asking why a process crashed and receiving structured evidence-backed causal explanation derived from KDS, not a crash dump.

## COMPLETION CHECK

A developer can read this document and know what to build in Month 1, how to prove Phase 1, and when SAIOS becomes practically different from existing operating systems.
