//! SAIOS Configuration Manager
//! Compatibility-facing configuration manager.
//! The canonical config lives at /system/config/system/saios.conf and is
//! exposed through /etc/saios.conf when possible.

use alloc::string::String;
use alloc::vec::Vec;

use crate::config::SaiosConfig;

static mut CONFIG: Option<SaiosConfig> = None;

pub fn init() {
    unsafe {
        CONFIG = Some(load_config());
    }
}

fn load_config() -> SaiosConfig {
    for path in [
        crate::config::CANONICAL_CONFIG_PATH,
        crate::config::COMPAT_CONFIG_PATH,
    ] {
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

fn save_config(config: &SaiosConfig) {
    let json = format_config(config);
    crate::write_file_pub(crate::config::CANONICAL_CONFIG_PATH, json.as_bytes());
    if !crate::ensure_symlink_pub(
        crate::config::COMPAT_CONFIG_PATH,
        crate::config::CANONICAL_CONFIG_PATH,
    ) {
        crate::write_file_pub(crate::config::COMPAT_CONFIG_PATH, json.as_bytes());
    }
}

fn format_config(config: &SaiosConfig) -> String {
    alloc::format!(
        "{{\n  \"version\": \"{}\",\n  \"ai\": {{\n    \"provider\": \"{}\",\n    \"host\": \"{}\",\n    \"model\": \"{}\"\n  }},\n  \"network\": {{\n    \"hostname\": \"{}\",\n    \"dns\": [\"{}\"]\n  }},\n  \"packages\": {{\n    \"mirror\": \"{}\"\n  }}\n}}",
        config.version,
        config.ai.provider,
        config.ai.host,
        config.ai.model,
        config.network.hostname,
        config.network.dns.join("\", \""),
        config.packages.mirror
    )
}

fn parse_config(content: &str) -> SaiosConfig {
    let mut config = SaiosConfig::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            let v = v.trim().trim_matches('"');
            match k {
                "ai_provider" => config.ai.provider = parse_provider(v),
                "ai_host" => config.ai.host = String::from(v),
                "ai_model" => config.ai.model = String::from(v),
                "hostname" => config.network.hostname = String::from(v),
                "dns" => config.network.dns = parse_dns(v),
                "apt_mirror" | "packages.mirror" => config.packages.mirror = String::from(v),
                _ => {}
            }
        }
    }
    config
}

fn parse_provider(s: &str) -> String {
    match s {
        "anthropic" => "Anthropic".to_string(),
        "openai" => "OpenAI".to_string(),
        "together" => "Together".to_string(),
        _ => "Ollama".to_string(),
    }
}

fn parse_dns(s: &str) -> Vec<String> {
    let s = s.trim().trim_matches('[').trim_matches(']');
    s.split(',')
        .map(|p| p.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}


pub fn get() -> SaiosConfig {
    unsafe { CONFIG.as_ref().unwrap().clone() }
}

pub fn set_provider(provider: &str) {
    unsafe {
        if let Some(cfg) = CONFIG.as_mut() {
            cfg.ai.provider = provider.to_string();
        }
    }
    save_current();
}

pub fn set_host(host: &str) {
    unsafe {
        if let Some(cfg) = CONFIG.as_mut() {
            cfg.ai.host = host.to_string();
        }
    }
    save_current();
}

pub fn set_model(model: &str) {
    unsafe {
        if let Some(cfg) = CONFIG.as_mut() {
            cfg.ai.model = model.to_string();
        }
    }
    save_current();
}

fn save_current() {
    if let Some(config) = unsafe { CONFIG.as_ref() } {
        save_config(config);
    }
}

pub fn reload() {
    unsafe { CONFIG = Some(load_config()); }
}

pub fn show() {
    if let Some(config) = unsafe { CONFIG.as_ref() } {
        println!("Configuration:");
        println!("  provider: {}", config.ai.provider);
        println!("  host: {}", config.ai.host);
        println!("  model: {}", config.ai.model);
        println!("  hostname: {}", config.network.hostname);
        println!("  dns: {:?}", config.network.dns);
        println!("  apt_mirror: {}", config.packages.mirror);
    }
}

pub fn save() -> bool {
    if let Some(config) = unsafe { CONFIG.as_ref() } {
        save_config(config);
        true
    } else {
        false
    }
}
