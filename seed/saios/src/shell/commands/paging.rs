use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;
use crate::vmm;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "paging",
        description: "Show paging and VMM status",
        handler: cmd_paging,
    }));
}

fn cmd_paging(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let stats = vmm::stats();
    console::println!(
        "paging: initialized={} cr3={:#x} mappings={} pages={} next_va={:#x}",
        stats.initialized,
        stats.cr3,
        stats.mappings,
        stats.mapped_pages,
        stats.next_kernel_virt,
    );
    Ok(())
}
