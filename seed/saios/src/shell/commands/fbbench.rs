//! Framebuffer throughput benchmark command.
//!
//! Measures full-screen clear bandwidth of the active framebuffer backend.

use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

const DEFAULT_PASSES: usize = 120;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "fbbench",
        description: "Benchmark framebuffer clear throughput",
        handler: cmd_fbbench,
    }));
}

fn cmd_fbbench(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    let passes = if let Some(arg) = args.first() {
        match arg.parse::<usize>() {
            Ok(v) if v > 0 => v,
            _ => {
                console::println!("fbbench: invalid pass count '{}'", arg);
                console::println!("usage: fbbench [passes]");
                return Ok(());
            }
        }
    } else {
        DEFAULT_PASSES
    };

    let Some(result) = console::benchmark_framebuffer_clears(passes) else {
        console::println!("fbbench: no framebuffer attached");
        return Ok(());
    };

    let elapsed = if result.elapsed_ms == 0 {
        1
    } else {
        result.elapsed_ms
    };

    console::println!("Framebuffer benchmark");
    console::println!("  passes       : {}", result.passes);
    console::println!("  bytes written: {}", result.bytes_written);
    console::println!("  elapsed      : {} ms ({} ticks)", elapsed, result.elapsed_ticks);
    console::println!("  throughput   : {} MiB/s", result.mib_per_sec);

    Ok(())
}
