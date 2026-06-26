//! First-boot setup wizard - removed, replaced by automatic config
//!
//! Previously this was an interactive wizard, now config is auto-created.

pub fn run() {
    if crate::shell::booted_from_hdd() {
        crate::serial_println!("[firstboot] installed-system firstboot mode accepted");
        crate::println!("SAIOS installed system first boot");
        crate::println!("Automatic configuration is active; continuing to login.");
    } else {
        crate::println!("firstboot: this mode is only valid when booted from the installed disk.");
        crate::println!("Use install or update from SAIOS install media.");
    }
}

pub fn save_config(_mirror: &str) {
    // Config is now managed by crate::config::manager
    // This function kept for compatibility but does nothing
}
