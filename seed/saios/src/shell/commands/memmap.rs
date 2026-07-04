use alloc::boxed::Box;

use crate::console;
use crate::pmm;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "memmap",
        description: "Show physical memory map statistics",
        handler: cmd_memmap,
    }));
}

fn cmd_memmap(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    console::println!(
        "memmap: total_pages={} free_pages={} used_pages={} available={} used={}",
        pmm::total_pages(),
        pmm::free_pages(),
        pmm::used_pages(),
        pmm::available_bytes(),
        pmm::used_bytes(),
    );
    Ok(())
}
