//! Compatibility roadmap authority.
//!
//! Compatibility features must advance in the documented phase order. Future
//! layers may exist as scaffolding, but executable paths must be gated here.

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompatibilityPhase {
    NativeSaiosBaseline = 1,
    ElfAndPosixSubset = 2,
    LinuxSyscallCompatibility = 3,
    SairuAndContainers = 4,
    FullLinuxUserspace = 5,
    WindowsAndAiIntegration = 6,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityLayer {
    NativeSaios = 1,
    PosixSubset = 2,
    LinuxSyscallAbi = 3,
    Containers = 4,
    FullLinuxUserspace = 5,
    WindowsCompatibility = 6,
    AiModelIntegration = 7,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityGateReason {
    Allowed = 0,
    FuturePhase = 1,
    PlaceholderSurface = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityGate {
    pub layer: CompatibilityLayer,
    pub active_phase: CompatibilityPhase,
    pub required_phase: CompatibilityPhase,
    pub reason: CompatibilityGateReason,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderSurfaceKind {
    PackageExtraction = 1,
    CompressionCodec = 2,
    WindowsLoader = 3,
    WirelessDriver = 4,
    UefiBootScaffold = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaceholderSurface {
    pub id: &'static str,
    pub kind: PlaceholderSurfaceKind,
    pub layer: CompatibilityLayer,
    pub required_phase: CompatibilityPhase,
    pub production_facing: bool,
    pub acceptance: &'static str,
}

const PLACEHOLDER_SURFACES: &[PlaceholderSurface] = &[
    PlaceholderSurface {
        id: "bash.deb.xz.extractor",
        kind: PlaceholderSurfaceKind::PackageExtraction,
        layer: CompatibilityLayer::FullLinuxUserspace,
        required_phase: CompatibilityPhase::FullLinuxUserspace,
        production_facing: true,
        acceptance: "extract data.tar.xz/control.tar.xz and install /bin/bash without manual host steps",
    },
    PlaceholderSurface {
        id: "compress.zstd",
        kind: PlaceholderSurfaceKind::CompressionCodec,
        layer: CompatibilityLayer::FullLinuxUserspace,
        required_phase: CompatibilityPhase::FullLinuxUserspace,
        production_facing: true,
        acceptance: "decode zstd package payloads used by modern package archives",
    },
    PlaceholderSurface {
        id: "windows.pe.load",
        kind: PlaceholderSurfaceKind::WindowsLoader,
        layer: CompatibilityLayer::WindowsCompatibility,
        required_phase: CompatibilityPhase::WindowsAndAiIntegration,
        production_facing: true,
        acceptance: "map PE sections, initialise PEB/TEB, resolve imports, and transfer to a valid entry",
    },
    PlaceholderSurface {
        id: "iwlwifi.microcode.connect",
        kind: PlaceholderSurfaceKind::WirelessDriver,
        layer: CompatibilityLayer::FullLinuxUserspace,
        required_phase: CompatibilityPhase::FullLinuxUserspace,
        production_facing: true,
        acceptance: "parse TLV firmware, DMA-load microcode, scan, associate, and exchange frames",
    },
    PlaceholderSurface {
        id: "uefi.open-protocol.stub",
        kind: PlaceholderSurfaceKind::UefiBootScaffold,
        layer: CompatibilityLayer::NativeSaios,
        required_phase: CompatibilityPhase::NativeSaiosBaseline,
        production_facing: false,
        acceptance: "replace narrow boot-service lookup shim with complete OpenProtocol handling",
    },
];

pub struct CompatibilityContract;

impl CompatibilityContract {
    pub const ACTIVE_PHASE: CompatibilityPhase = CompatibilityPhase::LinuxSyscallCompatibility;

    pub fn active_phase_number() -> u8 {
        Self::ACTIVE_PHASE as u8
    }

    pub fn phase_label(phase: CompatibilityPhase) -> &'static str {
        match phase {
            CompatibilityPhase::NativeSaiosBaseline => "Native SAIOS baseline",
            CompatibilityPhase::ElfAndPosixSubset => "ELF64 and POSIX subset",
            CompatibilityPhase::LinuxSyscallCompatibility => "Linux syscall ABI compatibility",
            CompatibilityPhase::SairuAndContainers => "SAIRU phase one and containers",
            CompatibilityPhase::FullLinuxUserspace => "full Linux userspace on real hardware",
            CompatibilityPhase::WindowsAndAiIntegration => "WCL and AI model integration",
        }
    }

    pub fn active_phase_label() -> &'static str {
        Self::phase_label(Self::ACTIVE_PHASE)
    }

    pub fn active_status_summary() -> &'static str {
        "Compatibility roadmap Phase 3 active: Linux syscall ABI compatibility in progress"
    }

    pub fn placeholder_surfaces() -> &'static [PlaceholderSurface] {
        PLACEHOLDER_SURFACES
    }

    pub fn placeholder_surface(id: &'static str) -> Option<&'static PlaceholderSurface> {
        PLACEHOLDER_SURFACES.iter().find(|surface| surface.id == id)
    }

    pub fn required_phase(layer: CompatibilityLayer) -> CompatibilityPhase {
        match layer {
            CompatibilityLayer::NativeSaios => CompatibilityPhase::NativeSaiosBaseline,
            CompatibilityLayer::PosixSubset => CompatibilityPhase::ElfAndPosixSubset,
            CompatibilityLayer::LinuxSyscallAbi => CompatibilityPhase::LinuxSyscallCompatibility,
            CompatibilityLayer::Containers => CompatibilityPhase::SairuAndContainers,
            CompatibilityLayer::FullLinuxUserspace => CompatibilityPhase::FullLinuxUserspace,
            CompatibilityLayer::WindowsCompatibility | CompatibilityLayer::AiModelIntegration => {
                CompatibilityPhase::WindowsAndAiIntegration
            }
        }
    }

    pub fn require_layer(layer: CompatibilityLayer) -> Result<(), CompatibilityGate> {
        let required_phase = Self::required_phase(layer);
        if Self::ACTIVE_PHASE >= required_phase {
            return Ok(());
        }
        let gate = CompatibilityGate {
            layer,
            active_phase: Self::ACTIVE_PHASE,
            required_phase,
            reason: CompatibilityGateReason::FuturePhase,
        };
        Self::emit_gate(gate);
        Err(gate)
    }

    pub fn require_placeholder_available(id: &'static str) -> Result<(), CompatibilityGate> {
        if let Some(surface) = Self::placeholder_surface(id) {
            if !surface.production_facing || Self::ACTIVE_PHASE >= surface.required_phase {
                return Ok(());
            }
            let gate = CompatibilityGate {
                layer: surface.layer,
                active_phase: Self::ACTIVE_PHASE,
                required_phase: surface.required_phase,
                reason: CompatibilityGateReason::PlaceholderSurface,
            };
            Self::emit_gate(gate);
            return Err(gate);
        }
        Ok(())
    }

    pub fn emit_gate(gate: CompatibilityGate) {
        crate::observability_contract::ObservabilityContract::kds_event(
            crate::kds::KdsSubsystem::Syscall,
            crate::kds::KdsEventType::CompatibilityFailure,
            crate::kds::KdsSeverity::Info,
            [
                gate.layer as u64,
                gate.active_phase as u64,
                gate.required_phase as u64,
                gate.reason as u64,
            ],
        );
    }
}
