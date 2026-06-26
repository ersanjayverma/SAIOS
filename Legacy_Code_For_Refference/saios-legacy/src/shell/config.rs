//! SAIOS persistent configuration system.
//! Deprecated - kept for backwards compatibility with old /etc/saios.conf format.
//! All configuration now uses crate::config.

use alloc::string::{String, ToString};
use alloc::vec::{self, Vec};

pub fn load() -> crate::config::SaiosConfig {
    crate::configuration_contract::ConfigurationContract::get()
}

pub fn save(_config: &crate::config::SaiosConfig) {
    // Deprecated - config saves happen via config manager
}
