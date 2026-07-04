//! Console terminal introspection command.
//!
//! Provides `console` to report the current text grid size, cursor position,
//! scrollback state, and whether the framebuffer renderer is active.  This is
//! useful for diagnosing terminal behavior and verifying that the fast
//! scroll/partial-update paths are in use.

use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "console",
        description: "Show console terminal state",
        handler: cmd_console,
    }));
}

fn cmd_console(_ctx: &mut CommandContext, args: &[&str]) -> ShellResult {
    if let Some(arg) = args.first() {
        match *arg {
            "clear" | "cls" => {
                console::clear();
                return Ok(());
            }
            "reset" => {
                console::clear();
                return Ok(());
            }
            "help" => {
                print_help();
                return Ok(());
            }
            _ => {
                console::println!("console: unknown subcommand '{}'", arg);
                print_help();
                return Ok(());
            }
        }
    }

    let (cols, rows) = console::dimensions();
    let (cx, cy) = console::cursor_position();
    let scrollback = console::scrollback_lines();
    let offset = console::scrollback_offset();
    let fb = console::framebuffer_attached();

    console::println!("Console terminal");
    console::println!("  grid     : {}x{} cells", cols, rows);
    console::println!("  cursor   : ({}, {})", cx, cy);
    console::println!("  scrollback: {} lines", scrollback);
    console::println!("  view offset: {} lines", offset);
    console::println!("  framebuffer: {}", if fb { "attached" } else { "none" });

    if let Some(props) = console::framebuffer_properties() {
        console::println!(
            "  resolution : {}x{} (stride {})",
            props.width,
            props.height,
            props.stride
        );
        console::println!(
            "  pixel      : {} bytes, {:?}",
            props.bytes_per_pixel,
            props.pixel_format
        );
        console::println!("  fb size    : {} bytes", props.framebuffer_size);
    }

    Ok(())
}

fn print_help() {
    console::println!("console           - Show terminal state");
    console::println!("console clear     - Clear screen");
    console::println!("console reset     - Clear screen");
    console::println!("console help      - Show this help");
}
