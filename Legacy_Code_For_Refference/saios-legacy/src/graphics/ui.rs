//! SAIOS native widget toolkit (Phase 9 — GUI stack).
//!
//! The from-scratch, Rust-native equivalent of the GNOME dependency stack:
//!   • Cairo-like 2-D primitives  — alpha blending, lines, rounded rects, gradients
//!   • GTK-like widgets           — Label, Button (with heap-closure callbacks)
//!   • an event loop              — driven by the PS/2 mouse + keyboard, with a
//!                                  save-under hardware-style cursor
//!
//! Rather than cross-compiling GLib/Cairo/Pango/GTK (which need a full glibc +
//! dynamic-linking userspace), SAIOS implements the same capabilities natively,
//! the way it already does for TLS, ext4, FAT and the network stack.
//!
//! Milestone: a window with buttons that respond to clicks.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::font::{self, CELL_H, CELL_W};
use crate::driver::vesa;
use crate::driver::{keyboard, mouse};

// -- Palette ------------------------------------------------------------------
const DESKTOP: u32 = 0x00_141C28;
const WIN_BG: u32 = 0x00_24242E;
const TITLE_TOP: u32 = 0x00_3A6EA5;
const TITLE_BOT: u32 = 0x00_274B73;
const TEXT: u32 = 0x00_E8E8F0;
const SUBTLE: u32 = 0x00_9098A8;
const BTN: u32 = 0x00_3A3A4A;
const BTN_HOVER: u32 = 0x00_4C4C62;
const BTN_DOWN: u32 = 0x00_30D050;
const BORDER: u32 = 0x00_505066;

// -- 2-D primitives (Cairo-like) -----------------------------------------------

/// Alpha-blend a foreground colour over the framebuffer pixel at (x, y).
#[inline]
pub fn blend(x: usize, y: usize, fg: u32, alpha: u8) {
    match alpha {
        0 => {}
        255 => super::vesa_put(x, y, fg),
        a => {
            let bg = vesa::get_pixel(x as u32, y as u32);
            let a = a as u32;
            let ch = |fs: u32, bs: u32| {
                ((((fg >> fs) & 0xFF) * a + ((bg >> bs) & 0xFF) * (255 - a)) / 255) & 0xFF
            };
            let r = ch(16, 16);
            let g = ch(8, 8);
            let b = ch(0, 0);
            super::vesa_put(x, y, (r << 16) | (g << 8) | b);
        }
    }
}

/// Bresenham line.
pub fn line(mut x0: i32, mut y0: i32, x1: i32, y1: i32, colour: u32) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x0 >= 0 && y0 >= 0 {
            super::vesa_put(x0 as usize, y0 as usize, colour);
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/// Filled rectangle with rounded corners.
pub fn rounded_rect(x: usize, y: usize, w: usize, h: usize, r: usize, colour: u32) {
    if w == 0 || h == 0 {
        return;
    }
    let r = r.min(w / 2).min(h / 2);
    // Centre + straight edges.
    super::fill_rect(x + r, y, w - 2 * r, h, colour);
    super::fill_rect(x, y + r, r, h - 2 * r, colour);
    super::fill_rect(x + w - r, y + r, r, h - 2 * r, colour);
    // Four quarter-circle corners.
    for dy in 0..r {
        for dx in 0..r {
            let ddx = (r - dx) as i32;
            let ddy = (r - dy) as i32;
            if ddx * ddx + ddy * ddy <= (r * r) as i32 {
                super::vesa_put(x + dx, y + dy, colour);
                super::vesa_put(x + w - 1 - dx, y + dy, colour);
                super::vesa_put(x + dx, y + h - 1 - dy, colour);
                super::vesa_put(x + w - 1 - dx, y + h - 1 - dy, colour);
            }
        }
    }
}

/// Vertical gradient fill from `top` colour to `bottom` colour.
pub fn gradient_v(x: usize, y: usize, w: usize, h: usize, top: u32, bottom: u32) {
    if h == 0 {
        return;
    }
    for row in 0..h {
        let t = row as u32 * 255 / h as u32;
        let ch = |sh: u32| {
            let a = (top >> sh) & 0xFF;
            let b = (bottom >> sh) & 0xFF;
            ((a * (255 - t) + b * t) / 255) & 0xFF
        };
        let c = (ch(16) << 16) | (ch(8) << 8) | ch(0);
        super::fill_rect(x, y + row, w, 1, c);
    }
}

fn text_centered(rect: Rect, s: &str, fg: u32, bg: u32) {
    let tw = s.chars().count() * CELL_W;
    let tx = rect.x + rect.w.saturating_sub(tw) / 2;
    let ty = rect.y + rect.h.saturating_sub(CELL_H) / 2;
    font::draw_string(tx, ty, s, fg, bg);
}

// -- Geometry ---------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x as i32
            && px < (self.x + self.w) as i32
            && py >= self.y as i32
            && py < (self.y + self.h) as i32
    }
}

// -- Widgets ---------------------------------------------------------------

enum Kind {
    Label,
    /// Re-evaluates its text each redraw (for live state, e.g. a counter).
    DynLabel(fn() -> String),
    Button(Box<dyn FnMut()>),
    /// A toggle; the current state is `Widget.checked`.
    Checkbox,
}

pub struct Widget {
    rect: Rect,
    text: String,
    kind: Kind,
    hovered: bool,
    pressed: bool,
    checked: bool,
}

impl Widget {
    pub fn label(x: usize, y: usize, text: &str) -> Self {
        Self {
            rect: Rect {
                x,
                y,
                w: text.chars().count() * CELL_W,
                h: CELL_H,
            },
            text: text.to_string(),
            kind: Kind::Label,
            hovered: false,
            pressed: false,
            checked: false,
        }
    }
    pub fn dyn_label(x: usize, y: usize, f: fn() -> String) -> Self {
        Self {
            rect: Rect {
                x,
                y,
                w: 40 * CELL_W,
                h: CELL_H,
            },
            text: String::new(),
            kind: Kind::DynLabel(f),
            hovered: false,
            pressed: false,
            checked: false,
        }
    }
    pub fn button(
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        text: &str,
        on: Box<dyn FnMut()>,
    ) -> Self {
        Self {
            rect: Rect { x, y, w, h },
            text: text.to_string(),
            kind: Kind::Button(on),
            hovered: false,
            pressed: false,
            checked: false,
        }
    }
    pub fn checkbox(x: usize, y: usize, text: &str, checked: bool) -> Self {
        Self {
            rect: Rect {
                x,
                y,
                w: 18 + text.chars().count() * CELL_W,
                h: 18,
            },
            text: text.to_string(),
            kind: Kind::Checkbox,
            hovered: false,
            pressed: false,
            checked,
        }
    }

    fn draw(&mut self) {
        match &self.kind {
            Kind::Label => {
                super::fill_rect(self.rect.x, self.rect.y, self.rect.w, CELL_H, WIN_BG);
                font::draw_string(self.rect.x, self.rect.y, &self.text, TEXT, WIN_BG);
            }
            Kind::DynLabel(f) => {
                let s = f();
                super::fill_rect(self.rect.x, self.rect.y, self.rect.w, CELL_H, WIN_BG);
                font::draw_string(self.rect.x, self.rect.y, &s, super::SAIOS_GREEN, WIN_BG);
            }
            Kind::Button(_) => {
                let bg = if self.pressed {
                    BTN_DOWN
                } else if self.hovered {
                    BTN_HOVER
                } else {
                    BTN
                };
                let fg = if self.pressed { 0x00_102010 } else { TEXT };
                rounded_rect(self.rect.x, self.rect.y, self.rect.w, self.rect.h, 6, bg);
                // border
                for i in 0..self.rect.w {
                    blend(self.rect.x + i, self.rect.y, BORDER, 120);
                    blend(self.rect.x + i, self.rect.y + self.rect.h - 1, BORDER, 120);
                }
                text_centered(self.rect, &self.text, fg, bg);
            }
            Kind::Checkbox => {
                // 16x16 box + label.
                super::fill_rect(self.rect.x, self.rect.y, self.rect.w, 18, WIN_BG);
                let box_bg = if self.checked { BTN_DOWN } else { BTN };
                rounded_rect(self.rect.x, self.rect.y, 16, 16, 3, box_bg);
                super::draw_rect(self.rect.x, self.rect.y, 16, 16, BORDER);
                if self.checked {
                    // simple check mark
                    line(
                        self.rect.x as i32 + 3,
                        self.rect.y as i32 + 8,
                        self.rect.x as i32 + 6,
                        self.rect.y as i32 + 12,
                        0x00_102010,
                    );
                    line(
                        self.rect.x as i32 + 6,
                        self.rect.y as i32 + 12,
                        self.rect.x as i32 + 13,
                        self.rect.y as i32 + 3,
                        0x00_102010,
                    );
                }
                font::draw_string(self.rect.x + 22, self.rect.y + 1, &self.text, TEXT, WIN_BG);
            }
        }
    }
}

// -- Application window + event loop ------------------------------------------

pub struct App {
    title: String,
    rect: Rect,
    widgets: Vec<Widget>,
}

impl App {
    pub fn new(title: &str, x: usize, y: usize, w: usize, h: usize) -> Self {
        Self {
            title: title.to_string(),
            rect: Rect { x, y, w, h },
            widgets: Vec::new(),
        }
    }
    pub fn add(&mut self, w: Widget) {
        self.widgets.push(w);
    }

    fn close_btn(&self) -> Rect {
        Rect {
            x: self.rect.x + self.rect.w - 26,
            y: self.rect.y + 5,
            w: 20,
            h: 20,
        }
    }

    fn draw_frame(&mut self) {
        let r = self.rect;
        // Body, gradient title bar, title text, border.
        super::fill_rect(r.x, r.y, r.w, r.h, WIN_BG);
        gradient_v(r.x, r.y, r.w, 30, TITLE_TOP, TITLE_BOT);
        font::draw_string(r.x + 10, r.y + 7, &self.title, 0x00_FFFFFF, TITLE_BOT);
        super::draw_rect(r.x, r.y, r.w, r.h, BORDER);
        // Close button.
        let cb = self.close_btn();
        rounded_rect(cb.x, cb.y, cb.w, cb.h, 4, 0x00_C0403C);
        text_centered(cb, "x", 0x00_FFFFFF, 0x00_C0403C);
        for w in self.widgets.iter_mut() {
            w.draw();
        }
    }

    /// Run the modal event loop until ESC or the quit flag is set.
    pub fn run(&mut self) {
        if !super::available() {
            crate::println!("ui: no framebuffer");
            return;
        }
        let (fw, fh) = super::dimensions();
        mouse::set_gfx_bounds(fw as i32, fh as i32);
        QUIT.store(false, Ordering::Relaxed);
        // A GUI event loop hard-requires interrupts (timer wakes hlt; IRQ1/IRQ12
        // deliver input).  Enable them defensively so the loop can never freeze.
        crate::arch::enable_interrupts();

        crate::vga_buffer::use_gfx_console(false); // suspend text console output
        super::clear(DESKTOP);
        self.draw_frame();

        let (mut cx, mut cy, _l) = mouse::gfx_state();
        let mut cur = Cursor::new();
        cur.save_and_draw(cx, cy);
        let mut prev_left = false;

        loop {
            let hb = HEARTBEAT.fetch_add(1, Ordering::Relaxed);
            // Refresh the live status label a few times a second.
            let mut dirty = hb.is_multiple_of(9);

            // Keyboard.
            if let Some(ev) = keyboard::poll() {
                match ev {
                    keyboard::KeyEvent::Escape => break,
                    keyboard::KeyEvent::Char(c) => {
                        LAST_KEY.store(c as u32, Ordering::Relaxed);
                        dirty = true;
                    }
                    _ => {
                        LAST_KEY.store('?' as u32, Ordering::Relaxed);
                        dirty = true;
                    }
                }
            }
            if QUIT.load(Ordering::Relaxed) {
                break;
            }

            let (nx, ny, left) = mouse::gfx_state();

            // Hover tracking.
            for w in self.widgets.iter_mut() {
                if matches!(w.kind, Kind::Button(_)) {
                    let h = w.rect.contains(nx, ny);
                    if h != w.hovered {
                        w.hovered = h;
                        dirty = true;
                    }
                }
            }

            // Click edge detection.
            if left && !prev_left {
                for w in self.widgets.iter_mut() {
                    if matches!(w.kind, Kind::Button(_)) && w.rect.contains(nx, ny) {
                        w.pressed = true;
                        dirty = true;
                    }
                }
            } else if !left && prev_left {
                // Close button → quit.
                if self.close_btn().contains(nx, ny) {
                    QUIT.store(true, Ordering::Relaxed);
                }
                for w in self.widgets.iter_mut() {
                    if w.pressed {
                        w.pressed = false;
                        dirty = true;
                        if w.rect.contains(nx, ny)
                            && let Kind::Button(cb) = &mut w.kind
                        {
                            cb();
                        }
                    }
                    // Checkboxes toggle on release-over.
                    if matches!(w.kind, Kind::Checkbox) && w.rect.contains(nx, ny) {
                        w.checked = !w.checked;
                        dirty = true;
                    }
                }
            }
            prev_left = left;

            // Redraw (widgets first, then re-place the cursor on top).
            if dirty {
                cur.restore();
                self.draw_frame();
                cur.save_and_draw(nx, ny);
            } else if nx != cx || ny != cy {
                cur.restore();
                cur.save_and_draw(nx, ny);
            }
            cx = nx;
            cy = ny;

            crate::arch::halt();
        }

        // Restore text mode.
        super::clear(DESKTOP);
        crate::vga_buffer::use_gfx_console(true);
        crate::vga_buffer::clear();
    }
}

const BLACK_: u32 = 0x00_000000;

// Demo state.
static CLICKS: AtomicU32 = AtomicU32::new(0);
static QUIT: AtomicBool = AtomicBool::new(false);
static HEARTBEAT: AtomicU32 = AtomicU32::new(0); // event-loop tick (diagnostic)
static LAST_KEY: AtomicU32 = AtomicU32::new(0); // last key seen (diagnostic)

fn clicks_text() -> String {
    alloc::format!("Clicks: {}", CLICKS.load(Ordering::Relaxed))
}

/// Live input diagnostic: loop heartbeat + mouse position/button + last key.
fn status_text() -> String {
    let (x, y, l) = mouse::gfx_state();
    let k = LAST_KEY.load(Ordering::Relaxed);
    let kc = char::from_u32(k).filter(|c| *c >= ' ').unwrap_or(' ');
    alloc::format!(
        "hb {}  mouse {},{} {}  key '{}'",
        HEARTBEAT.load(Ordering::Relaxed),
        x,
        y,
        if l { "DOWN" } else { "up" },
        kc
    )
}

/// `gfx ui` — open the demo widget window.
pub fn demo() {
    let (fw, fh) = super::dimensions();
    if fw == 0 {
        crate::println!("ui: no framebuffer (boot in graphics mode)");
        return;
    }
    let w = 460usize.min(fw - 20);
    let h = 280usize.min(fh - 20);
    let x = (fw - w) / 2;
    let y = (fh - h) / 2;

    let mut app = App::new("SAIOS Widgets - Phase 9", x, y, w, h);
    app.add(Widget::label(
        x + 16,
        y + 44,
        "Native GTK-style toolkit (from scratch)",
    ));
    app.add(Widget::dyn_label(x + 16, y + 70, clicks_text));
    app.add(Widget::button(
        x + 16,
        y + 100,
        130,
        40,
        "Click me",
        Box::new(|| {
            CLICKS.fetch_add(1, Ordering::Relaxed);
        }),
    ));
    app.add(Widget::button(
        x + 160,
        y + 100,
        130,
        40,
        "Beep",
        Box::new(|| {
            crate::driver::hda::beep(880, 80);
        }),
    ));
    app.add(Widget::button(
        x + 16,
        y + 150,
        130,
        40,
        "Quit",
        Box::new(|| {
            QUIT.store(true, Ordering::Relaxed);
        }),
    ));
    app.add(Widget::checkbox(x + 160, y + 160, "Checkbox widget", true));
    app.add(Widget::dyn_label(x + 16, y + h - 44, status_text)); // live input diagnostic
    app.add(Widget::label(
        x + 16,
        y + h - 24,
        "Move mouse / press keys (watch the line above); ESC exits",
    ));

    app.run();
    crate::println!("ui: closed (clicks: {})", CLICKS.load(Ordering::Relaxed));
}

// -- Save-under mouse cursor --------------------------------------------------

const CW: usize = 12;
const CH: usize = 19;
// '1' = white fill, '2' = black outline, anything else (space) = transparent.
// Variable-length rows are fine — bytes past a row's length are transparent.
const ARROW: [&[u8]; CH] = [
    b"2",
    b"22",
    b"212",
    b"2112",
    b"21112",
    b"211112",
    b"2111112",
    b"21111112",
    b"211111112",
    b"2111111112",
    b"21111111112",
    b"2111112222",
    b"211 2112",
    b"21   2112",
    b"2     2112",
    b"      2112",
    b"       2112",
    b"       2112",
    b"        22",
];

struct Cursor {
    buf: [u32; CW * CH],
    x: usize,
    y: usize,
    valid: bool,
}

impl Cursor {
    fn new() -> Self {
        Self {
            buf: [0; CW * CH],
            x: 0,
            y: 0,
            valid: false,
        }
    }

    fn save_and_draw(&mut self, x: i32, y: i32) {
        let x = x.max(0) as usize;
        let y = y.max(0) as usize;
        for dy in 0..CH {
            for dx in 0..CW {
                self.buf[dy * CW + dx] = vesa::get_pixel((x + dx) as u32, (y + dy) as u32);
            }
        }
        self.x = x;
        self.y = y;
        self.valid = true;
        for (dy, row) in ARROW.iter().enumerate() {
            for (dx, &c) in row.iter().enumerate() {
                match c {
                    b'1' => super::vesa_put(x + dx, y + dy, 0x00_FFFFFF),
                    b'2' => super::vesa_put(x + dx, y + dy, 0x00_000000),
                    _ => {}
                }
            }
        }
    }

    fn restore(&mut self) {
        if !self.valid {
            return;
        }
        for dy in 0..CH {
            for dx in 0..CW {
                super::vesa_put(self.x + dx, self.y + dy, self.buf[dy * CW + dx]);
            }
        }
    }
}
