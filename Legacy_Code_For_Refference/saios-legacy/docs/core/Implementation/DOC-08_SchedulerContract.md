# SAIOS SchedulerContract Specification
**Document ID:** DOC-08_SchedulerContract.txt
**Layer:** Core Kernel Contracts
**Version:** 1.0.0
**Authority:** Subordinate to DOC-01 and DOC-05; authoritative over run queues, CPU assignment, preemption policy, scheduler metadata, and stall detection

## SOURCE TRACEABILITY

Sources: SAIOS_SSOT.txt SCHEDULERCONTRACT; PROGRESSCONTRACT; NUMA SCHEDULER POLICY; NUMA MIGRATION AND LOCALITY METRICS; NUMA FAILURE MODES.

## OWNERSHIP

SchedulerContract owns run queue membership, CPU assignment, block, wake, exit handoff, process selection, preemption, and finish-switch bookkeeping. ProgressContract owns cross-subsystem progress attribution and emits evidence; it never intervenes directly.

## INVARIANTS

1. PID on-CPU is in exactly one current slot and absent from all run queues.
2. Queued PID is Ready, not on-CPU, and absent from all current slots.
3. Idle process is never in any run queue.
4. Finish-switch executes for every switch-to with no exceptions.

Scheduler invariants take absolute precedence over NUMA preference.

## CURRENT SCHEDULER MODEL

The current SAIOS scheduler model is `SharedFifoRoundRobin`, matching `SchedulerContract::capability_view()`. It uses a shared runnable queue, preserves idle outside runnable queues, filters by allowed CPU metadata where implemented, and treats finish-switch bookkeeping as mandatory.

Per-CPU run queues, work stealing, CFS virtual runtime, deadline scheduling, and Linux-style realtime class semantics are not current guarantees. They are future scheduler policy phases and must not be reported as active capability until implementation and retained evidence exist.

## FUTURE SCHEDULING CLASSES

The intended future class order is Deadline real-time, FIFO real-time, Round-Robin real-time, fair normal, then Idle. Higher classes may preempt lower classes once those classes are implemented. Idle runs only when no non-idle process is Ready.

Future fair scheduling may use virtual runtime, a tree-backed run queue, target latency, and minimum granularity. Optimization Intelligence Subsystem hints may influence advisory weights only when DOC-01 execution-place invariants remain satisfied.

## NUMA POLICY

Current scheduling preserves NUMA metadata for future policy but does not run an automatic NUMA balancer. A future NUMA-aware scheduler may prefer local CPUs, emit NUMA_REMOTE_SCHEDULE for forced remote placement, detect load imbalance, migrate eligible processes, and emit NUMA_REBALANCE. NUMA balancing remains suspended during Red Ring when implemented.

## TELEMETRY

Scheduler telemetry must truthfully reflect the active model. Context-switch and scheduler-progress evidence are current requirements. Per-CPU run queue depth, precise per-task latency, NUMA rebalance, and preemption-class telemetry are future requirements tied to their corresponding policy phases.

## FAILURE MODES

PID in run queue and on-CPU simultaneously is Red Ring critical. Run queue corruption with cycle or null node is Red Ring critical. Missing finish-switch is Red Ring critical. CPU offline with current PID migrates the PID before acknowledgement or Red Rings. All CPUs in scheduler with no Ready process and no idle is ProgressContract livelock evidence. SIGKILL to idle is silently dropped. Dead process remaining on-CPU triggers Red Ring on next timer interrupt.

## PRIORITY INVERSION AND STARVATION

Priority inversion exists when a high-priority process blocks on a lock held by a lower-priority process. Priority inheritance is a future realtime-policy feature, not a current scheduler guarantee. Until implemented, related evidence may report detection without claiming inheritance.

Starvation exists when a Ready process is unscheduled beyond the configured threshold. Aging through temporary priority boost is a future policy feature. SCHEDULER_STARVATION evidence must distinguish detection from boost application.

## PROGRESSCONTRACT SIGNALS

ProgressContract monitors: scheduler forward progress, 5s, emits SCHED_STALL; KDS write throughput zero for 30s, emits KDS_WRITE_STALL; priority inversion beyond 500ms, emits PRIORITY_INVERSION_DETECTED; process starvation beyond 10s, emits SCHEDULER_STARVATION; OOM pressure above 85 percent for 60s, emits OOM_PRESSURE_TREND; driver init timeout 30s, emits DRIVER_INIT_TIMEOUT; IRQ storm above 80 percent CPU for 5s, emits IRQ_STORM.

ProgressContract emits KDS evidence and never intervenes directly. Automated intervention without human approval has caused more outages than it has prevented.

## CORNER CASES

AP startup failure marks CPU offline and never assigns work. CPU hotplug removal migrates all current and queued work before acknowledgement. IRQ storm consuming CPU time is detected by ProgressContract. Dead process on CPU is Red Ring at timer boundary.

## COMPLETION CHECK

A developer can reason from this document to the current shared FIFO scheduler and to the future phases for per-CPU queues, fair scheduling, realtime policy, NUMA-aware selection, priority inheritance, starvation aging, and precise progress thresholds without confusing future goals for current capability.
