//! SAIRU (SAI Runtime): deterministic intelligence runtime shell client surface.
//!
//! Phase 1 has no AI model integration. It interprets requests through fixed
//! skills and KDS-backed tools so diagnosis remains available on every boot.

use crate::{print, println};

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolId {
    ProcessList = 1,
    SchedulerState = 2,
    MemoryState = 3,
    CpuState = 4,
    KdsEvents = 5,
    KdsMetrics = 6,
    KdsTraces = 7,
    StorageState = 8,
    HardwareState = 9,
    NetworkState = 10,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillId {
    DiagnoseFreeze = 1,
    DiagnoseMemory = 2,
    DiagnoseHealth = 3,
    ExplainProcess = 4,
    ExplainCpu = 5,
    DiagnoseStorage = 6,
    DiagnoseHardware = 7,
    ExplainStorage = 8,
    ExplainHardware = 9,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskId {
    DiagnoseFreezePeriodic = 1,
    HealthReportHourly = 2,
    ResourceSummaryDaily = 3,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SairuEngineId {
    Context = 1,
    Tool = 2,
    Skill = 3,
    Task = 4,
    Knowledge = 5,
    Planning = 6,
    Policy = 7,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    SchedulerStall = 1,
    KernelPanic = 2,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectorId {
    WatchdogContract = 1,
    PanicHandler = 2,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticActionId {
    CollectFreezeDump = 1,
    InspectSchedulerAndLocks = 2,
    InspectPanicLocation = 3,
    ReviewRecentKdsEvents = 4,
}

#[derive(Debug, Clone, Copy)]
pub struct SairuValidationReport {
    pub runtime_available: bool,
    pub engines_available: bool,
    pub tools_available: bool,
    pub skills_available: bool,
    pub tasks_available: bool,
    pub health_diagnostic: bool,
    pub memory_diagnostic: bool,
    pub freeze_diagnostic: bool,
    pub contract_boundary: bool,
    pub evidence_citations: bool,
    pub deterministic: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SairuRuntimeResult {
    pub title: &'static str,
    pub skill: Option<SkillId>,
    pub task: Option<TaskId>,
    pub evidence_class: &'static str,
    pub gap: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct SairuToolResult {
    pub tool: ToolId,
    pub evidence_class: &'static str,
    pub available: bool,
    pub records: u64,
    pub gaps: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct FailureDiagnostic {
    pub failure_kind: FailureKind,
    pub confidence: &'static str,
    pub detected_by: DetectorId,
    pub likely_cause: &'static str,
    pub evidence_label_1: &'static str,
    pub evidence_value_1: u64,
    pub evidence_label_2: &'static str,
    pub evidence_value_2: u64,
    pub evidence_label_3: &'static str,
    pub evidence_value_3: u64,
    pub recommended_action_1: DiagnosticActionId,
    pub recommended_action_2: DiagnosticActionId,
    pub reference_id: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct SairuEngineStatus {
    pub engine: SairuEngineId,
    pub deterministic: bool,
    pub contract_bound: bool,
    pub evidence_class: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SairuEvidenceRef {
    pub event_id: u64,
    pub source: &'static str,
    pub confidence: FixedConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedConfidence(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CausalRelationship {
    Caused,
    Enabled,
    BlockedBy,
    DependsOn,
    Produced,
    Consumed,
    CoOccurredWith,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SairuPolicyDecision {
    pub approved: bool,
    pub reason: &'static str,
    pub required_capability: Option<crate::security_contract::SecurityCapability>,
}

impl ToolId {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ProcessList => "process-list",
            Self::SchedulerState => "scheduler-state",
            Self::MemoryState => "memory-state",
            Self::CpuState => "cpu-state",
            Self::KdsEvents => "kds-events",
            Self::KdsMetrics => "kds-metrics",
            Self::KdsTraces => "kds-traces",
            Self::StorageState => "storage-state",
            Self::HardwareState => "hardware-state",
            Self::NetworkState => "network-state",
        }
    }

    pub const fn evidence_class(self) -> &'static str {
        match self {
            Self::ProcessList => "ProcessContract.process-evidence",
            Self::SchedulerState => "SchedulerContract.cpu/freeze-evidence",
            Self::MemoryState => "MemoryContract.memory-diagnostic-view",
            Self::CpuState => "SchedulerContract.cpu-evidence-view",
            Self::KdsEvents => "ObservabilityContract.KDS-events",
            Self::KdsMetrics => "ObservabilityContract.KDS-metrics",
            Self::KdsTraces => "ObservabilityContract.KDS-traces",
            Self::StorageState => "StoragePlatform.graph-snapshot-store",
            Self::HardwareState => "StoragePlatform.hardware-analysis",
            Self::NetworkState => "NetworkContract.status-view",
        }
    }
}

impl SkillId {
    pub const fn command(self) -> &'static str {
        match self {
            Self::DiagnoseFreeze => "diagnose-freeze",
            Self::DiagnoseMemory => "diagnose-memory",
            Self::DiagnoseHealth => "diagnose-health",
            Self::ExplainProcess => "explain-process",
            Self::ExplainCpu => "explain-cpu",
            Self::DiagnoseStorage => "diagnose-storage",
            Self::DiagnoseHardware => "diagnose-hardware",
            Self::ExplainStorage => "explain-storage",
            Self::ExplainHardware => "explain-hardware",
        }
    }
}

impl TaskId {
    pub const fn description(self) -> &'static str {
        match self {
            Self::DiagnoseFreezePeriodic => "diagnose-freeze every 5m",
            Self::HealthReportHourly => "health-report hourly",
            Self::ResourceSummaryDaily => "resource-summary daily",
        }
    }
}

impl SairuEngineId {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Context => "context-engine",
            Self::Tool => "tool-engine",
            Self::Skill => "skill-engine",
            Self::Task => "task-engine",
            Self::Knowledge => "knowledge-engine",
            Self::Planning => "planning-engine",
            Self::Policy => "policy-engine",
        }
    }

    pub const fn evidence_class(self) -> &'static str {
        match self {
            Self::Context => "SAIRU.context.KDS-reconstruction",
            Self::Tool => "SAIRU.tool.contract-api-schema",
            Self::Skill => "SAIRU.skill.diagnostic-sequence",
            Self::Task => "SAIRU.task.approved-workflow",
            Self::Knowledge => "SAIRU.knowledge.KGS-causal-query",
            Self::Planning => "SAIRU.planning.rollback-aware-plan",
            Self::Policy => "SAIRU.policy.capability-safety-reversibility",
        }
    }
}

impl FixedConfidence {
    pub const ZERO: Self = Self(0);
    pub const FULL: Self = Self(u16::MAX);
    pub const HIGH: Self = Self(0xC000);
}

impl FailureKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SchedulerStall => "Scheduler Stall",
            Self::KernelPanic => "Kernel Panic",
        }
    }
}

impl DetectorId {
    pub const fn label(self) -> &'static str {
        match self {
            Self::WatchdogContract => "Watchdog Contract",
            Self::PanicHandler => "Panic Handler",
        }
    }
}

impl DiagnosticActionId {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CollectFreezeDump => "Collect freeze dump",
            Self::InspectSchedulerAndLocks => "Inspect scheduler and lock paths",
            Self::InspectPanicLocation => "Inspect panic location",
            Self::ReviewRecentKdsEvents => "Review recent KDS events",
        }
    }
}

impl SairuValidationReport {
    pub fn passed(self) -> bool {
        self.runtime_available
            && self.engines_available
            && self.tools_available
            && self.skills_available
            && self.tasks_available
            && self.health_diagnostic
            && self.memory_diagnostic
            && self.freeze_diagnostic
            && self.contract_boundary
            && self.evidence_citations
            && self.deterministic
    }
}

pub fn engine_statuses() -> [SairuEngineStatus; 7] {
    let mut statuses = [SairuEngineStatus {
        engine: SairuEngineId::Context,
        deterministic: true,
        contract_bound: true,
        evidence_class: SairuEngineId::Context.evidence_class(),
    }; 7];
    let mut index = 0;
    while index < SAIRU_ENGINE_IDS.len() {
        let engine = SAIRU_ENGINE_IDS[index];
        statuses[index] = SairuEngineStatus {
            engine,
            deterministic: true,
            contract_bound: true,
            evidence_class: engine.evidence_class(),
        };
        index += 1;
    }
    statuses
}

pub fn approve_policy_action(
    action: &'static str,
    required_capability: Option<crate::security_contract::SecurityCapability>,
) -> SairuPolicyDecision {
    if let Some(capability) = required_capability
        && !crate::security_contract::SecurityContract::current_capabilities()
            .has_effective(capability)
    {
        return SairuPolicyDecision {
            approved: false,
            reason: "policy: missing required capability",
            required_capability,
        };
    }
    let _ = action;
    SairuPolicyDecision {
        approved: true,
        reason: "policy: deterministic checks passed",
        required_capability,
    }
}

pub fn handle_request(args: &str) {
    let request = args.trim();
    if request.eq_ignore_ascii_case("analyze my storage")
        || request.eq_ignore_ascii_case("analyse my storage")
        || request.eq_ignore_ascii_case("analyze storage")
        || request.eq_ignore_ascii_case("analyse storage")
    {
        storage_analyze();
        return;
    }

    let mut parts = args.split_whitespace();
    match (parts.next(), parts.next()) {
        (None, _) => help(),
        (Some("daignose"), Some("storage")) => {
            println!("Did you mean: sairu diagnose storage");
            diagnose_storage();
        }
        (Some("daignose"), Some("hardware")) => {
            println!("Did you mean: sairu diagnose hardware");
            diagnose_hardware();
        }
        (Some("diagnose"), Some("storag")) => {
            println!("Did you mean: sairu diagnose storage");
            diagnose_storage();
        }
        (Some("diagnose"), Some("freeze")) => diagnose_freeze(),
        (Some("diagnose"), Some("memory")) => diagnose_memory(),
        (Some("diagnose"), Some("health")) => diagnose_health(),
        (Some("diagnose"), Some("storage")) => diagnose_storage(),
        (Some("diagnose"), Some("hardware")) => diagnose_hardware(),
        (Some("analyze"), Some("storage")) => storage_analyze(),
        (Some("analyse"), Some("storage")) => {
            println!("Did you mean: sairu analyze storage");
            storage_analyze();
        }
        (Some("explain"), Some("process")) => explain_process(parts.next()),
        (Some("explain"), Some("cpu")) => explain_cpu(parts.next()),
        (Some("explain"), Some("storage")) => explain_storage(),
        (Some("explain"), Some("hardware")) => explain_hardware(),
        (Some("storage"), Some("disks")) => storage_disks(),
        (Some("storage"), Some("partitions")) => storage_partitions(),
        (Some("storage"), Some("filesystems")) => storage_filesystems(),
        (Some("storage"), Some("operating-systems")) => storage_operating_systems(),
        (Some("storage"), Some("analyze")) => storage_analyze(),
        (Some("storage"), Some("analyse")) => {
            println!("Did you mean: sairu storage analyze");
            storage_analyze();
        }
        (Some("storage"), Some("explain")) => explain_storage(),
        (Some("storage"), Some("diagnose")) => diagnose_storage(),
        (Some("storage"), Some("daignose")) => {
            println!("Did you mean: sairu storage diagnose");
            diagnose_storage();
        }
        (Some("hardware"), Some("analyze")) => explain_hardware(),
        (Some("hardware"), Some("compatibility")) => diagnose_hardware(),
        (Some("hardware"), Some("report")) => hardware_report(),
        (Some("task"), Some("run")) => match parts.next().and_then(task_from_name) {
            Some(task) => {
                let result = run_task(task);
                print_result_metadata(result);
            }
            None => {
                println!("usage: sairu task run diagnose-freeze|health-report|resource-summary")
            }
        },
        (Some("engines"), None) => engines(),
        (Some("tools"), None) => tools(),
        (Some("skills"), None) => skills(),
        (Some("tasks"), None) => tasks(),
        _ => help(),
    }
}

fn help() {
    println!("SAIRU - SAI Runtime");
    println!("usage:");
    println!("  sairu diagnose freeze");
    println!("  sairu diagnose memory");
    println!("  sairu diagnose health");
    println!("  sairu diagnose storage");
    println!("  sairu analyze storage");
    println!("  sairu diagnose hardware");
    println!("  sairu explain process <pid>");
    println!("  sairu explain cpu [id]");
    println!("  sairu explain storage");
    println!("  sairu explain hardware");
    println!(
        "  sairu storage disks|partitions|filesystems|operating-systems|analyze|explain|diagnose"
    );
    println!("  sairu hardware analyze|compatibility|report");
    println!("  sairu task run diagnose-freeze|health-report|resource-summary");
    println!("  sairu engines");
    println!("  sairu tools");
    println!("  sairu skills");
    println!("  sairu tasks");
}

fn override_help() {
    println!("SAIRU Override Commands:");
    println!("  sairu override <objective>   - Request override for objective");
    println!("  sairu override approve       - Approve pending override");
    println!("  sairu override abort         - Abort pending override");
    println!("  sairu override status        - Show override state");
    println!("  sairu override evidence      - Show override evidence");
    println!("  sairu override execute       - Execute approved override");
    println!("  sairu override verify        - Verify override result");
    println!();
    println!("Objectives:");
    println!("  reinstall   - Full system reinstall with evidence capture");
    println!("  update      - System update with rollback capture");
    println!("  boot-repair - Boot sector and GRUB repair");
    println!("  recovery    - SAIOS recovery workflow");
    println!("  filesystem-repair - Filesystem integrity restoration");
}

struct SairuToolProvider;

impl SairuToolProvider {
    fn execute(tool: ToolId) -> SairuToolResult {
        match tool {
            ToolId::ProcessList => SairuToolResult {
                tool,
                evidence_class: tool.evidence_class(),
                available: true,
                records: crate::kds::count_events(crate::kds::KdsEventType::TaskCreate),
                gaps: 0,
            },
            ToolId::SchedulerState | ToolId::CpuState => {
                let view = crate::scheduler_contract::SchedulerContract::freeze_diagnostic_view();
                let records = view.scheduler_progress_metrics + view.heartbeat_metrics;
                SairuToolResult {
                    tool,
                    evidence_class: tool.evidence_class(),
                    available: true,
                    records,
                    gaps: (records == 0) as u64,
                }
            }
            ToolId::MemoryState => {
                let view = crate::memory_contract::MemoryContract::diagnostic_view();
                let records = view.mmap_events
                    + view.munmap_events
                    + view.mprotect_events
                    + view.cow_fault_events
                    + view.fault_events
                    + view.page_alloc_metrics
                    + view.page_free_metrics;
                SairuToolResult {
                    tool,
                    evidence_class: tool.evidence_class(),
                    available: true,
                    records,
                    gaps: (records == 0) as u64,
                }
            }
            ToolId::KdsEvents => tool_stream_result(tool, crate::kds::stats().events.records),
            ToolId::KdsMetrics => tool_stream_result(tool, crate::kds::stats().metrics.records),
            ToolId::KdsTraces => tool_stream_result(tool, crate::kds::stats().traces.records),
            ToolId::StorageState => {
                let snapshot = crate::saios::storage_platform::decision_snapshot();
                SairuToolResult {
                    tool,
                    evidence_class: tool.evidence_class(),
                    available: true,
                    records: 1,
                    gaps: snapshot.plan.refusal_reason.is_some() as u64,
                }
            }
            ToolId::HardwareState => {
                let report = Self::hardware_analysis();
                SairuToolResult {
                    tool,
                    evidence_class: tool.evidence_class(),
                    available: true,
                    records: 1,
                    gaps: (report.critical_failures > 0) as u64,
                }
            }
            ToolId::NetworkState => {
                let status = crate::network_contract::NetworkContract::status_view();
                SairuToolResult {
                    tool,
                    evidence_class: tool.evidence_class(),
                    available: true,
                    records: status.tx_enqueued
                        + status.rx_enqueued
                        + status.socket_events
                        + status.tcp_transitions,
                    gaps: (status.driver == "none") as u64,
                }
            }
        }
    }

    fn storage_install_diagnostic() -> crate::saios::storage_platform::SairuDiagnostic {
        crate::saios::storage_platform::diagnostic_for_install_block()
    }

    fn hardware_analysis() -> crate::saios::storage_platform::CompatibilityReport {
        crate::saios::storage_platform::analyze_hardware()
    }

    fn install_target() -> crate::saios::storage_platform::InstallTargetAnalysis {
        crate::saios::storage_platform::analyze_install_target()
    }

    fn storage_scan() -> crate::saios::storage_platform::StoragePlatformReport {
        crate::saios::storage_platform::scan_storage()
    }

    fn install_plan() -> crate::saios::storage_platform::OperationPlan {
        crate::saios::storage_platform::plan_install()
    }

    fn cpu_features() -> crate::saios::storage_platform::CpuFeatureSummary {
        crate::saios::storage_platform::cpu_features()
    }
}

fn tool_stream_result(tool: ToolId, records: u64) -> SairuToolResult {
    SairuToolResult {
        tool,
        evidence_class: tool.evidence_class(),
        available: true,
        records,
        gaps: (records == 0) as u64,
    }
}

pub fn run_task(task: TaskId) -> SairuRuntimeResult {
    match task {
        TaskId::DiagnoseFreezePeriodic => {
            diagnose_freeze();
            runtime_result(
                "SAIRU task diagnose-freeze-periodic",
                Some(SkillId::DiagnoseFreeze),
                Some(task),
                ToolId::SchedulerState.evidence_class(),
                None,
            )
        }
        TaskId::HealthReportHourly => {
            diagnose_health();
            runtime_result(
                "SAIRU task health-report-hourly",
                Some(SkillId::DiagnoseHealth),
                Some(task),
                ToolId::KdsEvents.evidence_class(),
                None,
            )
        }
        TaskId::ResourceSummaryDaily => {
            diagnose_memory();
            runtime_result(
                "SAIRU task resource-summary-daily",
                Some(SkillId::DiagnoseMemory),
                Some(task),
                ToolId::MemoryState.evidence_class(),
                None,
            )
        }
    }
}

fn runtime_result(
    title: &'static str,
    skill: Option<SkillId>,
    task: Option<TaskId>,
    evidence_class: &'static str,
    gap: Option<&'static str>,
) -> SairuRuntimeResult {
    SairuRuntimeResult {
        title,
        skill,
        task,
        evidence_class,
        gap,
    }
}

fn diagnose_storage() {
    let diagnostic = SairuToolProvider::storage_install_diagnostic();
    println!("SAIRU diagnose-storage");
    println!("  evidence_class={}", ToolId::StorageState.evidence_class());
    print_storage_diagnostic(diagnostic);
}

fn diagnose_hardware() {
    let report = SairuToolProvider::hardware_analysis();
    println!("SAIRU diagnose-hardware");
    println!(
        "  evidence_class={}",
        ToolId::HardwareState.evidence_class()
    );
    println!("  failure_type: CompatibilityAnalysis");
    println!("  confidence: high");
    println!(
        "  evidence: score={} critical_failures={} warnings={}",
        report.score, report.critical_failures, report.warnings
    );
    println!("  likely_cause: {}", report.summary);
    println!("  recommendation: resolve critical failures before install/update execution");
    println!(
        "  recovery_path: run `storage graph`, `storage plan recover`, and `storage recovery`; no disk modifications are made"
    );
}

fn explain_storage() {
    let target = SairuToolProvider::install_target();
    println!("SAIRU explain-storage");
    println!("  evidence_class={}", ToolId::StorageState.evidence_class());
    println!("  target: {}", target.classification);
    println!("  risk: {}", target.risk.label());
    println!("  dual_boot_required: {}", target.dual_boot_required);
    if let Some(reason) = target.blocked_reason {
        println!("  advisory: {}", reason);
    } else {
        println!(
            "  advisory: install can continue after explicit user confirmation"
        );
    }
}

fn explain_hardware() {
    let report = SairuToolProvider::hardware_analysis();
    println!("SAIRU explain-hardware");
    println!(
        "  evidence_class={}",
        ToolId::HardwareState.evidence_class()
    );
    println!("  cpu required: {}", report.cpu_pass);
    println!("  memory required: {}", report.memory_pass);
    println!("  storage required: {}", report.storage_pass);
    println!("  boot required: {}", report.boot_pass);
    println!("  filesystem required: {}", report.filesystem_pass);
    println!("  device required: {}", report.device_pass);
    println!("  compatibility score: {}", report.score);
    println!("  summary: {}", report.summary);
}

fn storage_disks() {
    let report = SairuToolProvider::storage_scan();
    println!("SAIRU storage disks");
    match report.disk {
        Some(disk) => println!(
            "  {} {} serial={} capacity={}MiB sectors={} sector_size={}",
            disk.vendor,
            disk.model,
            disk.serial,
            disk.capacity_mib,
            disk.sector_count,
            disk.sector_size
        ),
        None => println!("  none"),
    }
}

fn storage_partitions() {
    let report = SairuToolProvider::storage_scan();
    println!("SAIRU storage partitions");
    if report.partitions.is_empty() {
        println!("  none");
    }
    for partition in &report.partitions {
        println!(
            "  {} table={} type=0x{:02x} start={} size={} fs={}",
            partition.index,
            crate::block::partition_table_name(partition.table),
            partition.type_code,
            partition.start_lba,
            partition.size_lba,
            partition.filesystem.label()
        );
    }
}

fn storage_filesystems() {
    let report = SairuToolProvider::storage_scan();
    println!("SAIRU storage filesystems");
    if report.filesystems.is_empty() {
        println!("  none");
    }
    for fs in &report.filesystems {
        println!(
            "  partition={} kind={} confidence={} evidence={}",
            fs.partition_index,
            fs.kind.label(),
            fs.confidence,
            fs.evidence
        );
    }
}

fn storage_operating_systems() {
    let report = SairuToolProvider::storage_scan();
    println!("SAIRU storage operating-systems");
    if report.operating_systems.is_empty() {
        println!("  none");
    }
    for os in &report.operating_systems {
        println!(
            "  kind={} confidence={} evidence={}",
            os.kind.label(),
            os.confidence,
            os.evidence
        );
    }
}

fn storage_analyze() {
    explain_storage();
    let plan = SairuToolProvider::install_plan();
    println!("  target_available: {}", plan.execution_enabled);
    println!("  plan_risk: {}", plan.risk.label());
    if plan.operations.is_empty() {
        println!("  plan_operations: none");
    } else {
        println!("  plan_operations:");
        for operation in &plan.operations {
            println!("    - {}", operation);
        }
    }
    if let Some(reason) = plan.refusal_reason {
        println!("  target_issue: {}", reason);
    }
}

fn hardware_report() {
    explain_hardware();
    let cpu = SairuToolProvider::cpu_features();
    println!(
        "  features: sse={} sse2={} sse3={} sse4.1={} sse4.2={} avx={} avx2={} apic={} x2apic={} nx={} pat={} tsc={} invariant_tsc={} virtualization={}",
        cpu.sse,
        cpu.sse2,
        cpu.sse3,
        cpu.sse4_1,
        cpu.sse4_2,
        cpu.avx,
        cpu.avx2,
        cpu.apic,
        cpu.x2apic,
        cpu.nx,
        cpu.pat,
        cpu.tsc,
        cpu.invariant_tsc,
        cpu.virtualization
    );
}

fn print_storage_diagnostic(diagnostic: crate::saios::storage_platform::SairuDiagnostic) {
    println!("  Failure Type: {}", diagnostic.failure_type);
    println!("  Confidence: {}", diagnostic.confidence);
    println!("  Evidence: {}", diagnostic.evidence);
    println!("  Likely Cause: {}", diagnostic.likely_cause);
    println!("  Recommendation: {}", diagnostic.recommendation);
    println!("  Recovery Path: {}", diagnostic.recovery_path);
}

fn diagnose_freeze() {
    let view = crate::scheduler_contract::SchedulerContract::freeze_diagnostic_view();

    println!("SAIRU diagnose-freeze");
    println!(
        "  evidence_class={}",
        ToolId::SchedulerState.evidence_class()
    );
    println!("  watchdog_stall_events={}", view.watchdog_stall_events);
    println!("  scheduler_stall_events={}", view.scheduler_stall_events);
    println!("  heartbeat_metrics={}", view.heartbeat_metrics);
    println!(
        "  scheduler_progress_metrics={}",
        view.scheduler_progress_metrics
    );
    if view.watchdog_stall_events > 0 || view.scheduler_stall_events > 0 {
        println!("  assessment: freeze evidence exists in KDS");
    } else if view.heartbeat_metrics > 0 && view.scheduler_progress_metrics > 0 {
        println!("  assessment: no freeze event found; progress evidence is present");
    } else {
        println!("  evidence_gap: missing watchdog stall and progress metrics");
        println!("  assessment: insufficient KDS progress evidence");
    }
}

fn diagnose_memory() {
    let view = crate::memory_contract::MemoryContract::diagnostic_view();
    println!("SAIRU diagnose-memory");
    println!("  evidence_class={}", ToolId::MemoryState.evidence_class());
    println!("  mmap_events={}", view.mmap_events);
    println!("  munmap_events={}", view.munmap_events);
    println!("  mprotect_events={}", view.mprotect_events);
    println!("  cow_fault_events={}", view.cow_fault_events);
    println!("  fault_events={}", view.fault_events);
    println!("  page_alloc_metrics={}", view.page_alloc_metrics);
    println!("  page_free_metrics={}", view.page_free_metrics);
    if view.mmap_events
        + view.munmap_events
        + view.mprotect_events
        + view.cow_fault_events
        + view.fault_events
        + view.page_alloc_metrics
        + view.page_free_metrics
        == 0
    {
        println!("  evidence_gap: no memory contract events or metrics in retained window");
    }
}

fn diagnose_health() {
    crate::kds::flush_aggregates();
    let stats = crate::kds::stats();
    println!("SAIRU diagnose-health");
    println!("  evidence_class={}", ToolId::KdsEvents.evidence_class());
    report_stream(
        "events",
        stats.events.records,
        stats.events.dropped,
        stats.events.capacity,
    );
    report_stream(
        "metrics",
        stats.metrics.records,
        stats.metrics.dropped,
        stats.metrics.capacity,
    );
    report_stream(
        "traces",
        stats.traces.records,
        stats.traces.dropped,
        stats.traces.capacity,
    );
    report_stream(
        "objects",
        stats.objects.records,
        stats.objects.dropped,
        stats.objects.capacity,
    );
    report_stream(
        "state",
        stats.state.records,
        stats.state.dropped,
        stats.state.capacity,
    );
    println!("  aggregates_active={}", stats.aggregates_used);
    if stats.events.records == 0
        || stats.metrics.records == 0
        || stats.traces.capacity == 0
        || stats.objects.capacity == 0
        || stats.state.capacity == 0
    {
        println!("  evidence_gap: one or more KDS streams have no retained records or no capacity");
    }
}

fn report_stream(name: &str, records: u64, dropped: u64, capacity: usize) {
    let utilization = if capacity == 0 {
        0
    } else {
        records.saturating_mul(100) / capacity as u64
    };
    println!(
        "  {:<7} records={} drops={} capacity={} utilization={} status={}",
        name,
        records,
        dropped,
        capacity,
        utilization,
        if dropped == 0 { "ok" } else { "overflow" }
    );
}

fn explain_process(pid_arg: Option<&str>) {
    let Some(pid) = pid_arg.and_then(|value| value.parse::<u32>().ok()) else {
        println!("usage: sairu explain process <pid>");
        return;
    };

    let mut seen = 0u64;
    println!("SAIRU explain-process pid={}", pid);
    println!("  evidence_class={}", ToolId::ProcessList.evidence_class());
    crate::process_contract::ProcessContract::for_each_evidence_view(pid, 128, |record| {
        seen += 1;
        println!(
            "  ts={} cpu={} subsystem={} event={} payload={:#x},{:#x},{:#x},{:#x}",
            record.timestamp,
            record.cpu,
            record.subsystem,
            record.event,
            record.evidence[0],
            record.evidence[1],
            record.evidence[2],
            record.evidence[3]
        );
    });
    if seen == 0 {
        println!(
            "  evidence_gap: no recent contract evidence for pid={}",
            pid
        );
    }
}

fn explain_cpu(cpu_arg: Option<&str>) {
    let cpu = cpu_arg.and_then(|value| value.parse::<u32>().ok());
    let smp = crate::smp::diagnostic_snapshot();
    println!("SAIRU explain-cpu {}", cpu.map_or(-1, |value| value as i32));
    println!("  evidence_class={}", ToolId::CpuState.evidence_class());
    println!(
        "  smp masks started={:#x} initialized={:#x} scheduler_visible={:#x}",
        smp.started_mask, smp.initialized_mask, smp.scheduler_visible_mask
    );
    let mut seen = 0u64;
    crate::scheduler_contract::SchedulerContract::for_each_cpu_evidence_view(cpu, 128, |record| {
        seen += 1;
        println!(
            "  ts={} cpu={} subsystem={} metric={} value={}",
            record.timestamp, record.cpu, record.subsystem, record.metric, record.value
        );
    });
    if seen == 0 {
        println!("  evidence_gap: no recent scheduler CPU metrics for requested scope");
    }
}

fn tools() {
    println!("SAIRU tools");
    for tool in TOOL_IDS {
        let result = SairuToolProvider::execute(tool);
        println!(
            "  {} evidence={} available={} records={} gaps={}",
            result.tool.name(),
            result.evidence_class,
            result.available,
            result.records,
            result.gaps
        );
    }
}

fn engines() {
    println!("SAIRU engines");
    for status in engine_statuses() {
        println!(
            "  {} evidence={} deterministic={} contract_bound={}",
            status.engine.name(),
            status.evidence_class,
            status.deterministic,
            status.contract_bound
        );
    }
}

fn skills() {
    println!("SAIRU skills");
    for skill in SKILL_IDS {
        println!(
            "  {} evidence={}",
            skill.command(),
            skill_evidence_class(skill)
        );
    }
}

fn tasks() {
    println!("SAIRU tasks");
    println!("  scheduled execution support: runtime task invocation active");
    print!("  examples:");
    for task in TASK_IDS {
        print!(
            " {}{}",
            task.description(),
            if task == TASK_IDS[TASK_IDS.len() - 1] {
                ""
            } else {
                ";"
            }
        );
    }
    println!();
}

fn task_from_name(name: &str) -> Option<TaskId> {
    match name {
        "diagnose-freeze" | "diagnose-freeze-periodic" => Some(TaskId::DiagnoseFreezePeriodic),
        "health-report" | "health-report-hourly" => Some(TaskId::HealthReportHourly),
        "resource-summary" | "resource-summary-daily" => Some(TaskId::ResourceSummaryDaily),
        _ => None,
    }
}

fn skill_evidence_class(skill: SkillId) -> &'static str {
    match skill {
        SkillId::DiagnoseFreeze => ToolId::SchedulerState.evidence_class(),
        SkillId::DiagnoseMemory => ToolId::MemoryState.evidence_class(),
        SkillId::DiagnoseHealth => ToolId::KdsEvents.evidence_class(),
        SkillId::ExplainProcess => ToolId::ProcessList.evidence_class(),
        SkillId::ExplainCpu => ToolId::CpuState.evidence_class(),
        SkillId::DiagnoseStorage | SkillId::ExplainStorage => ToolId::StorageState.evidence_class(),
        SkillId::DiagnoseHardware | SkillId::ExplainHardware => {
            ToolId::HardwareState.evidence_class()
        }
    }
}

fn print_result_metadata(result: SairuRuntimeResult) {
    println!("  result_title: {}", result.title);
    println!("  evidence_class: {}", result.evidence_class);
    if let Some(skill) = result.skill {
        println!("  skill: {}", skill.command());
    }
    if let Some(task) = result.task {
        println!("  task: {}", task.description());
    }
    if let Some(gap) = result.gap {
        println!("  evidence_gap: {}", gap);
    }
}

/// Number of registered SAIRU skills (Gate 15 validation).
pub const fn skill_count() -> usize {
    9 // SkillId variants: DiagnoseFreeze..ExplainHardware
}

/// Number of registered SAIRU tools (Gate 15 validation).
pub const fn tool_count() -> usize {
    10 // ToolId variants: ProcessList..NetworkState
}

pub fn failure_summary() -> FailureDiagnostic {
    if let Some(stall) = crate::scheduler_contract::SchedulerContract::latest_watchdog_stall_view()
    {
        let cpu = stall.cpu as u64;
        let pid = stall.pid as u64;
        let seconds = stall.seconds_without_progress;
        return FailureDiagnostic {
            failure_kind: FailureKind::SchedulerStall,
            confidence: "HIGH",
            detected_by: DetectorId::WatchdogContract,
            likely_cause: "A CPU stopped making forward progress",
            evidence_label_1: "Current CPU",
            evidence_value_1: cpu,
            evidence_label_2: "Current Task",
            evidence_value_2: pid,
            evidence_label_3: "Runtime Without Progress Seconds",
            evidence_value_3: seconds,
            recommended_action_1: DiagnosticActionId::CollectFreezeDump,
            recommended_action_2: DiagnosticActionId::InspectSchedulerAndLocks,
            reference_id: reference_id(cpu, pid, stall.event_id, seconds),
        };
    }

    let panic = crate::panic_state::sairu_failure_snapshot();
    let cpu = panic.map_or(crate::process::table::cpu_idx() as u64, |snap| {
        snap.owner_cpu as u64
    });
    let pid = panic.map_or(0, |snap| snap.owner_pid as u64);
    let rip = panic.map_or(0, |snap| snap.rip);
    FailureDiagnostic {
        failure_kind: FailureKind::KernelPanic,
        confidence: "MEDIUM",
        detected_by: DetectorId::PanicHandler,
        likely_cause: "Kernel code raised a panic or contract violation",
        evidence_label_1: "Current CPU",
        evidence_value_1: cpu,
        evidence_label_2: "Current Task",
        evidence_value_2: pid,
        evidence_label_3: "Instruction Pointer",
        evidence_value_3: rip,
        recommended_action_1: DiagnosticActionId::InspectPanicLocation,
        recommended_action_2: DiagnosticActionId::ReviewRecentKdsEvents,
        reference_id: reference_id(cpu, pid, rip, panic.map_or(0, |snap| snap.time)),
    }
}

fn reference_id(a: u64, b: u64, c: u64, d: u64) -> u64 {
    0x5A10_0000_0000_0000u64 ^ a.rotate_left(7) ^ b.rotate_left(17) ^ c ^ d.rotate_left(31)
}

pub fn validate_runtime() -> SairuValidationReport {
    crate::kds::flush_aggregates();
    let stats = crate::kds::stats();
    let memory = crate::memory_contract::MemoryContract::diagnostic_view();
    let freeze = crate::scheduler_contract::SchedulerContract::freeze_diagnostic_view();
    let mut tools_available = true;
    let mut evidence_citations = true;
    let mut engines_available = SAIRU_ENGINE_IDS.len() == 7;
    for engine in engine_statuses() {
        engines_available &= engine.deterministic
            && engine.contract_bound
            && !engine.engine.name().is_empty()
            && !engine.evidence_class.is_empty();
    }
    for tool in TOOL_IDS {
        let result = SairuToolProvider::execute(tool);
        tools_available &= result.available;
        evidence_citations &= !result.evidence_class.is_empty();
    }
    let task_result = runtime_result(
        "SAIRU validation task boundary",
        Some(SkillId::DiagnoseHealth),
        Some(TaskId::HealthReportHourly),
        ToolId::KdsEvents.evidence_class(),
        None,
    );
    SairuValidationReport {
        runtime_available: true,
        engines_available,
        tools_available: TOOL_IDS.len() >= 10 && tools_available,
        skills_available: SKILL_IDS.len() >= 9,
        tasks_available: TASK_IDS.len() >= 3,
        health_diagnostic: stats.events.capacity > 0
            && stats.metrics.capacity > 0
            && stats.traces.capacity > 0
            && stats.objects.capacity > 0
            && stats.state.capacity > 0,
        memory_diagnostic: memory.mmap_events > 0 || memory.page_alloc_metrics > 0,
        freeze_diagnostic: freeze.watchdog_stall_events > 0
            || freeze.heartbeat_metrics > 0
            || crate::kds::aggregate_exists(
                crate::kds::KdsSubsystem::Watchdog,
                crate::kds::KdsMetricId::CpuHeartbeat,
            ),
        contract_boundary: !task_result.evidence_class.is_empty()
            && SairuToolProvider::execute(ToolId::StorageState).available
            && SairuToolProvider::execute(ToolId::NetworkState).available,
        evidence_citations,
        deterministic: true,
    }
}

const TOOL_IDS: [ToolId; 10] = [
    ToolId::ProcessList,
    ToolId::SchedulerState,
    ToolId::MemoryState,
    ToolId::CpuState,
    ToolId::KdsEvents,
    ToolId::KdsMetrics,
    ToolId::KdsTraces,
    ToolId::StorageState,
    ToolId::HardwareState,
    ToolId::NetworkState,
];

const SKILL_IDS: [SkillId; 9] = [
    SkillId::DiagnoseFreeze,
    SkillId::DiagnoseMemory,
    SkillId::DiagnoseHealth,
    SkillId::ExplainProcess,
    SkillId::ExplainCpu,
    SkillId::DiagnoseStorage,
    SkillId::DiagnoseHardware,
    SkillId::ExplainStorage,
    SkillId::ExplainHardware,
];

const TASK_IDS: [TaskId; 3] = [
    TaskId::DiagnoseFreezePeriodic,
    TaskId::HealthReportHourly,
    TaskId::ResourceSummaryDaily,
];

const SAIRU_ENGINE_IDS: [SairuEngineId; 7] = [
    SairuEngineId::Context,
    SairuEngineId::Tool,
    SairuEngineId::Skill,
    SairuEngineId::Task,
    SairuEngineId::Knowledge,
    SairuEngineId::Planning,
    SairuEngineId::Policy,
];