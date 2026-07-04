pub mod acpi;
pub mod clear;
pub mod console;
pub mod display;
pub mod fbbench;
pub mod health;
pub mod help;
pub mod inspect;
pub mod memmap;
pub mod objects;
pub mod paging;
pub mod pat;
pub mod reboot;
pub mod shutdown;
pub mod version;

use super::registry::CommandRegistry;

pub fn register(registry: &mut CommandRegistry) {
    acpi::register(registry);
    clear::register(registry);
    console::register(registry);
    display::register(registry);
    fbbench::register(registry);
    health::register(registry);
    help::register(registry);
    inspect::register(registry);
    memmap::register(registry);
    objects::register(registry);
    paging::register(registry);
    pat::register(registry);
    reboot::register(registry);
    shutdown::register(registry);
    version::register(registry);
}
