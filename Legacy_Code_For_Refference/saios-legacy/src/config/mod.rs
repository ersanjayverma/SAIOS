use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use crate::version;

const DEFAULT_OLLAMA_HOST: &str = "10.0.2.2";
const DEFAULT_OLLAMA_PORT: u16 = 11434;
const DEFAULT_OLLAMA_MODEL: &str = "llama3";
const DEFAULT_TOGETHER_MODEL: &str = "openai/gpt-oss-120b";

pub const CANONICAL_CONFIG_PATH: &str = "/system/config/system/saios.conf";
pub const COMPAT_CONFIG_PATH: &str = "/etc/saios.conf";
pub const LINUX_VIEW_CONFIG_PATH: &str = "/linux/etc/saios.conf";
pub const LINUX_COMPAT_CONFIG_PATH: &str = "/etc/saios.conf";

#[derive(Debug, Clone)]
pub struct SaiosConfig {
    pub version: String,
    pub ai: AiConfig,
    pub network: NetworkConfig,
    pub packages: PackageConfig,
}

impl Default for SaiosConfig {
    fn default() -> Self {
        Self {
            version: String::from(version::SAIOS_VERSION),
            ai: AiConfig::default(),
            network: NetworkConfig::default(),
            packages: PackageConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub provider: String,
    pub host: String,
    pub model: String,
    pub anthropic_key: Option<String>,
    pub openai_key: Option<String>,
    pub together_key: Option<String>,
    pub together_model: Option<String>,
    pub ollama_host: String,
    pub ollama_port: u16,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: "ollama".to_string(),
            host: "10.0.2.2:11434".to_string(),
            model: "llama3".to_string(),
            anthropic_key: None,
            openai_key: None,
            together_key: None,
            together_model: Some(DEFAULT_TOGETHER_MODEL.to_string()),
            ollama_host: DEFAULT_OLLAMA_HOST.to_string(),
            ollama_port: DEFAULT_OLLAMA_PORT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub hostname: String,
    pub dns: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            hostname: "saios".to_string(),
            dns: vec!["8.8.8.8".to_string(), "1.1.1.1".to_string()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct PackageConfig {
    pub mirror: String,
}

impl Default for PackageConfig {
    fn default() -> Self {
        Self {
            mirror: "deb.debian.org".to_string(),
        }
    }
}

// Manager functions - use Mutex for safe access
static CONFIG: Mutex<Option<SaiosConfig>> = Mutex::new(None);

#[derive(Clone, Copy)]
enum ReloadTarget {
    All,
    Ai,
    Network,
    Packages,
}

pub fn init() {
    let mut cfg = CONFIG.lock();
    *cfg = Some(load_config());
    drop(cfg);
    apply_to_ai();
    apply_to_system_files();
}

fn load_config() -> SaiosConfig {
    for path in [CANONICAL_CONFIG_PATH, COMPAT_CONFIG_PATH] {
        if let Ok(buf) = crate::vfs_contract::VfsContract::read_file(path)
            && !buf.is_empty()
        {
            return parse_config(&String::from_utf8_lossy(&buf));
        }
    }
    let defaults = SaiosConfig::default();
    save_config(&defaults);
    defaults
}

fn parse_config(content: &str) -> SaiosConfig {
    let mut config = SaiosConfig::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if matches!(line, "{" | "}" | "[" | "]") {
            continue;
        }

        let Some((raw_k, raw_v)) = split_config_entry(line) else {
            continue;
        };
        let key = sanitize_key(raw_k);
        let value = sanitize_value(raw_v);

        match key.as_str() {
            "ai_provider" | "ai.provider" | "provider" => {
                config.ai.provider = parse_provider(&value)
            }
            "ai_host" | "ai.host" | "host" => config.ai.host = value,
            "ai_model" | "ai.model" | "model" => config.ai.model = value,
            "anthropic_key" | "ai.anthropic_key" => config.ai.anthropic_key = non_empty(value),
            "openai_key" | "ai.openai_key" => config.ai.openai_key = non_empty(value),
            "together_key" | "ai.together_key" => config.ai.together_key = non_empty(value),
            "together_model" | "ai.together_model" => config.ai.together_model = non_empty(value),
            "ollama_host" | "ai.ollama_host" => config.ai.ollama_host = value,
            "ollama_port" | "ai.ollama_port" | "ai.port" => {
                if let Ok(port) = value.parse::<u16>() {
                    config.ai.ollama_port = port;
                }
            }
            "hostname" | "network.hostname" => config.network.hostname = value,
            "dns" | "network.dns" => config.network.dns = parse_dns(&value),
            "apt_mirror" | "packages.mirror" | "mirror" => config.packages.mirror = value,
            _ => {}
        }
    }
    normalize_config(&mut config);
    config
}

fn split_config_entry(line: &str) -> Option<(&str, &str)> {
    line.find('=')
        .map(|i| (&line[..i], &line[i + 1..]))
        .or_else(|| line.find(':').map(|i| (&line[..i], &line[i + 1..])))
}

fn sanitize_key(raw: &str) -> String {
    raw.trim()
        .trim_matches(',')
        .trim_matches('"')
        .trim_matches('{')
        .trim_matches('}')
        .trim()
        .to_string()
}

fn sanitize_value(raw: &str) -> String {
    raw.trim()
        .trim_matches(',')
        .trim_matches('"')
        .trim_matches('{')
        .trim_matches('}')
        .trim()
        .to_string()
}

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() { None } else { Some(value) }
}

fn normalize_config(config: &mut SaiosConfig) {
    if config.ai.provider.is_empty() {
        config.ai.provider = "ollama".to_string();
    }
    if config.ai.ollama_host.is_empty() {
        if let Some((host, _)) = split_host_port(&config.ai.host) {
            config.ai.ollama_host = host.to_string();
        } else {
            config.ai.ollama_host = DEFAULT_OLLAMA_HOST.to_string();
        }
    }
    if config.ai.ollama_port == 0 {
        config.ai.ollama_port = split_host_port(&config.ai.host)
            .and_then(|(_, port)| port.parse::<u16>().ok())
            .unwrap_or(DEFAULT_OLLAMA_PORT);
    }
    if config.ai.host.is_empty() {
        config.ai.host = alloc::format!("{}:{}", config.ai.ollama_host, config.ai.ollama_port);
    }
    if config.ai.model.is_empty() {
        config.ai.model = DEFAULT_OLLAMA_MODEL.to_string();
    }
    if config.ai.together_model.as_deref().unwrap_or("").is_empty() {
        config.ai.together_model = Some(DEFAULT_TOGETHER_MODEL.to_string());
    }
}

fn split_host_port(host: &str) -> Option<(&str, &str)> {
    host.rsplit_once(':')
}

fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = host.split('.');
    for octet in &mut out {
        *octet = parts.next()?.parse().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

fn leak_string(value: &str) -> &'static str {
    String::from(value).leak()
}

fn leak_opt(value: Option<&str>) -> Option<&'static str> {
    value.filter(|v| !v.is_empty()).map(leak_string)
}

fn provider_from_str(value: &str) -> crate::ai::Provider {
    match value {
        "anthropic" => crate::ai::Provider::Anthropic,
        "openai" => crate::ai::Provider::OpenAI,
        "together" => crate::ai::Provider::Together,
        _ => crate::ai::Provider::Ollama,
    }
}

fn parse_provider(s: &str) -> String {
    s.to_lowercase()
}

fn parse_dns(s: &str) -> Vec<String> {
    let mut dns = Vec::new();
    let s = s.trim().trim_matches('[').trim_matches(']');
    for p in s.split(',') {
        let p = p.trim().trim_matches('"');
        if !p.is_empty() {
            dns.push(p.to_string());
        }
    }
    dns
}

fn save_config(config: &SaiosConfig) {
    let text = format_config(config);
    crate::write_file_pub(CANONICAL_CONFIG_PATH, text.as_bytes());
    if !crate::ensure_symlink_pub(COMPAT_CONFIG_PATH, CANONICAL_CONFIG_PATH) {
        crate::write_file_pub(COMPAT_CONFIG_PATH, text.as_bytes());
    }
}

pub fn is_reload_path(path: &str) -> bool {
    matches!(
        path,
        CANONICAL_CONFIG_PATH | COMPAT_CONFIG_PATH | LINUX_VIEW_CONFIG_PATH
    )
}

fn format_config(config: &SaiosConfig) -> String {
    let anthropic_key = config.ai.anthropic_key.as_deref().unwrap_or("");
    let openai_key = config.ai.openai_key.as_deref().unwrap_or("");
    let together_key = config.ai.together_key.as_deref().unwrap_or("");
    let together_model = config
        .ai
        .together_model
        .as_deref()
        .unwrap_or(DEFAULT_TOGETHER_MODEL);
    let ollama_host = &config.ai.ollama_host;
    let ollama_port = config.ai.ollama_port;

    alloc::format!(
        "# SAIOS configuration\nversion={}\nai_provider={}\nai_host={}\nai_model={}\nanthropic_key={}\nopenai_key={}\ntogether_key={}\ntogether_model={}\nollama_host={}\nollama_port={}\nhostname={}\ndns={}\napt_mirror={}\n",
        config.version,
        config.ai.provider,
        config.ai.host,
        config.ai.model,
        anthropic_key,
        openai_key,
        together_key,
        together_model,
        ollama_host,
        ollama_port,
        config.network.hostname,
        config.network.dns.join(","),
        config.packages.mirror
    )
}

pub fn get() -> SaiosConfig {
    let cfg = CONFIG.lock();
    cfg.as_ref().unwrap().clone()
}

pub fn set_provider(provider: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.provider = provider.to_string();
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_host(host: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.host = host.to_string();
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_model(model: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.model = model.to_string();
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_anthropic_key(key: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.anthropic_key = Some(key.to_string());
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_openai_key(key: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.openai_key = Some(key.to_string());
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_together_key(key: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.together_key = Some(key.to_string());
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_together_model(model: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.together_model = Some(model.to_string());
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_ollama_model(model: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.model = model.to_string();
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_ollama_host(host: &str) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.ollama_host = host.to_string();
            c.ai.host = alloc::format!("{}:{}", host, c.ai.ollama_port);
        }
    }
    save_current();
    apply_to_ai();
}

pub fn set_ollama_port(port: u16) {
    {
        let mut cfg = CONFIG.lock();
        if let Some(c) = cfg.as_mut() {
            c.ai.ollama_port = port;
            c.ai.host = alloc::format!("{}:{}", c.ai.ollama_host, port);
        }
    }
    save_current();
    apply_to_ai();
}

fn save_current() {
    let cfg = CONFIG.lock();
    if let Some(c) = cfg.as_ref() {
        save_config(c);
        apply_config_projection(c);
    }
}

pub fn reload() {
    reload_target(ReloadTarget::All);
}

pub fn reload_ai() {
    reload_target(ReloadTarget::Ai);
}

pub fn reload_network() {
    reload_target(ReloadTarget::Network);
}

pub fn reload_packages() {
    reload_target(ReloadTarget::Packages);
}

fn reload_target(target: ReloadTarget) {
    let mut cfg = CONFIG.lock();
    *cfg = Some(load_config());
    drop(cfg);

    match target {
        ReloadTarget::All => {
            apply_to_ai();
            apply_to_system_files();
        }
        ReloadTarget::Ai => apply_to_ai(),
        ReloadTarget::Network => apply_network_projection(),
        ReloadTarget::Packages => apply_package_projection(),
    }
}

pub fn show() {
    let cfg = CONFIG.lock();
    if let Some(c) = cfg.as_ref() {
        crate::println!("Configuration:");
        crate::println!("  provider: {}", c.ai.provider);
        crate::println!("  host: {}", c.ai.host);
        crate::println!("  model: {}", c.ai.model);
        crate::println!("  hostname: {}", c.network.hostname);
        crate::println!("  dns: {:?}", c.network.dns);
        crate::println!("  apt_mirror: {}", c.packages.mirror);
    }
}

pub fn save() -> bool {
    let cfg = CONFIG.lock();
    if let Some(c) = cfg.as_ref() {
        save_config(c);
        apply_config_projection(c);
        true
    } else {
        false
    }
}

fn apply_to_system_files() {
    apply_network_projection();
    apply_package_projection();
}

fn apply_network_projection() {
    let cfg = get();
    let hostname = alloc::format!("{}\n", cfg.network.hostname);
    crate::write_file_pub("/etc/hostname", hostname.as_bytes());

    let mut resolv = String::new();
    for server in &cfg.network.dns {
        if !server.is_empty() {
            resolv.push_str("nameserver ");
            resolv.push_str(server);
            resolv.push('\n');
        }
    }
    if resolv.is_empty() {
        resolv.push_str("nameserver 8.8.8.8\n");
        resolv.push_str("nameserver 1.1.1.1\n");
    }
    crate::write_file_pub("/etc/resolv.conf", resolv.as_bytes());
}

fn apply_package_projection() {
    let cfg = get();
    let sources = alloc::format!(
        "deb http://{}/debian bookworm main contrib non-free\n\
deb http://security.debian.org/debian-security bookworm-security main\n\
deb http://{}/debian bookworm-updates main\n",
        cfg.packages.mirror,
        cfg.packages.mirror,
    );
    crate::write_file_pub("/etc/apt/sources.list", sources.as_bytes());
}

fn apply_config_projection(config: &SaiosConfig) {
    let hostname = alloc::format!("{}\n", config.network.hostname);
    crate::write_file_pub("/etc/hostname", hostname.as_bytes());

    let mut resolv = String::new();
    for server in &config.network.dns {
        if !server.is_empty() {
            resolv.push_str("nameserver ");
            resolv.push_str(server);
            resolv.push('\n');
        }
    }
    if resolv.is_empty() {
        resolv.push_str("nameserver 8.8.8.8\n");
        resolv.push_str("nameserver 1.1.1.1\n");
    }
    crate::write_file_pub("/etc/resolv.conf", resolv.as_bytes());

    let sources = alloc::format!(
        "deb http://{}/debian bookworm main contrib non-free\n\
deb http://security.debian.org/debian-security bookworm-security main\n\
deb http://{}/debian bookworm-updates main\n",
        config.packages.mirror,
        config.packages.mirror,
    );
    crate::write_file_pub("/etc/apt/sources.list", sources.as_bytes());
}

/// Sync configuration from ai::CONFIG to the persistent config manager.
/// This copies the current runtime config to the config module for persistence.
pub fn sync_from_ai() {
    let (
        provider_str,
        ollama_host,
        ollama_port,
        active_model,
        anthropic_key,
        openai_key,
        together_key,
        together_model,
    ) = {
        let ai_cfg = crate::ai::CONFIG.lock();

        let provider_str = ai_cfg.provider.as_str().to_string();

        let ollama_host = alloc::format!(
            "{}.{}.{}.{}",
            ai_cfg.ollama_host[0],
            ai_cfg.ollama_host[1],
            ai_cfg.ollama_host[2],
            ai_cfg.ollama_host[3]
        );

        let active_model = match ai_cfg.provider {
            crate::ai::Provider::Together => {
                ai_cfg.together_model.unwrap_or(DEFAULT_TOGETHER_MODEL)
            }
            _ => ai_cfg.ollama_model.unwrap_or(DEFAULT_OLLAMA_MODEL),
        }
        .to_string();

        (
            provider_str,
            ollama_host,
            ai_cfg.ollama_port,
            active_model,
            ai_cfg.anthropic_key.map(str::to_string),
            ai_cfg.openai_key.map(str::to_string),
            ai_cfg.together_key.map(str::to_string),
            ai_cfg.together_model.map(str::to_string),
        )
    }; // AI lock released here

    let host = alloc::format!("{}:{}", ollama_host, ollama_port);

    {
        let mut cfg = CONFIG.lock();

        if let Some(c) = cfg.as_mut() {
            c.ai.provider = provider_str;
            c.ai.host = host;
            c.ai.model = active_model;
            c.ai.anthropic_key = anthropic_key;
            c.ai.openai_key = openai_key;
            c.ai.together_key = together_key;
            c.ai.together_model = together_model;
            c.ai.ollama_host = ollama_host;
            c.ai.ollama_port = ollama_port;
        }
    } // CONFIG lock released here

    save_current();
}
pub fn apply_to_ai() {
    let cfg = get();
    let mut ai_cfg = crate::ai::CONFIG.lock();

    ai_cfg.provider = provider_from_str(&cfg.ai.provider);
    ai_cfg.ollama_host = parse_ipv4(&cfg.ai.ollama_host)
        .or_else(|| split_host_port(&cfg.ai.host).and_then(|(host, _)| parse_ipv4(host)))
        .unwrap_or([10, 0, 2, 2]);
    ai_cfg.ollama_port = if cfg.ai.ollama_port != 0 {
        cfg.ai.ollama_port
    } else {
        split_host_port(&cfg.ai.host)
            .and_then(|(_, port)| port.parse::<u16>().ok())
            .unwrap_or(DEFAULT_OLLAMA_PORT)
    };
    ai_cfg.ollama_model = Some(leak_string(&cfg.ai.model));
    ai_cfg.anthropic_key = leak_opt(cfg.ai.anthropic_key.as_deref());
    ai_cfg.openai_key = leak_opt(cfg.ai.openai_key.as_deref());
    ai_cfg.together_key = leak_opt(cfg.ai.together_key.as_deref());
    ai_cfg.together_model = leak_opt(cfg.ai.together_model.as_deref())
        .or_else(|| Some(leak_string(DEFAULT_TOGETHER_MODEL)));
}
