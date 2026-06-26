//! Configuration contract owner surface.

use crate::config::SaiosConfig;
use crate::observability_contract::{
    ContractId, EventRecord, ObservabilityContract, ObservableEvent, ObservationOutcome,
    ObservationTag, ResourceClass,
};

pub struct ConfigurationContract;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardcodedAssumptionClass {
    ConfigurationDefault = 1,
    AbiProtocolConstant = 2,
    PlatformProbeLimit = 3,
    CompatibilityStub = 4,
    TestSeed = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HardcodedAssumption {
    pub owner: ContractId,
    pub file: &'static str,
    pub symbol: &'static str,
    pub class: HardcodedAssumptionClass,
    pub value_hint: &'static str,
    pub rationale: &'static str,
}

const HARDCODED_ASSUMPTIONS: &[HardcodedAssumption] = &[
    HardcodedAssumption {
        owner: ContractId::Network,
        file: "src/NetworkContract.rs",
        symbol: "DEFAULT_NETWORK_IPV4",
        class: HardcodedAssumptionClass::ConfigurationDefault,
        value_hint: "10.0.2.15",
        rationale: "QEMU NAT fallback address until boot/network configuration overrides identity",
    },
    HardcodedAssumption {
        owner: ContractId::Network,
        file: "src/net/tcp.rs",
        symbol: "SYN/ACK/FIN/RST/PSH",
        class: HardcodedAssumptionClass::AbiProtocolConstant,
        value_hint: "TCP flag bits",
        rationale: "wire-format constants defined by TCP, not runtime policy",
    },
    HardcodedAssumption {
        owner: ContractId::Syscall,
        file: "src/process/exec.rs",
        symbol: "AT_* auxv constants",
        class: HardcodedAssumptionClass::AbiProtocolConstant,
        value_hint: "System V AMD64 auxv tags",
        rationale: "ELF/Linux ABI values required for userspace compatibility",
    },
    HardcodedAssumption {
        owner: ContractId::Syscall,
        file: "src/process/exec.rs",
        symbol: "WINDOWS_STUB_USER_STACK",
        class: HardcodedAssumptionClass::CompatibilityStub,
        value_hint: "0x80000000",
        rationale: "placeholder Windows process stack until roadmap phase enables full PE userspace",
    },
    HardcodedAssumption {
        owner: ContractId::Syscall,
        file: "src/windows/pe_loader.rs",
        symbol: "PE_DEFAULT_BASE_ADDR/PE_STUB_ENTRY_POINT",
        class: HardcodedAssumptionClass::CompatibilityStub,
        value_hint: "0x400000/0x401000",
        rationale: "compatibility stub addresses, not completed Windows loader policy",
    },
    HardcodedAssumption {
        owner: ContractId::Memory,
        file: "src/memory/mod.rs",
        symbol: "MIN_HEAP/MAX_HEAP/IDENTITY_MAP_LIMIT",
        class: HardcodedAssumptionClass::PlatformProbeLimit,
        value_hint: "16MiB/256MiB/128GiB",
        rationale: "boot identity-map and heap sizing policy tied to detected RAM",
    },
    HardcodedAssumption {
        owner: ContractId::Syscall,
        file: "src/process/exec.rs",
        symbol: "AT_RANDOM fallback bytes",
        class: HardcodedAssumptionClass::TestSeed,
        value_hint: "fixed 16-byte pattern",
        rationale: "deterministic placeholder until entropy-backed process randomization is owned",
    },
];

impl ConfigurationContract {
    pub const DEFAULT_NETWORK_GATEWAY: [u8; 4] = [10, 0, 2, 2];
    pub const DEFAULT_NETWORK_NETMASK: [u8; 4] = [255, 255, 255, 0];
    pub const DEFAULT_NETWORK_DNS: [u8; 4] = [8, 8, 8, 8];
    pub const DEFAULT_NETWORK_IPV4: [u8; 4] = [10, 0, 2, 15];
    pub const DEFAULT_SOCKET_SEND_BUFFER_BYTES: usize = 64 * 1024;
    pub const DEFAULT_SOCKET_RECV_BUFFER_BYTES: usize = 64 * 1024;
    pub const SOCKET_BUFFER_PRESSURE_PERCENT: u8 = 80;

    pub fn hardcoded_assumptions() -> &'static [HardcodedAssumption] {
        HARDCODED_ASSUMPTIONS
    }

    pub fn hardcoded_assumption_count() -> usize {
        HARDCODED_ASSUMPTIONS.len()
    }

    pub fn get() -> SaiosConfig {
        let cfg = crate::config::get();
        Self::emit(
            "configuration.snapshot",
            ObservationOutcome::Success,
            [
                cfg.version.len() as u64,
                cfg.network.hostname.len() as u64,
                cfg.network.dns.len() as u64,
                cfg.packages.mirror.len() as u64,
            ],
        );
        cfg
    }

    pub fn show() {
        Self::emit(
            "configuration.show",
            ObservationOutcome::Success,
            [0, 0, 0, 0],
        );
        crate::config::show();
    }

    pub fn save() -> bool {
        let ok = crate::config::save();
        Self::emit(
            "configuration.save",
            if ok {
                ObservationOutcome::Success
            } else {
                ObservationOutcome::Failed
            },
            [ok as u64, 0, 0, 0],
        );
        ok
    }

    pub fn reload() {
        crate::config::reload();
        Self::emit(
            "configuration.reload",
            ObservationOutcome::Success,
            [0, 0, 0, 0],
        );
    }
    pub fn reload_ai() {
        crate::config::reload_ai();
        Self::emit(
            "configuration.reload.ai",
            ObservationOutcome::Success,
            [0, 0, 0, 0],
        );
    }
    pub fn reload_network() {
        crate::config::reload_network();
        Self::emit(
            "configuration.reload.network",
            ObservationOutcome::Success,
            [0, 0, 0, 0],
        );
    }
    pub fn reload_packages() {
        crate::config::reload_packages();
        Self::emit(
            "configuration.reload.packages",
            ObservationOutcome::Success,
            [0, 0, 0, 0],
        );
    }

    pub fn set_provider(provider: &str) {
        crate::config::set_provider(provider);
        Self::emit_set("configuration.set.provider", provider.len() as u64);
    }
    pub fn set_anthropic_key(key: &str) {
        crate::config::set_anthropic_key(key);
        Self::emit_secret("configuration.set.secret.anthropic", key.len() as u64);
    }
    pub fn set_openai_key(key: &str) {
        crate::config::set_openai_key(key);
        Self::emit_secret("configuration.set.secret.openai", key.len() as u64);
    }
    pub fn set_together_key(key: &str) {
        crate::config::set_together_key(key);
        Self::emit_secret("configuration.set.secret.together", key.len() as u64);
    }
    pub fn set_together_model(model: &str) {
        crate::config::set_together_model(model);
        Self::emit_set("configuration.set.together_model", model.len() as u64);
    }
    pub fn set_ollama_model(model: &str) {
        crate::config::set_ollama_model(model);
        Self::emit_set("configuration.set.ollama_model", model.len() as u64);
    }
    pub fn set_ollama_host(host: &str) {
        crate::config::set_ollama_host(host);
        Self::emit_set("configuration.set.ollama_host", host.len() as u64);
    }
    pub fn set_ollama_port(port: u16) {
        crate::config::set_ollama_port(port);
        Self::emit_set("configuration.set.ollama_port", port as u64);
    }

    fn emit_set(reason: &'static str, value: u64) {
        Self::emit(
            reason,
            ObservationOutcome::Success,
            [
                value,
                crate::process::current_pid().unwrap_or(0) as u64,
                0,
                0,
            ],
        );
    }

    fn emit_secret(reason: &'static str, len: u64) {
        Self::emit(reason, ObservationOutcome::Success, [len, 0, 0, 0]);
    }

    fn emit(reason: &'static str, outcome: ObservationOutcome, evidence: [u64; 4]) {
        ObservabilityContract::emit(EventRecord {
            event: ObservableEvent::Transition,
            contract: ContractId::Configuration,
            tag: ObservationTag::Transition,
            reason,
            outcome,
            resource: ResourceClass::Configuration,
            owner: ObservabilityContract::current_pid_owner(),
            cpu: Some(crate::process::table::cpu_idx()),
            pid: crate::process::current_pid(),
            correlation_id: ObservabilityContract::current_correlation_id(),
            evidence,
        });
    }
}
