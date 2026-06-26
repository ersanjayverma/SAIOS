//! Framebuffer text console (Phase 7).
//!
//! Renders a scrolling text grid onto the linear framebuffer using the 8x16
//! bitmap font.  This is the graphics-mode equivalent of the VGA text buffer:
//! when SAIOS runs with a framebuffer, kernel output can be routed here
//! instead of the 80x25 VGA text memory.
//!
//! The console keeps a character grid sized to the framebuffer and redraws
//! dirty cells.  Scrolling shifts the grid up one row.

use super::font::{CELL_H, CELL_W};
use super::{BLACK, LIGHT_GRAY};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

/// Cursor blink phase, toggled by the timer IRQ (a plain atomic â€” no locking, so
/// it is safe to flip from interrupt context).  The shell loop reads it and
/// redraws the cursor from THREAD context, where taking the CONSOLE/FB locks is
/// safe (drawing from the IRQ would deadlock against a thread holding them).
static BLINK_ON: AtomicBool = AtomicBool::new(true);
static LAST_TOGGLE: AtomicU64 = AtomicU64::new(0);

/// Called from the timer IRQ: flip the blink phase about twice a second.
pub fn tick_blink(now_ticks: u64) {
    // ~9 ticks â‰ˆ 0.5 s at the 18 Hz PIT.
    if now_ticks.wrapping_sub(LAST_TOGGLE.load(Ordering::Relaxed)) >= 9 {
        LAST_TOGGLE.store(now_ticks, Ordering::Relaxed);
        BLINK_ON.fetch_xor(true, Ordering::Relaxed);
    }
}

/// Maximum console grid (covers up to 1920x1080 at 8x16 = 240x67 cells).
const MAX_COLS: usize = 240;
const MAX_ROWS: usize = 68;

/// One character cell: Unicode scalar + colour.
///
/// Older code stored `u8`, which meant UTF-8 text was rendered byte-by-byte.
/// Storing `char` lets the console preserve decoded Unicode code points and
/// pass them to the kernel bitmap font overlay.
#[derive(Clone, Copy)]
struct Cell {
    ch: char,
    fg: u32,
    bg: u32,
}

/// Framebuffer text console state.
pub struct GfxConsole {
    cols: usize,
    rows: usize,
    cx: usize, // cursor column
    cy: usize, // cursor row
    fg: u32,
    bg: u32,
    grid: [[Cell; MAX_COLS]; MAX_ROWS],
    active: bool,
    cursor_drawn: bool, // is the blink cursor currently painted?
}

impl GfxConsole {
    const fn new() -> Self {
        Self {
            cols: 0,
            rows: 0,
            cx: 0,
            cy: 0,
            fg: LIGHT_GRAY,
            bg: BLACK,
            grid: [[Cell {
                ch: ' ',
                fg: LIGHT_GRAY,
                bg: BLACK,
            }; MAX_COLS]; MAX_ROWS],
            active: false,
            cursor_drawn: false,
        }
    }

    /// Compute the grid size from the framebuffer dimensions and clear it.
    fn setup(&mut self) {
        let (w, h) = super::dimensions();
        self.cols = (w / CELL_W).min(MAX_COLS);
        self.rows = (h / CELL_H).min(MAX_ROWS);
        self.cx = 0;
        self.cy = 0;
        self.active = true;
        super::clear(self.bg);
    }

    /// Write one decoded character to the console.
    ///
    /// Control characters are handled here; printable Unicode is stored as a
    /// `char` and rendered through graphics::font::draw_char().
    fn put(&mut self, ch: char) {
        match ch {
            '\n' => {
                self.cx = 0;
                self.advance_row();
            }
            '\r' => {
                self.cx = 0;
            }
            '\x08' => {
                // backspace
                if self.cx > 0 {
                    self.cx -= 1;
                    self.set_cell(self.cx, self.cy, ' ');
                }
            }
            c => {
                if self.cx >= self.cols {
                    self.cx = 0;
                    self.advance_row();
                }
                self.set_cell(self.cx, self.cy, c);
                self.cx += 1;
            }
        }
    }

    fn advance_row(&mut self) {
        if self.cy + 1 >= self.rows {
            self.scroll();
        } else {
            self.cy += 1;
        }
    }

    /// Draw the cursor as an underline on the bottom two scanlines of the cell,
    /// or erase it by redrawing the underlying glyph.  Underline (vs. block) so
    /// it never hides the character beneath it.
    fn render_cursor(&self, on: bool) {
        if !self.active || self.cx >= self.cols || self.cy >= self.rows {
            return;
        }
        let x = self.cx * CELL_W;
        let y = self.cy * CELL_H;
        if on {
            super::fill_rect(x, y + CELL_H - 2, CELL_W, 2, self.fg);
        } else {
            // Erase: repaint the cell's background then its glyph.
            let cell = self.grid[self.cy][self.cx];
            super::fill_rect(x, y, CELL_W, CELL_H, cell.bg);
            super::font::draw_char(x, y, cell.ch, cell.fg, cell.bg);
        }
    }

    /// Draw a single cell to the framebuffer and record it in the grid.
    fn set_cell(&mut self, col: usize, row: usize, ch: char) {
        if col >= self.cols || row >= self.rows {
            return;
        }
        self.grid[row][col] = Cell {
            ch,
            fg: self.fg,
            bg: self.bg,
        };
        super::font::draw_char(col * CELL_W, row * CELL_H, ch, self.fg, self.bg);
    }

    /// Clear the whole console: blank the grid, reset the cursor, wipe the FB.
    fn clear(&mut self) {
        super::clear(self.bg);
        for r in 0..self.rows {
            for c in 0..self.cols {
                self.grid[r][c] = Cell {
                    ch: ' ',
                    fg: self.fg,
                    bg: self.bg,
                };
            }
        }
        self.cx = 0;
        self.cy = 0;
    }

    /// Scroll the grid up one row.
    /// Uses a single framebuffer memmove instead of redrawing every cell â€”
    /// avoids millions of put_pixel() calls (and their per-call FB.lock()
    /// acquisitions) that made scroll take tens of milliseconds.
    fn scroll(&mut self) {
        // Shift the logical grid
        for r in 1..self.rows {
            self.grid[r - 1] = self.grid[r];
        }
        let bg = self.bg;
        for c in 0..self.cols {
            self.grid[self.rows - 1][c] = Cell {
                ch: ' ',
                fg: self.fg,
                bg,
            };
        }
        // Shift the framebuffer pixels in one memmove + clear
        crate::driver::vesa::scroll_up_px(CELL_H, bg);
    }
}

/// Global framebuffer console instance.
pub static CONSOLE: Mutex<GfxConsole> = Mutex::new(GfxConsole::new());

/// Switch the console into graphics mode (sizes the grid to the framebuffer).
pub fn enter() {
    if super::available() {
        CONSOLE.lock().setup();
    }
}

/// Clear the graphics console (used by the shell `clear` command in gfx mode).
pub fn clear() {
    let mut con = CONSOLE.lock();
    if !con.active {
        return;
    }
    con.clear();
}

/// Move the cursor to (col, row) and draw it there â€” used by full-screen apps
/// (nano) to show the editing position.  Erases the cursor at its old spot first.
pub fn set_cursor(col: usize, row: usize) {
    let mut con = CONSOLE.lock();
    if !con.active {
        return;
    }
    if con.cursor_drawn {
        con.render_cursor(false);
        con.cursor_drawn = false;
    }
    con.cx = col.min(con.cols.saturating_sub(1));
    con.cy = row.min(con.rows.saturating_sub(1));
    con.render_cursor(true);
    con.cursor_drawn = true;
}

/// Erase the previous character (shell backspace in gfx mode).
pub fn backspace() {
    let mut con = CONSOLE.lock();
    if !con.active {
        return;
    }
    if con.cursor_drawn {
        con.render_cursor(false);
        con.cursor_drawn = false;
    }
    con.put('\x08');
}

/// Write a string to the graphics console.
/// Plain spinlock â€” no without_interrupts.  No IRQ handler touches CONSOLE
/// (keyboard IRQ only pushes to SCANCODE_QUEUE), so there is no IRQâ†’lock
/// path and therefore no risk of IRQ-context deadlock.
/// without_interrupts here would CAUSE deadlock: if bg_worker holds CONSOLE
/// and the timer switches to the shell, the shell's without_interrupts block
/// disables the timer preventing bg_worker from ever being rescheduled.
pub fn write_str(s: &str) {
    let mut con = CONSOLE.lock();
    if !con.active {
        return;
    }
    // Lift the cursor before writing so its underline doesn't linger at the old
    // position; the blink loop repaints it at the new cursor cell.
    if con.cursor_drawn {
        con.render_cursor(false);
        con.cursor_drawn = false;
    }
    // Decode UTF-8 once at the console boundary. Invalid UTF-8 cannot occur in
    // Rust `&str`; unsupported Unicode scalars fall back inside font::draw_char.
    for ch in s.chars() {
        con.put(ch);
    }
}

/// Repaint the blink cursor to match the current phase.  Called from the shell
/// loop (THREAD context â€” safe to take the CONSOLE/FB locks here).
pub fn update_cursor() {
    let on = BLINK_ON.load(Ordering::Relaxed);
    let mut con = CONSOLE.lock();
    if !con.active {
        return;
    }
    if on && !con.cursor_drawn {
        con.render_cursor(true);
        con.cursor_drawn = true;
    } else if !on && con.cursor_drawn {
        con.render_cursor(false);
        con.cursor_drawn = false;
    }
}
