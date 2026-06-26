//! Simple windows for the SAIOS compositor (Phase 7).
//!
//! A `Window` is a rectangular region with a title bar and a content area.
//! Drawing is immediate-mode (no retained scene graph yet) — the compositor
//! redraws windows on demand.  This is the foundation that a Wayland-style
//! compositor (Phase 8) will build on.

use super::font::{CELL_H, CELL_W};
use super::*;

/// Title-bar height in pixels.
const TITLEBAR_H: usize = 24;
/// Window border thickness.
const BORDER: usize = 2;

/// A single on-screen window.
pub struct Window {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    pub title: &'static str,
    /// Title-bar colour (changes when focused).
    pub focused: bool,
}

impl Window {
    /// Create a new window at (x, y) of size (w, h) with a title.
    pub fn new(x: usize, y: usize, w: usize, h: usize, title: &'static str) -> Self {
        Self {
            x,
            y,
            w,
            h,
            title,
            focused: true,
        }
    }

    /// Draw the window frame (border, title bar, content background).
    pub fn draw(&self) {
        // Drop shadow (offset dark rectangle)
        fill_rect(self.x + 4, self.y + 4, self.w, self.h, 0x00_101820);

        // Window background (content area)
        fill_rect(self.x, self.y, self.w, self.h, 0x00_F0F0F0);

        // Title bar
        let bar_colour = if self.focused { SAIOS_GREEN } else { GRAY };
        fill_rect(self.x, self.y, self.w, TITLEBAR_H, bar_colour);
        super::font::draw_string(self.x + 6, self.y + 4, self.title, BLACK, bar_colour);

        // Close button (a red square on the right of the title bar)
        let bx = self.x + self.w - 20;
        fill_rect(bx, self.y + 5, 14, 14, RED);
        super::font::draw_string(bx + 3, self.y + 5, "x", WHITE, RED);

        // Border
        draw_rect(self.x, self.y, self.w, self.h, DARK_GRAY);
    }

    /// Draw a line of text inside the content area at text-row `row`
    /// (row 0 = first line below the title bar).
    pub fn draw_text(&self, row: usize, text: &str) {
        let tx = self.x + 6;
        let ty = self.y + TITLEBAR_H + 4 + row * CELL_H;
        // Clip vertically to the window content area.
        if ty + CELL_H > self.y + self.h - BORDER {
            return;
        }
        super::font::draw_string(tx, ty, text, BLACK, 0x00_F0F0F0);
    }

    /// Return true if the point (px, py) is inside the window.
    pub fn contains(&self, px: usize, py: usize) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// Return true if the point is on the close button.
    pub fn close_hit(&self, px: usize, py: usize) -> bool {
        let bx = self.x + self.w - 20;
        px >= bx && px < bx + 14 && py >= self.y + 5 && py < self.y + 19
    }
}
