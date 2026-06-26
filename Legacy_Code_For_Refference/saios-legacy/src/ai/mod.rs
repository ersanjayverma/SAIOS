//! SAIOS AI subsystem — connects to AI inference providers.
//!
//! Free/open providers:
//!   - Ollama (local, runs on LAN — completely free, OSS)
//!
//! Cloud providers (require API keys configured at runtime):
//!   - Anthropic Claude
//!   - OpenAI GPT

pub mod anthropic;
pub mod memory;
pub mod ollama;
pub mod openai;
pub mod together;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    Ollama,
    Anthropic,
    OpenAI,
    Together,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Ollama => "ollama",
            Provider::Anthropic => "anthropic",
            Provider::OpenAI => "openai",
            Provider::Together => "together",
        }
    }
}

/// Runtime AI configuration — set via shell `ai config` command.
pub static CONFIG: Mutex<AiConfig> = Mutex::new(AiConfig {
    provider: Provider::Ollama,
    // Portable default: the NAT gateway/host where a local Ollama typically
    // listens.  A site-specific LAN server + model belongs in /etc/saios.conf
    // (loaded at first boot via firstboot::apply_ai_config), which overrides
    // these — never hard-code a LAN-only host/model in the static default.
    ollama_host: [10, 0, 2, 2],
    ollama_port: 11434,
    ollama_model: None,
    anthropic_key: None,
    openai_key: None,
    // Together AI (OpenAI-compatible).  Key is NEVER hardcoded (public repo) —
    // set it with `ai key together <key>` or `together_key=` in /etc/saios.conf.
    together_key: None,
    together_model: Some("openai/gpt-oss-120b"),
});

pub struct AiConfig {
    pub provider: Provider,
    pub ollama_host: [u8; 4],
    pub ollama_port: u16,
    pub ollama_model: Option<&'static str>,
    pub anthropic_key: Option<&'static str>,
    pub openai_key: Option<&'static str>,
    pub together_key: Option<&'static str>,
    pub together_model: Option<&'static str>,
}

#[derive(Debug)]
pub struct Message {
    pub role: &'static str, // "user" | "assistant" | "system"
    pub content: String,
}

/// Send a prompt to the configured AI provider and return the response text.
pub fn complete(prompt: &str) -> Option<String> {
    let (provider, model, host, port, key) = {
        let cfg = CONFIG.lock();
        (
            cfg.provider.clone(),
            cfg.ollama_model.unwrap_or("llama3"),
            cfg.ollama_host,
            cfg.ollama_port,
            cfg.anthropic_key,
        )
    };

    match provider {
        Provider::Ollama => ollama::complete(host, port, model, prompt),
        Provider::Anthropic => anthropic::complete(key?, prompt),
        Provider::OpenAI => {
            let cfg = CONFIG.lock();
            openai::complete(cfg.openai_key?, prompt)
        }
        Provider::Together => {
            let cfg = CONFIG.lock();
            together::complete(
                cfg.together_key?,
                cfg.together_model.unwrap_or("openai/gpt-oss-120b"),
                prompt,
            )
        }
    }
}

/// List all available providers and their status.
pub fn status() -> Vec<String> {
    let cfg = CONFIG.lock();
    alloc::vec![
        alloc::format!(
            "Ollama   {}  {}:{} model={}",
            if cfg.provider == Provider::Ollama {
                "[active]"
            } else {
                "       "
            },
            cfg.ollama_host.map(|b| alloc::format!("{}", b)).join("."),
            cfg.ollama_port,
            cfg.ollama_model.unwrap_or("llama3")
        ),
        alloc::format!(
            "Anthropic {}  key={}",
            if cfg.provider == Provider::Anthropic {
                "[active]"
            } else {
                "        "
            },
            if cfg.anthropic_key.is_some() {
                "set"
            } else {
                "not set"
            }
        ),
        alloc::format!(
            "OpenAI    {}  key={}",
            if cfg.provider == Provider::OpenAI {
                "[active]"
            } else {
                "        "
            },
            if cfg.openai_key.is_some() {
                "set"
            } else {
                "not set"
            }
        ),
        alloc::format!(
            "Together {}  key={} model={}",
            if cfg.provider == Provider::Together {
                "[active]"
            } else {
                "        "
            },
            if cfg.together_key.is_some() {
                "set"
            } else {
                "not set"
            },
            cfg.together_model.unwrap_or("openai/gpt-oss-120b")
        ),
    ]
}
