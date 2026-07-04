use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;
use hal::arch::x86_64::msr;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "pat",
        description: "Show PAT configuration state",
        handler: cmd_pat,
    }));
}

fn cmd_pat(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    const IA32_PAT: u32 = 0x277;
    let pat_value = msr::rdmsr(IA32_PAT);
    console::println!("pat: IA32_PAT={:#x}", pat_value);
    Ok(())
}
