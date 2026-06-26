//! SAIOS Graphics Subsystem (Phase 7).
//!
//! Provides a 2-D graphics pipeline on top of the VESA/GOP linear
//! framebuffer set up by GRUB (Multiboot2 framebuffer tag) or the UEFI stub:
//!
//!   font / font_data  — 8x16 bitmap font + glyph rasteriser
//!   console           — scrolling text console rendered to the framebuffer
//!   window            — simple windows + a software compositor
//!
//! All drawing goes through `driver::vesa`, which owns the framebuffer
//! descriptor (address, width, height, pitch, bpp).
//!
//! # Milestone
//! `gfx` shell command → switches to graphics mode, draws a desktop with a
//! title bar, windows, and a mouse cursor.

pub mod console;
pub mod font;
pub mod font_data;
pub mod font_latin1;
pub mod ui;
pub mod window; // Phase 9: native widget toolkit

// -- Colour palette (ARGB 0x00RRGGBB) ---------------------------------------

pub const BLACK: u32 = 0x00_000000;
pub const WHITE: u32 = 0x00_FFFFFF;
pub const RED: u32 = 0x00_E04040;
pub const GREEN: u32 = 0x00_40E040;
pub const BLUE: u32 = 0x00_4060E0;
pub const CYAN: u32 = 0x00_40E0E0;
pub const YELLOW: u32 = 0x00_E0E040;
pub const GRAY: u32 = 0x00_808080;
pub const DARK_GRAY: u32 = 0x00_303030;
pub const LIGHT_GRAY: u32 = 0x00_C0C0C0;
pub const DESKTOP_BG: u32 = 0x00_1E2A3A; // dark blue-gray desktop
pub const SAIOS_GREEN: u32 = 0x00_30D050; // SAIOS accent colour

// -- Framebuffer access (thin wrappers over driver::vesa) -------------------

/// True if a usable framebuffer is available.
pub fn available() -> bool {
    crate::driver::vesa::active()
}

/// Framebuffer dimensions in pixels, or (0, 0) if none.
pub fn dimensions() -> (usize, usize) {
    let fb = crate::driver::vesa::FB.lock();
    (fb.width as usize, fb.height as usize)
}

/// Set a single pixel (used by the font rasteriser).
#[inline]
pub fn vesa_put(x: usize, y: usize, colour: u32) {
    crate::driver::vesa::put_pixel(x as u32, y as u32, colour);
}

/// Fill a rectangle with a solid colour.
pub fn fill_rect(x: usize, y: usize, w: usize, h: usize, colour: u32) {
    crate::driver::vesa::fill_rect(x as u32, y as u32, w as u32, h as u32, colour);
}

/// Draw a 1-pixel rectangle outline.
pub fn draw_rect(x: usize, y: usize, w: usize, h: usize, colour: u32) {
    if w == 0 || h == 0 {
        return;
    }
    fill_rect(x, y, w, 1, colour); // top
    fill_rect(x, y + h - 1, w, 1, colour); // bottom
    fill_rect(x, y, 1, h, colour); // left
    fill_rect(x + w - 1, y, 1, h, colour); // right
}

/// Clear the whole screen to a colour.
pub fn clear(colour: u32) {
    crate::driver::vesa::clear(colour);
}

// -- Initialisation ----------------------------------------------------------

/// Initialise the graphics subsystem.  Called after the framebuffer driver.
pub fn init() {
    if available() {
        let (w, h) = dimensions();
        crate::serial_println!("[gfx] graphics subsystem ready ({}x{})", w, h);
    } else {
        crate::serial_println!("[gfx] no framebuffer — text mode only");
        crate::serial_println!("[gfx] add 'set gfxpayload=1024x768x32' to grub.cfg for graphics");
    }
}

// -- Desktop demo -------------------------------------------------------------

/// Draw a sample SAIOS desktop: wallpaper, top bar, a couple of windows,
/// and a mouse cursor.  Invoked by the `gfx` shell command.
pub fn draw_desktop() {
    if !available() {
        crate::println!("gfx: no framebuffer available (boot in graphics mode)");
        return;
    }
    let (w, h) = dimensions();

    // Wallpaper
    clear(DESKTOP_BG);

    // Top bar
    fill_rect(0, 0, w, 28, DARK_GRAY);
    font::draw_string(8, 6, "SAIOS Desktop", SAIOS_GREEN, DARK_GRAY);
    let clock = "12:00";
    font::draw_string(w - 8 * clock.len() - 8, 6, clock, WHITE, DARK_GRAY);

    // A couple of demo windows
    let win1 = window::Window::new(60, 80, 320, 200, "Terminal");
    win1.draw();
    win1.draw_text(2, "saios:/$ help");
    win1.draw_text(3, "saios:/$ gfx");
    win1.draw_text(4, "Graphics mode active.");

    let win2 = window::Window::new(420, 160, 280, 180, "AI Assistant");
    win2.draw();
    win2.draw_text(2, "Ask me anything:");
    win2.draw_text(3, "> _");

    // Taskbar at the bottom
    fill_rect(0, h - 32, w, 32, DARK_GRAY);
    font::draw_string(8, h - 26, "[Terminal] [Files] [AI]", LIGHT_GRAY, DARK_GRAY);

    // Mouse cursor (uses PS/2 mouse position scaled to the screen)
    let (mx, my) = mouse_pixel_position(w, h);
    draw_cursor(mx, my);

    crate::println!("gfx: desktop drawn. Press any key to return to text mode.");
}

/// Map the text-mode mouse position (80x25) onto framebuffer pixels.
fn mouse_pixel_position(w: usize, h: usize) -> (usize, usize) {
    // Snapshot under interrupts-off — IRQ12 also locks STATE.
    let (sx, sy) = crate::arch::without_interrupts(|| {
        let st = crate::driver::mouse::STATE.lock();
        (st.x, st.y)
    });
    let mx = (sx as usize * w) / 80;
    let my = (sy as usize * h) / 25;
    (mx.min(w - 1), my.min(h - 1))
}

/// Draw a simple arrow-shaped mouse cursor at (x, y).
pub fn draw_cursor(x: usize, y: usize) {
    // A small white arrow with a black outline (12 rows).
    const ARROW: [&str; 12] = [
        "X           ",
        "XX          ",
        "X.X         ",
        "X..X        ",
        "X...X       ",
        "X....X      ",
        "X.....X     ",
        "X......X    ",
        "X...XXXX    ",
        "X..X        ",
        "X.X         ",
        "XX          ",
    ];
    for (row, line) in ARROW.iter().enumerate() {
        for (col, c) in line.chars().enumerate() {
            let colour = match c {
                'X' => BLACK, // outline
                '.' => WHITE, // fill
                _ => continue,
            };
            vesa_put(x + col, y + row, colour);
        }
    }
}
