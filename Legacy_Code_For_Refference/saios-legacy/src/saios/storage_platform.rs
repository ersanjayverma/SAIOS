//! Constitutional Storage Platform Contract: discovery, graph planning, snapshots, rollback,
//! recovery, boot policy, and diagnostics.
//!
//! This module is the storage authority. Installer, updater, recovery, live
//! environment, and SAIRU paths route through this contract instead of owning
//! separate disk interpretation or rollback logic.

use crate::block::{self, PartitionTableKind, RootFilesystemState, StorageController};
use crate::vfs::{Inode, Stat};
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlatformRisk {
    Low,
    Medium,
    High,
    Critical,
}

impl PlatformRisk {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemKind {
    Fat32,
    Ext4,
    Ntfs,
    Unknown,
}

impl FilesystemKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fat32 => "FAT32/EFI",
            Self::Ext4 => "ext4",
            Self::Ntfs => "NTFS",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingSystemKind {
    Saios,
    Windows,
    Linux,
    UnknownEfi,
}

impl OperatingSystemKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Saios => "SAIOS",
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::UnknownEfi => "unknown EFI OS",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StorageDisk {
    pub controller: StorageController,
    pub transport: &'static str,
    pub vendor: &'static str,
    pub model: &'static str,
    pub serial: &'static str,
    pub capacity_mib: u64,
    pub sector_count: u64,
    pub sector_size: usize,
}

#[derive(Debug, Clone)]
pub struct StoragePartition {
    pub index: usize,
    pub table: PartitionTableKind,
    pub type_code: u8,
    pub start_lba: u64,
    pub size_lba: u64,
    pub filesystem: FilesystemKind,
}

#[derive(Debug, Clone)]
pub struct FilesystemFinding {
    pub partition_index: usize,
    pub kind: FilesystemKind,
    pub confidence: &'static str,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct OperatingSystemFinding {
    pub kind: OperatingSystemKind,
    pub confidence: &'static str,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct StoragePlatformReport {
    pub operation_id: u64,
    pub disk: Option<StorageDisk>,
    pub mbr_valid: bool,
    pub gpt_valid: bool,
    pub root_state: RootFilesystemState,
    pub partitions: Vec<StoragePartition>,
    pub filesystems: Vec<FilesystemFinding>,
    pub operating_systems: Vec<OperatingSystemFinding>,
}

#[derive(Debug, Clone, Copy)]
pub struct InstallRequirements {
    pub minimum_root_mib: u64,
    pub recommended_root_mib: u64,
    pub minimum_efi_mib: u64,
    pub recommended_efi_mib: u64,
}

pub const DEFAULT_INSTALL_REQUIREMENTS: InstallRequirements = InstallRequirements {
    minimum_root_mib: 20 * 1024,
    recommended_root_mib: 64 * 1024,
    minimum_efi_mib: 300,
    recommended_efi_mib: 512,
};

const UPDATE_ESP_START_LBA: u64 = 2048;
const UPDATE_ESP_MAX_SECTORS: u64 = 48 * 1024 * 1024 / 512;
const UPDATE_MIN_ROOT_SECTORS: u64 = 16 * 1024 * 1024 / 512;
const UPDATE_BOOT_POLICY_MAX_BYTES: usize = 64 * 1024;
const SPC_METADATA_DIR: &str = "/var/lib/saios/storage-platform";
const SPC_SLOT_METADATA_PATH: &str = "/var/lib/saios/storage-platform/slots.toml";
const SPC_LATEST_SNAPSHOT_PATH: &str = "/var/lib/saios/storage-platform/latest-snapshot.toml";

#[derive(Debug, Clone)]
pub struct Disk {
    pub controller: StorageController,
    pub transport: &'static str,
    pub vendor: &'static str,
    pub model: &'static str,
    pub serial: &'static str,
    pub capacity_mib: u64,
    pub sector_count: u64,
    pub sector_size: usize,
}

#[derive(Debug, Clone)]
pub struct Partition {
    pub index: usize,
    pub table: PartitionTableKind,
    pub type_code: u8,
    pub start_lba: u64,
    pub size_lba: u64,
    pub size_mib: u64,
    pub filesystem: FilesystemKind,
}

#[derive(Debug, Clone)]
pub struct Filesystem {
    pub partition_index: usize,
    pub kind: FilesystemKind,
    pub confidence: &'static str,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct OperatingSystem {
    pub kind: OperatingSystemKind,
    pub partition_index: Option<usize>,
    pub efi_partition_index: Option<usize>,
    pub confidence: &'static str,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct StorageModel {
    pub operation_id: u64,
    pub disk: Option<Disk>,
    pub mbr_valid: bool,
    pub gpt_valid: bool,
    pub root_state: RootFilesystemState,
    pub partitions: Vec<Partition>,
    pub filesystems: Vec<Filesystem>,
    pub operating_systems: Vec<OperatingSystem>,
    pub requirements: InstallRequirements,
}

#[derive(Debug, Clone)]
pub struct StorageCapability {
    pub install: bool,
    pub resize: bool,
    pub dual_boot: bool,
    pub recovery: bool,
    pub replace_existing: bool,
}

#[derive(Debug, Clone)]
pub struct StorageEvidence {
    pub subject: &'static str,
    pub detail: &'static str,
    pub confidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct StorageRecommendation {
    pub action: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct StorageAssessmentCheck {
    pub name: &'static str,
    pub passed: bool,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct StorageAssessmentFailure {
    pub reason: &'static str,
    pub evidence: &'static str,
    pub recommendation: &'static str,
}

#[derive(Debug, Clone)]
pub struct InstallDecision {
    pub allowed: bool,
    pub reasons: Vec<&'static str>,
    pub evidence: Vec<StorageEvidence>,
    pub confidence: &'static str,
    pub confidence_score: u8,
}

#[derive(Debug, Clone)]
pub struct StorageAssessment {
    pub operation_id: u64,
    pub model: StorageModel,
    pub checks: Vec<StorageAssessmentCheck>,
    pub failures: Vec<StorageAssessmentFailure>,
    pub risks: Vec<&'static str>,
    pub capabilities: StorageCapability,
    pub decision: InstallDecision,
    pub recommendations: Vec<StorageRecommendation>,
    pub evidence: Vec<StorageEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallValidationStatus {
    Passed,
    Failed,
}

impl InstallValidationStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallValidationCheck {
    pub name: &'static str,
    pub passed: bool,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct InstallValidationFailure {
    pub reason: &'static str,
    pub evidence: &'static str,
    pub suggested_fix: &'static str,
}

#[derive(Debug, Clone)]
pub struct InstallPlanValidation {
    pub operation_id: u64,
    pub status: InstallValidationStatus,
    pub checks: Vec<InstallValidationCheck>,
    pub failures: Vec<InstallValidationFailure>,
    pub suggested_fixes: Vec<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RiskFactor {
    pub level: RiskLevel,
    pub reason: &'static str,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct RiskRecommendation {
    pub action: &'static str,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct RiskAssessment {
    pub operation_id: u64,
    pub completed: bool,
    pub level: RiskLevel,
    pub score: u8,
    pub factors: Vec<RiskFactor>,
    pub recommendations: Vec<RiskRecommendation>,
}

#[derive(Debug, Clone)]
pub struct SimulatedAction {
    pub step: usize,
    pub action: &'static str,
    pub detail: &'static str,
}

#[derive(Debug, Clone)]
pub struct InstallSimulation {
    pub operation_id: u64,
    pub blocked: bool,
    pub blocked_reason: Option<&'static str>,
    pub actions: Vec<SimulatedAction>,
    pub no_changes_made: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Safe,
    DualBoot,
    Recovery,
    ReplaceExistingSaios,
    FreshDisk,
}

impl InstallMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Safe => "Safe",
            Self::DualBoot => "Dual Boot",
            Self::Recovery => "Recovery",
            Self::ReplaceExistingSaios => "Replace Existing SAIOS",
            Self::FreshDisk => "Fresh Disk",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallModeRecommendation {
    pub operation_id: u64,
    pub mode: InstallMode,
    pub confidence: u8,
    pub reasons: Vec<&'static str>,
    pub evidence: Vec<StorageEvidence>,
}

impl StorageModel {
    pub fn from_report(report: &StoragePlatformReport, requirements: InstallRequirements) -> Self {
        let sector_size = report
            .disk
            .as_ref()
            .map(|disk| disk.sector_size as u64)
            .unwrap_or(512);
        let disk = report.disk.as_ref().map(|disk| Disk {
            controller: disk.controller,
            transport: disk.transport,
            vendor: disk.vendor,
            model: disk.model,
            serial: disk.serial,
            capacity_mib: disk.capacity_mib,
            sector_count: disk.sector_count,
            sector_size: disk.sector_size,
        });
        let mut partitions = Vec::new();
        for partition in &report.partitions {
            partitions.push(Partition {
                index: partition.index,
                table: partition.table,
                type_code: partition.type_code,
                start_lba: partition.start_lba,
                size_lba: partition.size_lba,
                size_mib: partition
                    .size_lba
                    .saturating_mul(sector_size)
                    .checked_div(1024 * 1024)
                    .unwrap_or(0),
                filesystem: partition.filesystem,
            });
        }
        let mut filesystems = Vec::new();
        for filesystem in &report.filesystems {
            filesystems.push(Filesystem {
                partition_index: filesystem.partition_index,
                kind: filesystem.kind,
                confidence: filesystem.confidence,
                evidence: filesystem.evidence,
            });
        }
        let efi_partition_index = partitions
            .iter()
            .find(|partition| partition.type_code == 0xEF)
            .map(|partition| partition.index);
        let mut operating_systems = Vec::new();
        for operating_system in &report.operating_systems {
            operating_systems.push(OperatingSystem {
                kind: operating_system.kind,
                partition_index: operating_system_partition(operating_system.kind, &filesystems),
                efi_partition_index,
                confidence: operating_system.confidence,
                evidence: operating_system.evidence,
            });
        }
        StorageModel {
            operation_id: report.operation_id,
            disk,
            mbr_valid: report.mbr_valid,
            gpt_valid: report.gpt_valid,
            root_state: report.root_state,
            partitions,
            filesystems,
            operating_systems,
            requirements,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompatibilityReport {
    pub operation_id: u64,
    pub score: u8,
    pub critical_failures: u8,
    pub warnings: u8,
    pub cpu_pass: bool,
    pub memory_pass: bool,
    pub storage_pass: bool,
    pub boot_pass: bool,
    pub filesystem_pass: bool,
    pub device_pass: bool,
    pub summary: &'static str,
}

#[derive(Debug, Clone)]
pub struct InstallTargetAnalysis {
    pub operation_id: u64,
    pub classification: &'static str,
    pub risk: PlatformRisk,
    pub safe_install_target: bool,
    pub dual_boot_required: bool,
    pub required_operations: Vec<&'static str>,
    pub blocked_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct OperationPlan {
    pub operation_id: u64,
    pub execution_enabled: bool,
    pub risk: PlatformRisk,
    pub rollback_feasible: bool,
    pub estimated_seconds: u64,
    pub operations: Vec<&'static str>,
    pub refusal_reason: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageClassification {
    Saios,
    Windows,
    Linux,
    Empty,
    Unknown,
    Corrupt,
}

impl StorageClassification {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Saios => "SAIOS",
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::Empty => "Empty",
            Self::Unknown => "Unknown",
            Self::Corrupt => "Corrupt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaiosSlotId {
    A,
    B,
}

impl SaiosSlotId {
    pub const fn label(self) -> &'static str {
        match self {
            Self::A => "Slot A",
            Self::B => "Slot B",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaiosSlotState {
    Active,
    Previous,
    Candidate,
    Empty,
    Missing,
}

impl SaiosSlotState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Previous => "previous",
            Self::Candidate => "candidate",
            Self::Empty => "empty",
            Self::Missing => "missing",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SaiosSlot {
    pub slot: SaiosSlotId,
    pub state: SaiosSlotState,
    pub partition_index: Option<usize>,
    pub bootable: bool,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct BootEntry {
    pub name: &'static str,
    pub partition_index: Option<usize>,
    pub preferred: bool,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct StorageGraphPartition {
    pub index: usize,
    pub classification: StorageClassification,
    pub filesystem: FilesystemKind,
    pub start_lba: u64,
    pub size_lba: u64,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct StorageGraph {
    pub operation_id: u64,
    pub disk: Option<Disk>,
    pub classification: StorageClassification,
    pub partitions: Vec<StorageGraphPartition>,
    pub operating_systems: Vec<OperatingSystem>,
    pub slots: Vec<SaiosSlot>,
    pub boot_entries: Vec<BootEntry>,
    pub evidence: Vec<StorageEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageIntent {
    Install,
    Update,
    Recover,
    Rollback,
    Diagnose,
}

impl StorageIntent {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Recover => "recover",
            Self::Rollback => "rollback",
            Self::Diagnose => "diagnose storage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageGateKind {
    Storage,
    Filesystem,
    Boot,
    Snapshot,
    Rollback,
    Recovery,
    Verification,
}

impl StorageGateKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Storage => "Storage Gate",
            Self::Filesystem => "Filesystem Gate",
            Self::Boot => "Boot Gate",
            Self::Snapshot => "Snapshot Gate",
            Self::Rollback => "Rollback Gate",
            Self::Recovery => "Recovery Gate",
            Self::Verification => "Verification Gate",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StorageGateResult {
    pub gate: StorageGateKind,
    pub passed: bool,
    pub evidence: &'static str,
    pub blocking_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct ExecutionPlanStep {
    pub step: usize,
    pub action: &'static str,
    pub affected: &'static str,
    pub verification: &'static str,
    pub rollback_action: &'static str,
}

#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub plan_id: u64,
    pub intent: StorageIntent,
    pub graph: StorageGraph,
    pub risk: PlatformRisk,
    pub execution_enabled: bool,
    pub gates: Vec<StorageGateResult>,
    pub steps: Vec<ExecutionPlanStep>,
    pub approval_required: bool,
    pub refusal_reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct SystemSnapshot {
    pub snapshot_id: u64,
    pub build_id: &'static str,
    pub kernel_version: &'static str,
    pub created_at_ns: u64,
    pub source_slot: SaiosSlotId,
    pub target_slot: SaiosSlotId,
    pub root_start_lba: u64,
    pub root_size_lba: u64,
    pub configuration_state_hash: u64,
    pub boot_state_hash: u64,
    pub rollback_eligible: bool,
}

#[derive(Debug, Clone)]
pub struct SnapshotStoreReport {
    pub operation_id: u64,
    pub metadata_dir: &'static str,
    pub slot_metadata_path: &'static str,
    pub latest_snapshot_path: &'static str,
    pub available: bool,
    pub latest_snapshot_id: Option<u64>,
    pub evidence: &'static str,
}

#[derive(Debug, Clone)]
pub struct ResizeReport {
    pub execution_enabled: bool,
    pub safe: bool,
    pub reason: &'static str,
}

#[derive(Debug, Clone)]
pub struct RecoveryReport {
    pub operation_id: u64,
    pub disk_diagnostics: bool,
    pub partition_diagnostics: bool,
    pub filesystem_diagnostics: bool,
    pub efi_repair_available: bool,
    pub boot_repair_available: bool,
    pub rootfs_repair_available: bool,
    pub summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateFileIdentity {
    pub inode: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: i64,
    pub blocks: i64,
    pub links: u64,
}

#[derive(Debug, Clone)]
pub struct UpdatePreservationSnapshot {
    pub operation_id: u64,
    pub snapshot_id: u64,
    pub build_id: &'static str,
    pub kernel_version: &'static str,
    pub active_slot: SaiosSlotId,
    pub rollback_slot: SaiosSlotId,
    pub root_start_lba: u64,
    pub root_size_lba: u64,
    pub canonical_config: UpdateFileIdentity,
    pub compatibility_config: UpdateFileIdentity,
    pub previous_boot_policy: Vec<u8>,
    pub previous_boot_policy_hash: u64,
    pub system_snapshot: SystemSnapshot,
}

#[derive(Debug, Clone)]
pub struct SairuDiagnostic {
    pub failure_type: &'static str,
    pub confidence: &'static str,
    pub evidence: &'static str,
    pub likely_cause: &'static str,
    pub recommendation: &'static str,
    pub recovery_path: &'static str,
}

#[derive(Debug, Clone)]
pub struct StorageDecisionSnapshot {
    pub operation_id: u64,
    pub assessment: StorageAssessment,
    pub compatibility: CompatibilityReport,
    pub target: InstallTargetAnalysis,
    pub plan: OperationPlan,
    pub validation: InstallPlanValidation,
    pub risk: RiskAssessment,
    pub simulation: InstallSimulation,
    pub recommendation: InstallModeRecommendation,
}

pub fn scan_storage() -> StoragePlatformReport {
    let operation_id = emit_event(
        crate::kds::KdsEventType::HardwareScanBegin,
        crate::kds::KdsSeverity::Info,
        [1, 0, 0, 0],
    );
    let diagnostic = block::diagnose();
    let root_state = block::classify_root_filesystem(&diagnostic);
    let disk = diagnostic.device.map(|device| StorageDisk {
        controller: device.controller,
        transport: block::controller_name(device.controller),
        vendor: controller_vendor(device.controller),
        model: controller_model(device.controller),
        serial: "not-reported",
        capacity_mib: device
            .sector_count
            .saturating_mul(device.sector_size as u64)
            .checked_div(1024 * 1024)
            .unwrap_or(0),
        sector_count: device.sector_count,
        sector_size: device.sector_size,
    });

    let mut partitions = Vec::new();
    let mut filesystems = Vec::new();
    for partition in &diagnostic.partitions {
        let filesystem = infer_filesystem(partition, &diagnostic.probes);
        partitions.push(StoragePartition {
            index: partition.index,
            table: partition.table,
            type_code: partition.type_code,
            start_lba: partition.start_lba,
            size_lba: partition.size_lba,
            filesystem,
        });
        if filesystem != FilesystemKind::Unknown {
            filesystems.push(FilesystemFinding {
                partition_index: partition.index,
                kind: filesystem,
                confidence: filesystem_confidence(filesystem),
                evidence: filesystem_evidence(filesystem),
            });
        }
    }

    let operating_systems = discover_operating_systems(root_state, &partitions, &filesystems);
    emit_event(
        crate::kds::KdsEventType::HardwareScanComplete,
        crate::kds::KdsSeverity::Info,
        [
            operation_id,
            disk.is_some() as u64,
            partitions.len() as u64,
            filesystems.len() as u64,
        ],
    );
    crate::observability_contract::ObservabilityContract::kds_state(
        crate::kds::KdsSubsystem::Storage,
        operation_id,
        root_state as u64,
        crate::kds::KdsSeverity::Info,
        [partitions.len() as u64, operating_systems.len() as u64],
    );

    StoragePlatformReport {
        operation_id,
        disk,
        mbr_valid: diagnostic.mbr_valid,
        gpt_valid: diagnostic.gpt_valid,
        root_state,
        partitions,
        filesystems,
        operating_systems,
    }
}

pub fn typed_storage_model() -> StorageModel {
    let report = scan_storage();
    StorageModel::from_report(&report, DEFAULT_INSTALL_REQUIREMENTS)
}

pub fn storage_graph() -> StorageGraph {
    let model = typed_storage_model();
    storage_graph_from_model(&model)
}

pub fn storage_graph_from_model(model: &StorageModel) -> StorageGraph {
    let classification = graph_classification(model);
    let mut partitions = Vec::new();
    for partition in &model.partitions {
        partitions.push(StorageGraphPartition {
            index: partition.index,
            classification: partition_classification(model, partition),
            filesystem: partition.filesystem,
            start_lba: partition.start_lba,
            size_lba: partition.size_lba,
            evidence: filesystem_evidence(partition.filesystem),
        });
    }

    let has_saios = model
        .operating_systems
        .iter()
        .any(|os| os.kind == OperatingSystemKind::Saios);
    let root_partition = model
        .partitions
        .iter()
        .find(|partition| {
            partition.type_code == 0x83 && partition.filesystem == FilesystemKind::Ext4
        })
        .map(|partition| partition.index);
    let snapshot_report = if has_saios {
        Some(snapshot_store_report())
    } else {
        None
    };
    let has_snapshot = snapshot_report
        .as_ref()
        .and_then(|report| report.latest_snapshot_id)
        .is_some();
    let mut slots = Vec::new();
    slots.push(SaiosSlot {
        slot: SaiosSlotId::A,
        state: if has_snapshot {
            SaiosSlotState::Previous
        } else if has_saios {
            SaiosSlotState::Active
        } else {
            SaiosSlotState::Missing
        },
        partition_index: root_partition,
        bootable: has_saios,
        evidence: if has_saios {
            if has_snapshot {
                "SystemSnapshot metadata marks Slot A as rollback previous"
            } else {
                "installed SAIOS evidence maps to active Slot A compatibility view"
            }
        } else {
            "no installed SAIOS slot evidence discovered"
        },
    });
    slots.push(SaiosSlot {
        slot: SaiosSlotId::B,
        state: if has_snapshot {
            SaiosSlotState::Candidate
        } else if has_saios {
            SaiosSlotState::Empty
        } else {
            SaiosSlotState::Missing
        },
        partition_index: None,
        bootable: has_saios,
        evidence: if has_snapshot {
            "SystemSnapshot metadata marks Slot B as update candidate"
        } else if has_saios {
            "inactive Slot B metadata is available as an SPC update target placeholder"
        } else {
            "inactive update Slot B is not present because no SAIOS install was discovered"
        },
    });

    let mut boot_entries = Vec::new();
    for os in &model.operating_systems {
        boot_entries.push(BootEntry {
            name: os.kind.label(),
            partition_index: os.efi_partition_index.or(os.partition_index),
            preferred: os.kind == OperatingSystemKind::Saios,
            evidence: os.evidence,
        });
    }

    let mut evidence = Vec::new();
    evidence.push(StorageEvidence {
        subject: "StorageGraph",
        detail: "graph built from DeviceContract block evidence and VfsContract filesystem probes",
        confidence: if model.disk.is_some() {
            "high"
        } else {
            "medium"
        },
    });
    evidence.push(StorageEvidence {
        subject: "classification",
        detail: classification.label(),
        confidence: if classification == StorageClassification::Unknown {
            "low"
        } else {
            "high"
        },
    });

    crate::observability_contract::ObservabilityContract::kds_state(
        crate::kds::KdsSubsystem::Storage,
        model.operation_id,
        classification as u64,
        crate::kds::KdsSeverity::Info,
        [partitions.len() as u64, slots.len() as u64],
    );

    StorageGraph {
        operation_id: model.operation_id,
        disk: model.disk.clone(),
        classification,
        partitions,
        operating_systems: model.operating_systems.clone(),
        slots,
        boot_entries,
        evidence,
    }
}

pub fn execution_plan(intent: StorageIntent) -> ExecutionPlan {
    let assessment = assess_storage();
    let graph = storage_graph_from_model(&assessment.model);
    match intent {
        StorageIntent::Install => execution_plan_from_operation(
            intent,
            graph,
            &plan_install_from_views(
                &analyze_hardware_from_assessment(&assessment),
                &analyze_install_target_from_assessment(&assessment),
            ),
        ),
        StorageIntent::Update => execution_plan_from_operation(intent, graph, &plan_update()),
        StorageIntent::Recover => recovery_execution_plan(graph),
        StorageIntent::Rollback => rollback_execution_plan(graph),
        StorageIntent::Diagnose => diagnose_execution_plan(graph),
    }
}

fn execution_plan_from_operation(
    intent: StorageIntent,
    graph: StorageGraph,
    operation: &OperationPlan,
) -> ExecutionPlan {
    let gates = validation_gates_for(intent, &graph, operation);
    let execution_enabled = operation.execution_enabled;
    let mut steps = Vec::new();
    for action in &operation.operations {
        let step = steps.len() + 1;
        steps.push(ExecutionPlanStep {
            step,
            action,
            affected: "SPC-approved storage graph node",
            verification: "step completion must emit KDS evidence and pass final verification",
            rollback_action: if intent == StorageIntent::Update {
                "restore boot preference from SystemSnapshot and keep previous slot bootable"
            } else {
                "stop execution and route recovery through SPC"
            },
        });
    }
    if steps.is_empty() {
        steps.push(ExecutionPlanStep {
            step: 1,
            action: "no mutating storage operation selected",
            affected: "StorageGraph",
            verification: "diagnostic KDS evidence exists",
            rollback_action: "not required",
        });
    }
    ExecutionPlan {
        plan_id: operation.operation_id,
        intent,
        graph,
        risk: operation.risk,
        execution_enabled,
        gates,
        steps,
        approval_required: intent != StorageIntent::Diagnose,
        refusal_reason: operation.refusal_reason,
    }
}

fn recovery_execution_plan(graph: StorageGraph) -> ExecutionPlan {
    let operation = OperationPlan {
        operation_id: graph.operation_id,
        execution_enabled: false,
        risk: PlatformRisk::Medium,
        rollback_feasible: true,
        estimated_seconds: 0,
        operations: alloc::vec![
            "analyze storage graph",
            "open KDS viewer",
            "open Flight Recorder viewer",
            "generate repair or rollback plan",
        ],
        refusal_reason: Some("recovery execution requires an approved repair or rollback sub-plan"),
    };
    execution_plan_from_operation(StorageIntent::Recover, graph, &operation)
}

fn rollback_execution_plan(graph: StorageGraph) -> ExecutionPlan {
    let has_previous = graph
        .slots
        .iter()
        .any(|slot| slot.state == SaiosSlotState::Previous && slot.bootable);
    let operation = OperationPlan {
        operation_id: graph.operation_id,
        execution_enabled: has_previous,
        risk: if has_previous {
            PlatformRisk::Medium
        } else {
            PlatformRisk::Critical
        },
        rollback_feasible: has_previous,
        estimated_seconds: if has_previous { 20 } else { 0 },
        operations: alloc::vec![
            "validate SystemSnapshot metadata",
            "restore previous boot preference",
            "verify previous slot bootability",
        ],
        refusal_reason: if has_previous {
            None
        } else {
            Some("rollback requires a bootable previous SAIOS slot")
        },
    };
    execution_plan_from_operation(StorageIntent::Rollback, graph, &operation)
}

fn diagnose_execution_plan(graph: StorageGraph) -> ExecutionPlan {
    let operation = OperationPlan {
        operation_id: graph.operation_id,
        execution_enabled: true,
        risk: PlatformRisk::Low,
        rollback_feasible: true,
        estimated_seconds: 0,
        operations: alloc::vec![
            "build StorageGraph",
            "classify disks and operating systems",
            "emit KDS diagnostic report",
        ],
        refusal_reason: None,
    };
    execution_plan_from_operation(StorageIntent::Diagnose, graph, &operation)
}

fn validation_gates_for(
    intent: StorageIntent,
    graph: &StorageGraph,
    operation: &OperationPlan,
) -> Vec<StorageGateResult> {
    let has_disk = graph.disk.is_some();
    let has_saios = graph
        .operating_systems
        .iter()
        .any(|os| os.kind == OperatingSystemKind::Saios);
    let has_efi = graph
        .partitions
        .iter()
        .any(|partition| partition.filesystem == FilesystemKind::Fat32);
    let has_active_slot = graph
        .slots
        .iter()
        .any(|slot| slot.state == SaiosSlotState::Active && slot.bootable);
    let has_inactive_slot = graph.slots.iter().any(|slot| {
        matches!(
            slot.state,
            SaiosSlotState::Candidate | SaiosSlotState::Empty | SaiosSlotState::Previous
        )
    });
    let snapshot_store = if matches!(intent, StorageIntent::Update | StorageIntent::Rollback) {
        Some(snapshot_store_report())
    } else {
        None
    };

    let mut gates = Vec::new();
    gates.push(gate(
        StorageGateKind::Storage,
        has_disk && graph.classification != StorageClassification::Unknown,
        if has_disk {
            "storage graph contains a supported disk"
        } else {
            "storage graph contains no supported disk"
        },
        "storage discovery failed or graph classification is unknown",
    ));
    gates.push(gate(
        StorageGateKind::Filesystem,
        intent == StorageIntent::Diagnose
            || !graph.partitions.is_empty()
            || graph.classification == StorageClassification::Empty,
        "filesystem evidence was evaluated through SPC graph",
        "filesystem gate requires partition or empty-disk evidence",
    ));
    gates.push(gate(
        StorageGateKind::Boot,
        intent == StorageIntent::Install || intent == StorageIntent::Diagnose || has_efi,
        "boot entry and EFI evidence evaluated",
        "boot gate requires EFI evidence for this operation",
    ));
    gates.push(gate(
        StorageGateKind::Snapshot,
        !matches!(intent, StorageIntent::Update | StorageIntent::Rollback)
            || (has_active_slot
                && snapshot_store
                    .as_ref()
                    .is_some_and(|report| report.available)),
        "snapshot authority available for current slot evidence and metadata store",
        "snapshot gate requires an active SAIOS slot and writable metadata store",
    ));
    gates.push(gate(
        StorageGateKind::Rollback,
        intent != StorageIntent::Update || (operation.rollback_feasible && has_inactive_slot),
        "rollback gate evaluated slot reversibility",
        "update rollback gate requires A/B slot metadata before execution",
    ));
    gates.push(gate(
        StorageGateKind::Recovery,
        intent == StorageIntent::Diagnose || has_disk,
        "SAIOS recovery diagnostics can operate from discovered storage evidence",
        "recovery gate requires storage evidence",
    ));
    gates.push(gate(
        StorageGateKind::Verification,
        intent == StorageIntent::Diagnose
            || operation.execution_enabled
            || !operation.operations.is_empty()
            || has_saios,
        "verification gate produced an explainable result",
        "verification gate requires executable plan evidence or existing SAIOS evidence",
    ));
    gates
}

fn gate(
    gate: StorageGateKind,
    passed: bool,
    evidence: &'static str,
    failure: &'static str,
) -> StorageGateResult {
    StorageGateResult {
        gate,
        passed,
        evidence,
        blocking_reason: if passed { None } else { Some(failure) },
    }
}

fn graph_classification(model: &StorageModel) -> StorageClassification {
    if model.disk.is_none() {
        return StorageClassification::Unknown;
    }
    if matches!(model.root_state, RootFilesystemState::FilesystemCorrupt) {
        return StorageClassification::Corrupt;
    }
    if model.root_state == RootFilesystemState::PartitionTableMissing {
        return StorageClassification::Empty;
    }
    for os in &model.operating_systems {
        match os.kind {
            OperatingSystemKind::Saios => return StorageClassification::Saios,
            OperatingSystemKind::Windows => return StorageClassification::Windows,
            OperatingSystemKind::Linux => return StorageClassification::Linux,
            OperatingSystemKind::UnknownEfi => {}
        }
    }
    StorageClassification::Unknown
}

fn partition_classification(model: &StorageModel, partition: &Partition) -> StorageClassification {
    for os in &model.operating_systems {
        if os.partition_index == Some(partition.index)
            || os.efi_partition_index == Some(partition.index)
        {
            return match os.kind {
                OperatingSystemKind::Saios => StorageClassification::Saios,
                OperatingSystemKind::Windows => StorageClassification::Windows,
                OperatingSystemKind::Linux => StorageClassification::Linux,
                OperatingSystemKind::UnknownEfi => StorageClassification::Unknown,
            };
        }
    }
    match partition.filesystem {
        FilesystemKind::Unknown => StorageClassification::Unknown,
        _ => StorageClassification::Empty,
    }
}

pub fn assess_storage() -> StorageAssessment {
    let model = typed_storage_model();
    let has_disk = model.disk.is_some();
    let has_existing_os = !model.operating_systems.is_empty();
    let blank_disk = has_disk && model.root_state == RootFilesystemState::PartitionTableMissing;
    let unknown_state = has_disk && !blank_disk && !model.mbr_valid && !model.gpt_valid;
    let recovery_available = has_disk
        && (!model.partitions.is_empty()
            || matches!(model.root_state, RootFilesystemState::RootMounted));

    let capabilities = StorageCapability {
        install: blank_disk && !has_existing_os,
        resize: false,
        dual_boot: false,
        recovery: recovery_available,
        replace_existing: false,
    };

    let mut checks = Vec::new();
    checks.push(StorageAssessmentCheck {
        name: "disk discovered",
        passed: has_disk,
        evidence: if has_disk {
            "supported block device discovered"
        } else {
            "no supported block device discovered"
        },
    });
    checks.push(StorageAssessmentCheck {
        name: "typed partition model",
        passed: true,
        evidence: "partition report converted into typed storage model",
    });
    checks.push(StorageAssessmentCheck {
        name: "capability evaluation",
        passed: true,
        evidence: "install, resize, dual boot, recovery, and replacement capabilities evaluated",
    });

    let mut failures = Vec::new();
    let mut risks = Vec::new();
    let mut recommendations = Vec::new();
    if !has_disk {
        failures.push(StorageAssessmentFailure {
            reason: "no supported disk was discovered",
            evidence: "storage scan returned no disk model",
            recommendation: "attach a supported disk or check storage controller support",
        });
        risks.push("installation impossible without a supported disk");
        recommendations.push(StorageRecommendation {
            action: "run storage diagnose",
            reason: "disk discovery failed before planning",
        });
    } else if has_existing_os {
        failures.push(StorageAssessmentFailure {
            reason: "existing operating system detected",
            evidence: "operating-system evidence is present in typed storage model",
            recommendation: "warn about data loss, recommend backup, and require explicit user confirmation",
        });
        risks.push("existing operating system would be replaced by install");
        recommendations.push(StorageRecommendation {
            action: "run storage operating-systems",
            reason: "inspect read-only OS evidence before choosing an SPC install intent",
        });
    } else if unknown_state {
        failures.push(StorageAssessmentFailure {
            reason: "unknown disk state blocks modification",
            evidence: "disk exists without a recognized blank, MBR, or GPT state",
            recommendation: "collect diagnostics before attempting installation",
        });
        risks.push("unknown disk state has critical install risk");
        recommendations.push(StorageRecommendation {
            action: "run sairu diagnose storage",
            reason: "unknown storage state requires explanation before changes",
        });
    }
    if !capabilities.resize {
        risks.push("resize execution remains disabled");
    }
    if !capabilities.dual_boot && has_existing_os {
        risks.push("dual boot execution remains disabled");
    }

    let mut evidence = Vec::new();
    evidence.push(StorageEvidence {
        subject: "storage scan",
        detail: "typed model derived from read-only storage scan",
        confidence: "high",
    });
    if has_existing_os {
        evidence.push(StorageEvidence {
            subject: "operating systems",
            detail: "operating-system evidence affects install decision",
            confidence: "medium",
        });
    }

    let mut reasons = Vec::new();
    if capabilities.install && failures.is_empty() {
        reasons.push("blank-disk install capability available");
    } else if failures.is_empty() {
        reasons.push("installation is not currently allowed by storage capabilities");
    } else {
        for failure in &failures {
            reasons.push(failure.reason);
        }
    }

    let decision = InstallDecision {
        allowed: capabilities.install && failures.is_empty(),
        reasons,
        evidence: evidence.clone(),
        confidence: if has_disk { "high" } else { "medium" },
        confidence_score: if has_disk { 90 } else { 60 },
    };

    crate::observability_contract::ObservabilityContract::kds_state(
        crate::kds::KdsSubsystem::Storage,
        model.operation_id,
        decision.allowed as u64,
        crate::kds::KdsSeverity::Info,
        [capabilities.install as u64, failures.len() as u64],
    );

    StorageAssessment {
        operation_id: model.operation_id,
        model,
        checks,
        failures,
        risks,
        capabilities,
        decision,
        recommendations,
        evidence,
    }
}

pub fn validate_install_plan() -> InstallPlanValidation {
    decision_snapshot().validation
}

pub fn validate_install_plan_from_assessment(
    assessment: &StorageAssessment,
    plan: &OperationPlan,
) -> InstallPlanValidation {
    let mut checks = Vec::new();
    checks.push(InstallValidationCheck {
        name: "plan exists",
        passed: true,
        evidence: "operation plan generated before validation",
    });
    checks.push(InstallValidationCheck {
        name: "disk discovered",
        passed: assessment.model.disk.is_some(),
        evidence: if assessment.model.disk.is_some() {
            "typed storage model contains a disk"
        } else {
            "typed storage model contains no disk"
        },
    });
    checks.push(InstallValidationCheck {
        name: "install capability",
        passed: assessment.capabilities.install,
        evidence: if assessment.capabilities.install {
            "storage capabilities allow blank-disk install"
        } else {
            "storage capabilities do not allow install"
        },
    });
    checks.push(InstallValidationCheck {
        name: "assessment decision",
        passed: assessment.decision.allowed,
        evidence: if assessment.decision.allowed {
            "install decision allows execution"
        } else {
            "install decision blocks execution"
        },
    });
    checks.push(InstallValidationCheck {
        name: "plan execution",
        passed: plan.execution_enabled,
        evidence: if plan.execution_enabled {
            "operation plan execution is enabled"
        } else {
            "operation plan execution is disabled"
        },
    });
    checks.push(InstallValidationCheck {
        name: "required operations",
        passed: !plan.operations.is_empty() || !plan.execution_enabled,
        evidence: if !plan.operations.is_empty() {
            "operation plan contains required operations"
        } else {
            "operation plan has no operations"
        },
    });

    let mut failures = Vec::new();
    for failure in &assessment.failures {
        failures.push(InstallValidationFailure {
            reason: failure.reason,
            evidence: failure.evidence,
            suggested_fix: failure.recommendation,
        });
    }
    if !plan.execution_enabled {
        failures.push(InstallValidationFailure {
            reason: plan
                .refusal_reason
                .unwrap_or("operation plan execution disabled"),
            evidence: "plan_install refused execution",
            suggested_fix: "review storage analyze and storage recommend before confirming install",
        });
    }
    if assessment.capabilities.install && !plan.execution_enabled {
        failures.push(InstallValidationFailure {
            reason: "capability and plan disagree",
            evidence: "assessment allows install but operation plan refused execution",
            suggested_fix: "inspect compatibility and target analysis before install approval",
        });
    }

    let status = if failures.is_empty() && checks.iter().all(|check| check.passed) {
        InstallValidationStatus::Passed
    } else {
        InstallValidationStatus::Failed
    };
    let mut suggested_fixes = Vec::new();
    for failure in &failures {
        if !suggested_fixes.contains(&failure.suggested_fix) {
            suggested_fixes.push(failure.suggested_fix);
        }
    }

    InstallPlanValidation {
        operation_id: assessment.operation_id,
        status,
        checks,
        failures,
        suggested_fixes,
    }
}

pub fn assess_install_risk() -> RiskAssessment {
    decision_snapshot().risk
}

pub fn assess_install_risk_from_assessment(
    assessment: &StorageAssessment,
    validation: &InstallPlanValidation,
) -> RiskAssessment {
    let mut factors = Vec::new();
    let mut recommendations = Vec::new();

    for failure in &assessment.failures {
        factors.push(RiskFactor {
            level: risk_level_for_failure(failure.reason),
            reason: failure.reason,
            evidence: failure.evidence,
        });
        recommendations.push(RiskRecommendation {
            action: failure.recommendation,
            reason: failure.reason,
        });
    }
    if validation.status == InstallValidationStatus::Failed {
        factors.push(RiskFactor {
            level: RiskLevel::High,
            reason: "install validation failed",
            evidence: "validation view rejected the current operation plan",
        });
        recommendations.push(RiskRecommendation {
            action: "run storage validate",
            reason: "validation failure must be resolved before approval",
        });
    }
    if !assessment.capabilities.resize {
        factors.push(RiskFactor {
            level: RiskLevel::Low,
            reason: "resize execution disabled",
            evidence: "storage capabilities report resize=false",
        });
    }
    if !assessment.capabilities.dual_boot && !assessment.model.operating_systems.is_empty() {
        factors.push(RiskFactor {
            level: RiskLevel::High,
            reason: "dual boot execution disabled with existing operating system evidence",
            evidence: "storage capabilities report dual_boot=false",
        });
        recommendations.push(RiskRecommendation {
            action: "use planner-only dual boot analysis",
            reason: "existing operating systems block automatic modification",
        });
    }
    if assessment.decision.allowed && validation.status == InstallValidationStatus::Passed {
        recommendations.push(RiskRecommendation {
            action: "continue to explicit user approval",
            reason: "blank-disk install validation and assessment allow execution",
        });
    }

    let level = factors
        .iter()
        .map(|factor| factor.level)
        .max()
        .unwrap_or(RiskLevel::Low);
    let score = risk_score(level, factors.len());

    RiskAssessment {
        operation_id: assessment.operation_id,
        completed: true,
        level,
        score,
        factors,
        recommendations,
    }
}

pub fn simulate_install() -> InstallSimulation {
    decision_snapshot().simulation
}

pub fn simulate_install_from_views(
    assessment: &StorageAssessment,
    plan: &OperationPlan,
    validation: &InstallPlanValidation,
) -> InstallSimulation {
    let mut actions = Vec::new();
    if plan.operations.is_empty() {
        actions.push(SimulatedAction {
            step: 1,
            action: "No destructive install operation selected",
            detail: "planner did not produce disk modification actions for the current target",
        });
    } else {
        for operation in &plan.operations {
            let step = actions.len() + 1;
            actions.push(SimulatedAction {
                step,
                action: operation,
                detail: simulated_action_detail(operation),
            });
        }
    }

    let blocked = !plan.execution_enabled;
    let blocked_reason = if blocked {
        plan.refusal_reason
    } else {
        None
    };

    InstallSimulation {
        operation_id: plan.operation_id,
        blocked,
        blocked_reason,
        actions,
        no_changes_made: true,
    }
}

pub fn recommend_install_mode() -> InstallModeRecommendation {
    decision_snapshot().recommendation
}

pub fn recommend_install_mode_from_views(
    assessment: &StorageAssessment,
    validation: &InstallPlanValidation,
    risk: &RiskAssessment,
) -> InstallModeRecommendation {
    let mut reasons = Vec::new();
    let mode = if assessment.decision.allowed
        && validation.status == InstallValidationStatus::Passed
        && risk.level <= RiskLevel::Medium
    {
        reasons.push("blank-disk install capability available");
        InstallMode::FreshDisk
    } else if assessment
        .model
        .operating_systems
        .iter()
        .any(|os| os.kind == OperatingSystemKind::Saios)
    {
        reasons.push("existing SAIOS evidence detected; replacement remains conservative");
        InstallMode::Recovery
    } else if !assessment.model.operating_systems.is_empty() {
        reasons.push("existing operating system detected; automatic dual boot remains disabled");
        InstallMode::Safe
    } else if assessment.capabilities.recovery {
        reasons.push("recovery diagnostics are available for the current disk state");
        InstallMode::Recovery
    } else {
        reasons.push("insufficient evidence for destructive installation");
        InstallMode::Safe
    };

    if !assessment.capabilities.resize {
        reasons.push("resize execution remains disabled");
    }
    if risk.level >= RiskLevel::High {
        reasons.push("risk assessment requires conservative handling");
    }

    let confidence = match mode {
        InstallMode::FreshDisk => 92,
        InstallMode::Recovery => 78,
        InstallMode::Safe => 72,
        InstallMode::DualBoot => 0,
        InstallMode::ReplaceExistingSaios => 0,
    };

    InstallModeRecommendation {
        operation_id: assessment.operation_id,
        mode,
        confidence,
        reasons,
        evidence: assessment.evidence.clone(),
    }
}

pub fn analyze_hardware() -> CompatibilityReport {
    let assessment = assess_storage();
    analyze_hardware_from_assessment(&assessment)
}

pub fn analyze_hardware_from_assessment(assessment: &StorageAssessment) -> CompatibilityReport {
    let pci = crate::driver::pci::scan();
    let (total_frames, free_frames, _) = crate::memory::frame_stats();
    let cpu = cpu_features();
    let storage_pass = assessment.model.disk.is_some();
    let filesystem_pass = matches!(
        assessment.model.root_state,
        RootFilesystemState::PartitionTableMissing
            | RootFilesystemState::FilesystemValid
            | RootFilesystemState::RootMounted
    );
    let boot_pass = true;
    let device_pass = pci.iter().any(|dev| dev.class == 0x01) || storage_pass;
    let memory_pass = total_frames >= 4096 && free_frames > 256;
    let cpu_pass = cpu.required_pass();

    let mut score = 0u8;
    for pass in [
        cpu_pass,
        memory_pass,
        storage_pass,
        boot_pass,
        filesystem_pass,
        device_pass,
    ] {
        if pass {
            score = score.saturating_add(16);
        }
    }
    if score > 100 {
        score = 100;
    }
    let mut critical_failures = 0u8;
    if !cpu_pass {
        critical_failures += 1;
    }
    if !memory_pass {
        critical_failures += 1;
    }
    if !storage_pass {
        critical_failures += 1;
    }
    let warnings = (!cpu.avx2 as u8) + (!cpu.x2apic as u8) + (!device_pass as u8);
    let event_type = if critical_failures == 0 && score >= 70 {
        crate::kds::KdsEventType::CompatibilityPass
    } else if critical_failures == 0 {
        crate::kds::KdsEventType::CompatibilityWarning
    } else {
        crate::kds::KdsEventType::CompatibilityFailure
    };
    emit_event(
        event_type,
        if critical_failures == 0 {
            crate::kds::KdsSeverity::Info
        } else {
            crate::kds::KdsSeverity::Error
        },
        [
            assessment.operation_id,
            score as u64,
            critical_failures as u64,
            warnings as u64,
        ],
    );

    CompatibilityReport {
        operation_id: assessment.operation_id,
        score,
        critical_failures,
        warnings,
        cpu_pass,
        memory_pass,
        storage_pass,
        boot_pass,
        filesystem_pass,
        device_pass,
        summary: if critical_failures == 0 && score >= 70 {
            "compatibility threshold satisfied"
        } else {
            "compatibility concerns detected; user confirmation required"
        },
    }
}

pub fn analyze_install_target() -> InstallTargetAnalysis {
    let assessment = assess_storage();
    analyze_install_target_from_assessment(&assessment)
}

pub fn analyze_install_target_from_assessment(
    assessment: &StorageAssessment,
) -> InstallTargetAnalysis {
    let model = &assessment.model;
    let mut required_operations = Vec::new();
    let (classification, risk, safe_install_target, dual_boot_required, blocked_reason) =
        if model.disk.is_none() {
            (
                "No Disk",
                PlatformRisk::Critical,
                false,
                false,
                Some("no supported disk was discovered"),
            )
        } else if model
            .operating_systems
            .iter()
            .any(|os| os.kind == OperatingSystemKind::Windows)
        {
            (
                "Existing Windows",
                PlatformRisk::High,
                false,
                true,
                Some("existing Windows partitions will be removed if the user confirms"),
            )
        } else if model
            .operating_systems
            .iter()
            .any(|os| os.kind == OperatingSystemKind::Linux)
        {
            (
                "Existing Linux",
                PlatformRisk::High,
                false,
                true,
                Some("existing Linux partitions will be removed if the user confirms"),
            )
        } else if model
            .operating_systems
            .iter()
            .any(|os| os.kind == OperatingSystemKind::Saios)
        {
            (
                "Existing SAIOS",
                PlatformRisk::Medium,
                false,
                false,
                Some("existing SAIOS data will be replaced if the user confirms"),
            )
        } else if model.root_state == RootFilesystemState::PartitionTableMissing {
            ("Empty Disk", PlatformRisk::Low, true, false, None)
        } else if model.mbr_valid || model.gpt_valid {
            (
                "Existing EFI or Unknown Partition Table",
                PlatformRisk::High,
                false,
                true,
                Some("existing partitions will be removed if the user confirms"),
            )
        } else {
            (
                "Unknown Disk State",
                PlatformRisk::High,
                false,
                false,
                Some("filesystem could not be identified; proceeding may destroy existing data"),
            )
        };

    if model.disk.is_some() {
        required_operations.push("capture original MBR rollback point");
        required_operations.push("remove existing partition table entries");
        required_operations.push("create EFI system partition");
        required_operations.push("create ext4 root partition");
        required_operations.push("format FAT32/FAT ESP");
        required_operations.push("format ext4 root");
        required_operations.push("install boot files");
        required_operations.push("verify rootfs and boot files");
    }

    if blocked_reason.is_some() {
        emit_event(
            crate::kds::KdsEventType::CompatibilityWarning,
            crate::kds::KdsSeverity::Warn,
            [
                assessment.operation_id,
                risk as u64,
                dual_boot_required as u64,
                0,
            ],
        );
    }

    InstallTargetAnalysis {
        operation_id: assessment.operation_id,
        classification,
        risk,
        safe_install_target,
        dual_boot_required,
        required_operations,
        blocked_reason,
    }
}

pub fn plan_install() -> OperationPlan {
    decision_snapshot().plan
}

pub fn plan_install_from_views(
    compatibility: &CompatibilityReport,
    target: &InstallTargetAnalysis,
) -> OperationPlan {
    let mut operations = Vec::new();
    for operation in &target.required_operations {
        operations.push(*operation);
    }
    let execution_enabled = target.classification != "No Disk";
    let refusal_reason = if execution_enabled {
        None
    } else {
        Some("no supported disk was discovered")
    };
    OperationPlan {
        operation_id: target.operation_id,
        execution_enabled,
        risk: target.risk,
        rollback_feasible: execution_enabled && compatibility.storage_pass,
        estimated_seconds: if execution_enabled { 120 } else { 0 },
        operations,
        refusal_reason,
    }
}

pub fn decision_snapshot() -> StorageDecisionSnapshot {
    let assessment = assess_storage();
    let compatibility = analyze_hardware_from_assessment(&assessment);
    let target = analyze_install_target_from_assessment(&assessment);
    let plan = plan_install_from_views(&compatibility, &target);
    let validation = validate_install_plan_from_assessment(&assessment, &plan);
    let risk = assess_install_risk_from_assessment(&assessment, &validation);
    let simulation = simulate_install_from_views(&assessment, &plan, &validation);
    let recommendation = recommend_install_mode_from_views(&assessment, &validation, &risk);
    StorageDecisionSnapshot {
        operation_id: assessment.operation_id,
        assessment,
        compatibility,
        target,
        plan,
        validation,
        risk,
        simulation,
        recommendation,
    }
}

pub fn plan_update() -> OperationPlan {
    let assessment = assess_storage();
    let compatibility = analyze_hardware_from_assessment(&assessment);
    let operations = alloc::vec![
        "analyze update target",
        "recommend backup before overwrite",
        "report compatibility concerns",
        "capture original MBR rollback point",
        "remove existing partition table entries",
        "create EFI system partition",
        "create ext4 root partition",
        "format FAT32/FAT ESP",
        "format ext4 root",
        "install updated boot files and rootfs",
        "flush and record update evidence",
    ];

    let has_disk = assessment.model.disk.is_some();
    let has_saios = assessment
        .model
        .operating_systems
        .iter()
        .any(|os| os.kind == OperatingSystemKind::Saios);
    let has_efi = assessment
        .model
        .partitions
        .iter()
        .any(|partition| partition.type_code == 0xEF);
    let layout_validation = validate_update_target_layout(&assessment.model);
    let layout_valid = layout_validation.is_ok();
    let recovery_ready = assessment.capabilities.recovery;
    let snapshot_store = snapshot_store_report();
    let snapshot_ready = snapshot_store.available;
    let compatibility_ready = compatibility.score >= 70 && compatibility.critical_failures == 0;
    let execution_enabled = has_disk;
    let refusal_reason = if execution_enabled {
        None
    } else {
        Some("no supported disk was discovered")
    };

    OperationPlan {
        operation_id: assessment.operation_id,
        execution_enabled,
        risk: if !has_disk {
            PlatformRisk::Critical
        } else if has_saios
            && has_efi
            && layout_valid
            && recovery_ready
            && snapshot_ready
            && compatibility_ready
        {
            PlatformRisk::Medium
        } else {
            PlatformRisk::High
        },
        rollback_feasible: snapshot_ready,
        estimated_seconds: if execution_enabled { 120 } else { 0 },
        operations,
        refusal_reason,
    }
}

fn validate_update_target_layout(model: &StorageModel) -> Result<(), &'static str> {
    let disk = model
        .disk
        .as_ref()
        .ok_or("update requires a supported disk")?;
    let (esp, root) = update_layout_partitions(model)?;
    if esp.size_lba == 0 || esp.size_lba > UPDATE_ESP_MAX_SECTORS {
        return Err("update requires a bounded installed SAIOS ESP");
    }
    let esp_end = esp.start_lba.saturating_add(esp.size_lba);
    if esp_end > disk.sector_count {
        return Err("update ESP exceeds disk bounds");
    }
    if root.start_lba.saturating_add(root.size_lba) > disk.sector_count {
        return Err("update root partition exceeds disk bounds");
    }
    if !matches!(
        model.root_state,
        RootFilesystemState::FilesystemValid | RootFilesystemState::RootMounted
    ) {
        return Err("update requires a valid installed SAIOS root filesystem");
    }
    Ok(())
}

fn update_layout_partitions(
    model: &StorageModel,
) -> Result<(&Partition, &Partition), &'static str> {
    let esp = model
        .partitions
        .iter()
        .find(|partition| {
            partition.table == PartitionTableKind::Mbr
                && partition.type_code == 0xEF
                && partition.start_lba == UPDATE_ESP_START_LBA
        })
        .ok_or("update requires the installed SAIOS ESP layout")?;
    let esp_end = esp.start_lba.saturating_add(esp.size_lba);
    let root = model
        .partitions
        .iter()
        .find(|partition| {
            partition.table == PartitionTableKind::Mbr
                && partition.type_code == 0x83
                && partition.start_lba == esp_end
                && partition.size_lba >= UPDATE_MIN_ROOT_SECTORS
                && partition.filesystem == FilesystemKind::Ext4
        })
        .ok_or("update requires an installed SAIOS ext4 root partition")?;
    Ok((esp, root))
}

pub fn begin_update_preservation(
    plan: &OperationPlan,
) -> Result<UpdatePreservationSnapshot, &'static str> {
    let model = typed_storage_model();
    validate_update_target_layout(&model)?;
    let (esp_partition, root_partition) = update_layout_partitions(&model)?;
    let (canonical_config, compatibility_config) = installed_config_identities()?;
    let dev = block::get().ok_or("update snapshot requires a disk")?;
    let previous_boot_policy = read_fat16_file(
        &*dev,
        esp_partition.start_lba,
        &["BOOT", "GRUB", "GRUB.CFG"],
    )?;
    if previous_boot_policy.len() > UPDATE_BOOT_POLICY_MAX_BYTES {
        return Err("update snapshot boot policy is too large");
    }
    let previous_boot_policy_hash = stable_hash(&previous_boot_policy);
    let configuration_state_hash = canonical_config.inode
        ^ compatibility_config.inode
        ^ canonical_config.size as u64
        ^ compatibility_config.size as u64;
    let system_snapshot = SystemSnapshot {
        snapshot_id: plan.operation_id ^ previous_boot_policy_hash,
        build_id: crate::version::SAIOS_VERSION_TAG,
        kernel_version: crate::version::SAIOS_VERSION,
        created_at_ns: crate::time::uptime_ns(),
        source_slot: SaiosSlotId::A,
        target_slot: SaiosSlotId::B,
        root_start_lba: root_partition.start_lba,
        root_size_lba: root_partition.size_lba,
        configuration_state_hash,
        boot_state_hash: previous_boot_policy_hash,
        rollback_eligible: true,
    };
    persist_system_snapshot(&system_snapshot)?;
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationProgress,
        crate::kds::KdsSeverity::Info,
        [
            plan.operation_id,
            root_partition.start_lba,
            root_partition.size_lba,
            1,
        ],
    );
    Ok(UpdatePreservationSnapshot {
        operation_id: plan.operation_id,
        snapshot_id: system_snapshot.snapshot_id,
        build_id: crate::version::SAIOS_VERSION_TAG,
        kernel_version: crate::version::SAIOS_VERSION,
        active_slot: SaiosSlotId::A,
        rollback_slot: system_snapshot.source_slot,
        root_start_lba: root_partition.start_lba,
        root_size_lba: root_partition.size_lba,
        canonical_config,
        compatibility_config,
        previous_boot_policy,
        previous_boot_policy_hash,
        system_snapshot,
    })
}

pub fn seed_installed_metadata(
    root: &Arc<Inode>,
    operation_id: u64,
    root_start_lba: u64,
    root_size_lba: u64,
) -> Result<(), &'static str> {
    crate::vfs_contract::VfsContract::ensure_install_dir(root, SPC_METADATA_DIR)?;
    let slot_metadata = alloc::format!(
        "version=1\nactive_slot=A\nslot_a_state=active\nslot_a_start_lba={}\nslot_a_size_lba={}\nslot_b_state=empty\nrollback_slot=A\noperation_id={}\n",
        root_start_lba,
        root_size_lba,
        operation_id
    );
    crate::vfs_contract::VfsContract::write_install_file(
        root,
        SPC_SLOT_METADATA_PATH,
        slot_metadata.as_bytes(),
        0o644,
    )?;
    let initial_snapshot = SystemSnapshot {
        snapshot_id: operation_id ^ root_start_lba ^ root_size_lba,
        build_id: crate::version::SAIOS_VERSION_TAG,
        kernel_version: crate::version::SAIOS_VERSION,
        created_at_ns: crate::time::uptime_ns(),
        source_slot: SaiosSlotId::A,
        target_slot: SaiosSlotId::B,
        root_start_lba,
        root_size_lba,
        configuration_state_hash: operation_id ^ root_size_lba,
        boot_state_hash: operation_id ^ root_start_lba,
        rollback_eligible: true,
    };
    crate::vfs_contract::VfsContract::write_install_file(
        root,
        SPC_LATEST_SNAPSHOT_PATH,
        serialize_system_snapshot(&initial_snapshot).as_bytes(),
        0o644,
    )?;
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationProgress,
        crate::kds::KdsSeverity::Info,
        [
            operation_id,
            initial_snapshot.snapshot_id,
            root_start_lba,
            root_size_lba,
        ],
    );
    Ok(())
}

pub fn snapshot_store_report() -> SnapshotStoreReport {
    let operation_id = emit_event(
        crate::kds::KdsEventType::DiskOperationProgress,
        crate::kds::KdsSeverity::Info,
        [0x5350_4353, 0, 0, 0],
    );
    let mut latest_snapshot_id = None;
    let available = block::get()
        .and_then(|dev| crate::vfs_contract::VfsContract::mount_install_rootfs(dev).ok())
        .map(|root| {
            latest_snapshot_id = read_install_path(&root, SPC_LATEST_SNAPSHOT_PATH, 4096)
                .and_then(|bytes| parse_snapshot_id(&bytes));
        })
        .is_some();
    SnapshotStoreReport {
        operation_id,
        metadata_dir: SPC_METADATA_DIR,
        slot_metadata_path: SPC_SLOT_METADATA_PATH,
        latest_snapshot_path: SPC_LATEST_SNAPSHOT_PATH,
        available,
        latest_snapshot_id,
        evidence: if available {
            "snapshot metadata store root is mountable through VfsContract install-root path"
        } else {
            "snapshot metadata store root is not mountable from current storage state"
        },
    }
}

fn lookup_install_path(root: &Arc<Inode>, path: &str) -> Option<Arc<Inode>> {
    let mut current = root.clone();
    for segment in path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        current = current.ops.lookup(segment).ok()?;
    }
    Some(current)
}

fn read_install_path(root: &Arc<Inode>, path: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let inode = lookup_install_path(root, path)?;
    let stat = inode.ops.stat().ok()?;
    if stat.st_size < 0 {
        return None;
    }
    let size = core::cmp::min(stat.st_size as usize, max_bytes);
    let mut data = alloc::vec![0u8; size];
    let read = inode.ops.read(0, &mut data).ok()?;
    data.truncate(read);
    Some(data)
}

fn parse_snapshot_id(bytes: &[u8]) -> Option<u64> {
    let text = core::str::from_utf8(bytes).ok()?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("snapshot_id=") {
            return value.parse::<u64>().ok();
        }
    }
    None
}

fn persist_system_snapshot(snapshot: &SystemSnapshot) -> Result<(), &'static str> {
    let dev = block::get().ok_or("snapshot store requires a disk")?;
    let root = crate::vfs_contract::VfsContract::mount_install_rootfs(dev)?;
    crate::vfs_contract::VfsContract::ensure_install_dir(&root, SPC_METADATA_DIR)?;
    crate::vfs_contract::VfsContract::write_install_file(
        &root,
        SPC_SLOT_METADATA_PATH,
        serialize_slot_metadata(snapshot).as_bytes(),
        0o644,
    )?;
    crate::vfs_contract::VfsContract::write_install_file(
        &root,
        SPC_LATEST_SNAPSHOT_PATH,
        serialize_system_snapshot(snapshot).as_bytes(),
        0o644,
    )?;
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationProgress,
        crate::kds::KdsSeverity::Info,
        [
            snapshot.snapshot_id,
            snapshot.root_start_lba,
            snapshot.root_size_lba,
            snapshot.rollback_eligible as u64,
        ],
    );
    Ok(())
}

fn serialize_slot_metadata(snapshot: &SystemSnapshot) -> String {
    alloc::format!(
        "version=1\nactive_slot={}\nslot_a_state=previous\nslot_a_start_lba={}\nslot_a_size_lba={}\nslot_b_state=candidate\nrollback_slot={}\nsnapshot_id={}\n",
        snapshot.target_slot.label(),
        snapshot.root_start_lba,
        snapshot.root_size_lba,
        snapshot.source_slot.label(),
        snapshot.snapshot_id
    )
}

fn serialize_system_snapshot(snapshot: &SystemSnapshot) -> String {
    alloc::format!(
        "version=1\nsnapshot_id={}\nbuild_id={}\nkernel_version={}\ncreated_at_ns={}\nsource_slot={}\ntarget_slot={}\nroot_start_lba={}\nroot_size_lba={}\nconfiguration_state_hash={}\nboot_state_hash={}\nrollback_eligible={}\n",
        snapshot.snapshot_id,
        snapshot.build_id,
        snapshot.kernel_version,
        snapshot.created_at_ns,
        snapshot.source_slot.label(),
        snapshot.target_slot.label(),
        snapshot.root_start_lba,
        snapshot.root_size_lba,
        snapshot.configuration_state_hash,
        snapshot.boot_state_hash,
        snapshot.rollback_eligible
    )
}

pub fn verify_update_preservation(
    snapshot: &UpdatePreservationSnapshot,
) -> Result<(), &'static str> {
    let model = typed_storage_model();
    validate_update_target_layout(&model)?;
    let (_, root_partition) = update_layout_partitions(&model)?;
    if root_partition.start_lba != snapshot.root_start_lba
        || root_partition.size_lba != snapshot.root_size_lba
    {
        return Err("update preservation check failed: root partition changed");
    }
    let (canonical_config, compatibility_config) = installed_config_identities()?;
    if canonical_config != snapshot.canonical_config
        || compatibility_config != snapshot.compatibility_config
    {
        return Err("update preservation check failed: config identity changed");
    }
    if stable_hash(&snapshot.previous_boot_policy) != snapshot.previous_boot_policy_hash {
        return Err("update preservation check failed: snapshot boot policy changed");
    }
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationProgress,
        crate::kds::KdsSeverity::Info,
        [
            snapshot.operation_id,
            snapshot.root_start_lba,
            snapshot.root_size_lba,
            2,
        ],
    );
    Ok(())
}

pub fn installed_update_boot_policy(plan: &OperationPlan) -> Result<Vec<u8>, &'static str> {
    let model = typed_storage_model();
    validate_update_target_layout(&model)?;
    let (esp, _) = update_layout_partitions(&model)?;
    let dev = block::get().ok_or("update boot policy requires a disk")?;
    let policy = read_fat16_file(&*dev, esp.start_lba, &["BOOT", "GRUB", "GRUB.CFG"])?;
    if policy.len() > UPDATE_BOOT_POLICY_MAX_BYTES {
        return Err("update boot policy is too large");
    }
    if !contains_bytes(&policy, b"multiboot2 /boot/saios.elf")
        || !contains_bytes(&policy, b"saios.boot=hdd")
    {
        return Err("update boot policy is not an installed SAIOS disk policy");
    }
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationProgress,
        crate::kds::KdsSeverity::Info,
        [plan.operation_id, esp.start_lba, esp.size_lba, 3],
    );
    Ok(policy)
}

pub fn staged_update_boot_policy(
    snapshot: &UpdatePreservationSnapshot,
) -> Result<Vec<u8>, &'static str> {
    if snapshot.previous_boot_policy.is_empty() {
        return Err("update snapshot has no previous boot policy");
    }
    if snapshot.previous_boot_policy.len() > UPDATE_BOOT_POLICY_MAX_BYTES {
        return Err("update snapshot boot policy is too large");
    }
    if stable_hash(&snapshot.previous_boot_policy) != snapshot.previous_boot_policy_hash {
        return Err("update snapshot boot policy hash mismatch");
    }
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationProgress,
        crate::kds::KdsSeverity::Info,
        [
            snapshot.operation_id,
            snapshot.snapshot_id,
            snapshot.active_slot as u64,
            snapshot.rollback_slot as u64,
        ],
    );
    Ok(snapshot.previous_boot_policy.clone())
}

#[derive(Clone, Copy)]
struct FatDirectoryEntry {
    first_cluster: u16,
    size: u32,
    directory: bool,
}

fn read_fat16_file(
    dev: &dyn block::BlockDevice,
    part_lba: u64,
    path: &[&str],
) -> Result<Vec<u8>, &'static str> {
    let mut bpb = [0u8; 512];
    dev.read_bytes(part_lba.saturating_mul(512), &mut bpb)
        .map_err(|_| "update boot policy read failed")?;
    if bpb[510] != 0x55 || bpb[511] != 0xAA {
        return Err("update boot policy requires a FAT ESP");
    }
    let bytes_per_sector = u16::from_le_bytes([bpb[11], bpb[12]]) as u64;
    let sectors_per_cluster = bpb[13] as u64;
    let reserved = u16::from_le_bytes([bpb[14], bpb[15]]) as u64;
    let fats = bpb[16] as u64;
    let root_entries = u16::from_le_bytes([bpb[17], bpb[18]]) as u64;
    let sectors_per_fat = u16::from_le_bytes([bpb[22], bpb[23]]) as u64;
    if bytes_per_sector != 512 || sectors_per_cluster == 0 || fats == 0 || sectors_per_fat == 0 {
        return Err("update boot policy FAT geometry is unsupported");
    }
    let root_dir_sectors = (root_entries * 32).div_ceil(bytes_per_sector);
    let root_lba = part_lba + reserved + fats * sectors_per_fat;
    let data_lba = root_lba + root_dir_sectors;
    let fat = Fat16ReadContext {
        dev,
        part_lba,
        bytes_per_sector,
        sectors_per_cluster,
        reserved,
        data_lba,
    };

    let mut directory = alloc::vec![0u8; (root_dir_sectors * bytes_per_sector) as usize];
    dev.read_bytes(root_lba.saturating_mul(bytes_per_sector), &mut directory)
        .map_err(|_| "update boot policy root directory read failed")?;

    let mut current = directory;
    for (index, component) in path.iter().enumerate() {
        let want_directory = index + 1 != path.len();
        let entry = find_fat_entry(&current, component, want_directory)
            .ok_or("update boot policy path missing")?;
        if want_directory {
            current =
                read_fat16_cluster_chain(&fat, entry.first_cluster, UPDATE_BOOT_POLICY_MAX_BYTES)?;
        } else {
            let mut file =
                read_fat16_cluster_chain(&fat, entry.first_cluster, entry.size as usize)?;
            file.truncate(entry.size as usize);
            return Ok(file);
        }
    }
    Err("update boot policy path missing")
}

fn find_fat_entry(
    directory: &[u8],
    component: &str,
    want_directory: bool,
) -> Option<FatDirectoryEntry> {
    let target = fat_83_name(component)?;
    for entry in directory.chunks_exact(32) {
        if entry[0] == 0x00 {
            return None;
        }
        if entry[0] == 0xE5 || entry[11] == 0x0F {
            continue;
        }
        let directory = entry[11] & 0x10 != 0;
        if directory != want_directory {
            continue;
        }
        if entry[0..11] == target {
            return Some(FatDirectoryEntry {
                first_cluster: u16::from_le_bytes([entry[26], entry[27]]),
                size: u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]),
                directory,
            });
        }
    }
    None
}

fn fat_83_name(component: &str) -> Option<[u8; 11]> {
    let mut out = [b' '; 11];
    let (name, ext) = component.split_once('.').unwrap_or((component, ""));
    if name.is_empty() || name.len() > 8 || ext.len() > 3 {
        return None;
    }
    for (index, byte) in name.bytes().enumerate() {
        out[index] = byte.to_ascii_uppercase();
    }
    for (index, byte) in ext.bytes().enumerate() {
        out[8 + index] = byte.to_ascii_uppercase();
    }
    Some(out)
}

struct Fat16ReadContext<'a> {
    dev: &'a dyn block::BlockDevice,
    part_lba: u64,
    bytes_per_sector: u64,
    sectors_per_cluster: u64,
    reserved: u64,
    data_lba: u64,
}

fn read_fat16_cluster_chain(
    fat: &Fat16ReadContext<'_>,
    first_cluster: u16,
    max_bytes: usize,
) -> Result<Vec<u8>, &'static str> {
    if first_cluster < 2 {
        return Ok(Vec::new());
    }
    let cluster_bytes = (fat.bytes_per_sector * fat.sectors_per_cluster) as usize;
    let mut data = Vec::new();
    let mut cluster = first_cluster;
    for _ in 0..256 {
        if data.len().saturating_add(cluster_bytes) > max_bytes.max(cluster_bytes) {
            return Err("update boot policy exceeds read limit");
        }
        let lba = fat.data_lba + (cluster as u64 - 2) * fat.sectors_per_cluster;
        let mut buf = alloc::vec![0u8; cluster_bytes];
        fat.dev
            .read_bytes(lba.saturating_mul(fat.bytes_per_sector), &mut buf)
            .map_err(|_| "update boot policy cluster read failed")?;
        data.extend_from_slice(&buf);
        let next = read_fat16_entry(
            fat.dev,
            fat.part_lba,
            fat.bytes_per_sector,
            fat.reserved,
            cluster,
        )?;
        if next >= 0xFFF8 {
            break;
        }
        if next < 2 {
            return Err("update boot policy FAT chain is invalid");
        }
        cluster = next;
    }
    Ok(data)
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn read_fat16_entry(
    dev: &dyn block::BlockDevice,
    part_lba: u64,
    bytes_per_sector: u64,
    reserved: u64,
    cluster: u16,
) -> Result<u16, &'static str> {
    let offset = part_lba
        .saturating_mul(bytes_per_sector)
        .saturating_add(reserved.saturating_mul(bytes_per_sector))
        .saturating_add(cluster as u64 * 2);
    let mut entry = [0u8; 2];
    dev.read_bytes(offset, &mut entry)
        .map_err(|_| "update boot policy FAT read failed")?;
    Ok(u16::from_le_bytes(entry))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn installed_config_identities() -> Result<(UpdateFileIdentity, UpdateFileIdentity), &'static str> {
    let dev = block::get().ok_or("update preservation check requires a disk")?;
    let fs = crate::fs::ext4::Ext4Fs::mount(dev)
        .map_err(|_| "update preservation check failed: ext4 mount")?;
    let root = crate::fs::ext4::Ext4Fs::root_inode(fs)
        .map_err(|_| "update preservation check failed: root inode")?;
    let canonical = stat_path_on(root.clone(), crate::config::CANONICAL_CONFIG_PATH)
        .map_err(|_| "update preservation check failed: canonical config missing")?;
    let compatibility = stat_path_on(root, crate::config::COMPAT_CONFIG_PATH)
        .map_err(|_| "update preservation check failed: compatibility config missing")?;
    Ok((
        UpdateFileIdentity::from_stat(canonical),
        UpdateFileIdentity::from_stat(compatibility),
    ))
}

impl UpdateFileIdentity {
    fn from_stat(stat: Stat) -> Self {
        Self {
            inode: stat.st_ino,
            mode: stat.st_mode,
            uid: stat.st_uid,
            gid: stat.st_gid,
            size: stat.st_size,
            blocks: stat.st_blocks,
            links: stat.st_nlink,
        }
    }
}

fn stat_path_on(mut current: Arc<Inode>, path: &str) -> Result<Stat, &'static str> {
    if path.is_empty() || path == "/" {
        return current
            .ops
            .stat()
            .map_err(|_| "update preservation check failed: stat");
    }
    for part in path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        current = current
            .ops
            .lookup(part)
            .map_err(|_| "update preservation check failed: lookup")?;
    }
    current
        .ops
        .stat()
        .map_err(|_| "update preservation check failed: stat")
}

pub fn resize_analysis() -> ResizeReport {
    ResizeReport {
        execution_enabled: false,
        safe: false,
        reason: "resize execution disabled until planner, validation, rollback, and KDS coverage are proven reliable",
    }
}

pub fn recovery_report() -> RecoveryReport {
    let report = scan_storage();
    emit_event(
        crate::kds::KdsEventType::RecoveryBegin,
        crate::kds::KdsSeverity::Info,
        [report.operation_id, 0, 0, 0],
    );
    let disk_diagnostics = report.disk.is_some();
    let partition_diagnostics = !report.partitions.is_empty();
    let filesystem_diagnostics = !report.filesystems.is_empty()
        || matches!(
            report.root_state,
            RootFilesystemState::PartitionTableMissing
        );
    emit_event(
        crate::kds::KdsEventType::RecoveryComplete,
        crate::kds::KdsSeverity::Info,
        [
            report.operation_id,
            disk_diagnostics as u64,
            partition_diagnostics as u64,
            filesystem_diagnostics as u64,
        ],
    );
    RecoveryReport {
        operation_id: report.operation_id,
        disk_diagnostics,
        partition_diagnostics,
        filesystem_diagnostics,
        efi_repair_available: partition_diagnostics,
        boot_repair_available: partition_diagnostics,
        rootfs_repair_available: matches!(
            report.root_state,
            RootFilesystemState::FilesystemCorrupt | RootFilesystemState::RootMounted
        ),
        summary: "SAIOS recovery available through StoragePlatformContract APIs; repair plans remain conservative",
    }
}

pub fn install_gate() -> Result<OperationPlan, &'static str> {
    let snapshot = decision_snapshot();
    let recovery = recovery_report();
    if snapshot.plan.execution_enabled {
        emit_event(
            crate::kds::KdsEventType::InstallApproved,
            if snapshot.plan.risk >= PlatformRisk::High {
                crate::kds::KdsSeverity::Warn
            } else {
                crate::kds::KdsSeverity::Info
            },
            [
                snapshot.plan.operation_id,
                snapshot.plan.operations.len() as u64,
                snapshot.plan.risk as u64,
                recovery.operation_id,
            ],
        );
        Ok(snapshot.plan)
    } else {
        emit_event(
            crate::kds::KdsEventType::DiskOperationFailure,
            crate::kds::KdsSeverity::Error,
            [
                snapshot.plan.operation_id,
                snapshot.plan.risk as u64,
                snapshot.validation.failures.len() as u64,
                recovery.disk_diagnostics as u64,
            ],
        );
        Err(snapshot.plan.refusal_reason.unwrap_or("no supported disk was discovered"))
    }
}

pub fn reinstall_gate() -> Result<OperationPlan, &'static str> {
    let assessment = assess_storage();
    let recovery = recovery_report();
    let has_saios = assessment
        .model
        .operating_systems
        .iter()
        .any(|os| os.kind == OperatingSystemKind::Saios);

    if assessment.model.disk.is_some() {
        let operations = vec![
            "capture original MBR rollback point",
            "remove existing partition table entries",
            "replace target EFI system partition",
            "replace target ext4 root partition",
            "format FAT32/FAT ESP",
            "format ext4 root",
            "install boot files",
            "seed authoritative root filesystem",
            "verify rootfs and boot files",
        ];
        let plan = OperationPlan {
            operation_id: assessment.operation_id,
            execution_enabled: true,
            risk: if has_saios {
                PlatformRisk::High
            } else {
                PlatformRisk::Critical
            },
            rollback_feasible: recovery.disk_diagnostics && recovery.filesystem_diagnostics,
            estimated_seconds: 120,
            operations,
            refusal_reason: None,
        };
        emit_event(
            crate::kds::KdsEventType::InstallApproved,
            crate::kds::KdsSeverity::Warn,
            [
                plan.operation_id,
                plan.operations.len() as u64,
                plan.risk as u64,
                recovery.operation_id,
            ],
        );
        Ok(plan)
    } else {
        emit_event(
            crate::kds::KdsEventType::DiskOperationFailure,
            crate::kds::KdsSeverity::Error,
            [
                assessment.operation_id,
                has_saios as u64,
                recovery.disk_diagnostics as u64,
                recovery.filesystem_diagnostics as u64,
            ],
        );
        Err("no supported disk was discovered")
    }
}

pub fn update_gate() -> Result<OperationPlan, &'static str> {
    let plan = plan_update();
    let recovery = recovery_report();
    if plan.execution_enabled {
        emit_event(
            crate::kds::KdsEventType::InstallApproved,
            if plan.risk >= PlatformRisk::High {
                crate::kds::KdsSeverity::Warn
            } else {
                crate::kds::KdsSeverity::Info
            },
            [
                plan.operation_id,
                plan.operations.len() as u64,
                plan.risk as u64,
                recovery.operation_id,
            ],
        );
        Ok(plan)
    } else {
        emit_event(
            crate::kds::KdsEventType::DiskOperationFailure,
            crate::kds::KdsSeverity::Error,
            [
                plan.operation_id,
                plan.risk as u64,
                recovery.disk_diagnostics as u64,
                recovery.partition_diagnostics as u64,
            ],
        );
        Err(plan
            .refusal_reason
            .unwrap_or("no supported disk was discovered"))
    }
}

pub fn diagnostic_for_install_block() -> SairuDiagnostic {
    let target = analyze_install_target();
    SairuDiagnostic {
        failure_type: "InstallAdvisory",
        confidence: if target.blocked_reason.is_some() {
            "high"
        } else {
            "medium"
        },
        evidence: target.classification,
        likely_cause: target
            .blocked_reason
            .unwrap_or("target requires explicit user confirmation"),
        recommendation: if target.safe_install_target {
            "review operation plan and approve blank-disk install"
        } else {
            "recommend backup, inspect storage analysis, and continue only with explicit confirmation"
        },
        recovery_path: if target.dual_boot_required {
            "backup existing systems before replacing the disk"
        } else {
            "use recovery diagnostics or continue with explicit confirmation"
        },
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CpuFeatureSummary {
    pub sse: bool,
    pub sse2: bool,
    pub sse3: bool,
    pub sse4_1: bool,
    pub sse4_2: bool,
    pub avx: bool,
    pub avx2: bool,
    pub apic: bool,
    pub x2apic: bool,
    pub nx: bool,
    pub pat: bool,
    pub tsc: bool,
    pub invariant_tsc: bool,
    pub virtualization: bool,
}

impl CpuFeatureSummary {
    pub fn required_pass(self) -> bool {
        self.apic && self.nx && self.pat && self.tsc && self.sse && self.sse2
    }
}

pub fn cpu_features() -> CpuFeatureSummary {
    let (_, ebx7, _, _) = cpuid_count(7, 0);
    let (_, _, ecx1, edx1) = cpuid(1);
    let (_, _, _, edx_ext) = cpuid(0x8000_0001);
    let (_, _, _, edx_ext7) = cpuid(0x8000_0007);
    CpuFeatureSummary {
        sse: edx1 & (1 << 25) != 0,
        sse2: edx1 & (1 << 26) != 0,
        sse3: ecx1 & (1 << 0) != 0,
        sse4_1: ecx1 & (1 << 19) != 0,
        sse4_2: ecx1 & (1 << 20) != 0,
        avx: ecx1 & (1 << 28) != 0,
        avx2: ebx7 & (1 << 5) != 0,
        apic: edx1 & (1 << 9) != 0,
        x2apic: ecx1 & (1 << 21) != 0,
        nx: edx_ext & (1 << 20) != 0,
        pat: edx1 & (1 << 16) != 0,
        tsc: edx1 & (1 << 4) != 0,
        invariant_tsc: edx_ext7 & (1 << 8) != 0,
        virtualization: ecx1 & (1 << 5) != 0 || ecx1 & (1 << 2) != 0,
    }
}

fn discover_operating_systems(
    root_state: RootFilesystemState,
    partitions: &[StoragePartition],
    filesystems: &[FilesystemFinding],
) -> Vec<OperatingSystemFinding> {
    let mut out = Vec::new();
    if matches!(
        root_state,
        RootFilesystemState::RootMounted | RootFilesystemState::FilesystemValid
    ) {
        out.push(OperatingSystemFinding {
            kind: OperatingSystemKind::Saios,
            confidence: "medium",
            evidence: "valid ext4 root candidate detected by current rootfs classifier",
        });
    }
    if filesystems.iter().any(|fs| fs.kind == FilesystemKind::Ntfs) {
        out.push(OperatingSystemFinding {
            kind: OperatingSystemKind::Windows,
            confidence: "medium",
            evidence: "NTFS partition type detected; Windows Boot Manager file scan not implemented yet",
        });
    }
    if filesystems.iter().any(|fs| fs.kind == FilesystemKind::Ext4) && out.is_empty() {
        out.push(OperatingSystemFinding {
            kind: OperatingSystemKind::Linux,
            confidence: "medium",
            evidence: "ext4 filesystem detected outside mounted SAIOS evidence",
        });
    }
    if partitions
        .iter()
        .any(|partition| partition.type_code == 0xEF)
        && out.is_empty()
    {
        out.push(OperatingSystemFinding {
            kind: OperatingSystemKind::UnknownEfi,
            confidence: "low",
            evidence: "EFI system partition detected without readable boot-entry enumeration",
        });
    }
    out
}

fn operating_system_partition(
    kind: OperatingSystemKind,
    filesystems: &[Filesystem],
) -> Option<usize> {
    let filesystem_kind = match kind {
        OperatingSystemKind::Saios | OperatingSystemKind::Linux => FilesystemKind::Ext4,
        OperatingSystemKind::Windows => FilesystemKind::Ntfs,
        OperatingSystemKind::UnknownEfi => FilesystemKind::Fat32,
    };
    filesystems
        .iter()
        .find(|filesystem| filesystem.kind == filesystem_kind)
        .map(|filesystem| filesystem.partition_index)
}

fn risk_level_for_failure(reason: &str) -> RiskLevel {
    if reason.contains("no supported disk") || reason.contains("unknown disk state") {
        RiskLevel::Critical
    } else if reason.contains("existing operating system") {
        RiskLevel::High
    } else {
        RiskLevel::Medium
    }
}

fn risk_score(level: RiskLevel, factor_count: usize) -> u8 {
    let base: u8 = match level {
        RiskLevel::Low => 10,
        RiskLevel::Medium => 45,
        RiskLevel::High => 75,
        RiskLevel::Critical => 95,
    };
    let extra = factor_count.saturating_sub(1).min(5) as u8 * 3;
    base.saturating_add(extra).min(100)
}

fn simulated_action_detail(operation: &'static str) -> &'static str {
    match operation {
        "create EFI system partition" => "create a new 512 MiB recommended EFI system partition",
        "create ext4 root partition" => {
            "create an ext4 root partition with 20 GiB minimum and 64 GiB recommended policy"
        }
        "format FAT32/FAT ESP" => "format the planned ESP as FAT for UEFI boot files",
        "format ext4 root" => "format the planned root filesystem as ext4",
        "install boot files" => {
            "copy BOOTX64.EFI, grub.cfg, and saios.elf into the planned boot paths"
        }
        "verify rootfs and boot files" => {
            "verify root filesystem and boot file availability before completion"
        }
        _ => "planner operation would be executed only after approval",
    }
}

fn infer_filesystem(
    partition: &block::PartitionInfo,
    probes: &[block::Ext4ProbeInfo],
) -> FilesystemKind {
    if probes.iter().any(|probe| {
        probe.partition_index == Some(partition.index)
            && probe.read_ok
            && probe.actual_magic == probe.expected_magic
    }) {
        return FilesystemKind::Ext4;
    }
    match partition.type_code {
        0x07 => FilesystemKind::Ntfs,
        0x0B | 0x0C | 0x0E | 0xEF => FilesystemKind::Fat32,
        0x83 => FilesystemKind::Unknown,
        _ => FilesystemKind::Unknown,
    }
}

fn filesystem_confidence(filesystem: FilesystemKind) -> &'static str {
    match filesystem {
        FilesystemKind::Ext4 => "high",
        FilesystemKind::Fat32 => "medium",
        FilesystemKind::Ntfs => "medium",
        FilesystemKind::Unknown => "low",
    }
}

fn filesystem_evidence(filesystem: FilesystemKind) -> &'static str {
    match filesystem {
        FilesystemKind::Ext4 => "ext4 superblock magic matched",
        FilesystemKind::Fat32 => {
            "partition type indicates FAT/EFI; read-only FAT directory parsing pending"
        }
        FilesystemKind::Ntfs => {
            "partition type indicates NTFS; read-only NTFS metadata parsing pending"
        }
        FilesystemKind::Unknown => "filesystem signature not recognized",
    }
}

fn controller_vendor(controller: StorageController) -> &'static str {
    match controller {
        StorageController::Ahci => "AHCI-compatible controller",
        StorageController::VirtioBlk => "VirtIO",
        StorageController::Unknown => "unknown",
    }
}

fn controller_model(controller: StorageController) -> &'static str {
    match controller {
        StorageController::Ahci => "SATA block device",
        StorageController::VirtioBlk => "VirtIO block device",
        StorageController::Unknown => "generic block device",
    }
}

fn emit_event(
    event_type: crate::kds::KdsEventType,
    severity: crate::kds::KdsSeverity,
    payload: [u64; 4],
) -> u64 {
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        event_type,
        severity,
        payload,
    )
}

fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    cpuid_count(leaf, 0)
}

fn cpuid_count(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {0:e}, ebx",
            "pop rbx",
            out(reg) ebx,
            inlateout("eax") leaf => eax,
            inlateout("ecx") subleaf => ecx,
            out("edx") edx,
        );
    }
    (eax, ebx, ecx, edx)
}
