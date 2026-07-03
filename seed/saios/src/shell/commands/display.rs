//! Display / framebuffer introspection command.
//!
//! Provides `display` (alias `fb`) to report the geometry, pixel format, and
//! memory layout of the attached GOP framebuffer.  This is helpful when
//! verifying that the optimized 32-bit BGR/RGB flush paths are in use.

use alloc::boxed::Box;

use crate::console;
use crate::shell::command::{ShellResult, StaticCommand};
use crate::shell::registry::CommandRegistry;
use crate::shell::session::CommandContext;

pub fn register(registry: &mut CommandRegistry) {
    registry.register(Box::new(StaticCommand {
        name: "display",
        description: "Show framebuffer display info",
        handler: cmd_display,
    }));
    registry.register(Box::new(StaticCommand {
        name: "fb",
        description: "Alias for display",
        handler: cmd_display,
    }));
}

fn cmd_display(_ctx: &mut CommandContext, _args: &[&str]) -> ShellResult {
    let Some(props) = console::framebuffer_properties() else {
        console::println!("display: no framebuffer attached");
        return Ok(());
    };

    let pixel_count = props.width.saturating_mul(props.height);
    let row_bytes = props.stride.saturating_mul(props.bytes_per_pixel);
    let visible_bytes = pixel_count.saturating_mul(props.bytes_per_pixel);

    console::println!("Framebuffer display");
    console::println!("  resolution : {}x{} pixels", props.width, props.height);
    console::println!("  stride     : {} pixels", props.stride);
    console::println!("  row bytes  : {}", row_bytes);
    console::println!(
        "  pixel      : {} bytes, {:?}",
        props.bytes_per_pixel,
        props.pixel_format
    );
    console::println!(
        "  visible    : {} bytes ({} pixels)",
        visible_bytes,
        pixel_count
    );
    console::println!("  fb size    : {} bytes", props.framebuffer_size);

    if props.bytes_per_pixel == 4 {
        console::println!("  flush path : 32-bit row memcpy (fast)");
    } else {
        console::println!("  flush path : generic per-pixel writer");
    }

    Ok(())
}
