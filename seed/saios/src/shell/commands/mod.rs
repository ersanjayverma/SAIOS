pub mod clear;
pub mod health;
pub mod help;
pub mod inspect;
pub mod objects;
pub mod reboot;
pub mod shutdown;
pub mod version;

use super::registry::CommandRegistry;

pub fn register(registry: &mut CommandRegistry) {
    clear::register(registry);
    health::register(registry);
    help::register(registry);
    inspect::register(registry);
    objects::register(registry);
    reboot::register(registry);
    shutdown::register(registry);
    version::register(registry);
}
