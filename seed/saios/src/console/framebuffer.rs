use super::backend::ConsoleBackend;
use efi_main::graphics::FramebufferInfo;
use crate::graphics::display::{Display, FramebufferDisplay};
use crate::graphics::framebuffer::Color;
use crate::graphics::font::{glyph_row, FONT_HEIGHT, FONT_WIDTH};
const TAB_WIDTH: usize = 4;

pub struct FramebufferConsole {
    display: Option<FramebufferDisplay>,
    cursor_x: usize,
    cursor_y: usize,
    fg: Color,
    bg: Color,
}

impl FramebufferConsole {
    pub const fn new() -> Self {
        Self {
            display: None,
            cursor_x: 0,
            cursor_y: 0,
            fg: Color::WHITE,
            bg: Color::BLACK,
        }
    }

    pub fn ensure_renderer_ready(&mut self) -> bool {
        self.display.is_some()
    }

    fn text_bounds(&self) -> Option<(usize, usize)> {
        let cols = self.text_columns()?;
        let rows = self.text_rows()?;
        if cols == 0 || rows == 0 {
            return None;
        }
        Some((cols, rows))
    }

    fn scroll_one_text_row(&mut self) {
        let Some(display) = self.display.as_mut() else {
            return;
        };

        let shift_px = FONT_HEIGHT;
        let width = display.width();
        let height = display.height();
        let bg = self.bg.to_u32();

        if shift_px == 0 || shift_px >= height {
            self.clear();
            self.cursor_x = 0;
            self.cursor_y = 0;
            return;
        }

        let stride = display.stride();
        let fb = display.framebuffer();
        for y in 0..(height - shift_px) {
            for x in 0..width {
                let src = ((y + shift_px) * stride + x) * 4;
                let dst = (y * stride + x) * 4;
                unsafe {
                    let s = fb.add(src);
                    let d = fb.add(dst);
                    *d = *s;
                    *d.add(1) = *s.add(1);
                    *d.add(2) = *s.add(2);
                    *d.add(3) = 0;
                }
            }
        }

        let b = (bg & 0xFF) as u8;
        let g = ((bg >> 8) & 0xFF) as u8;
        let r = ((bg >> 16) & 0xFF) as u8;
        for y in (height - shift_px)..height {
            for x in 0..width {
                let off = (y * stride + x) * 4;
                unsafe {
                    let p = fb.add(off);
                    *p = b;
                    *p.add(1) = g;
                    *p.add(2) = r;
                    *p.add(3) = 0;
                }
            }
        }
    }

    fn newline(&mut self) {
        self.cursor_x = 0;
        let Some((_, rows)) = self.text_bounds() else {
            return;
        };

        if self.cursor_y + 1 < rows {
            self.cursor_y += 1;
        } else {
            self.scroll_one_text_row();
            self.cursor_y = rows.saturating_sub(1);
        }
    }

    fn put_char_direct(&mut self, c: char) {
        if let Some(display) = self.display.as_mut() {
            let px = self.cursor_x * FONT_WIDTH;
            let py = self.cursor_y * FONT_HEIGHT;

            let width = display.width();
            let height = display.height();
            let stride = display.stride();
            let fb = display.framebuffer();

            let fg = self.fg.to_u32();
            let bg = self.bg.to_u32();

            for row_idx in 0..FONT_HEIGHT {
                let y = py.saturating_add(row_idx);
                if y >= height {
                    continue;
                }

                let row_bits = glyph_row(c, row_idx);
                for bit in 0..FONT_WIDTH {
                    let x = px.saturating_add(bit);
                    if x >= width {
                        continue;
                    }

                    let mask = 1u8 << bit;
                    let color = if (row_bits & mask) != 0 { fg } else { bg };
                    let offset = (y * stride + x) * 4;

                    unsafe {
                        let p = fb.add(offset);
                        *p = (color & 0xFF) as u8;
                        *p.add(1) = ((color >> 8) & 0xFF) as u8;
                        *p.add(2) = ((color >> 16) & 0xFF) as u8;
                        *p.add(3) = 0;
                    }
                }
            }

            self.cursor_x += 1;
        }
    }

    fn clear_direct(&mut self) {
        if let Some(display) = self.display.as_mut() {
            let width = display.width();
            let height = display.height();
            let stride = display.stride();
            let fb = display.framebuffer();
            let color = self.bg.to_u32();
            let b = (color & 0xFF) as u8;
            let g = ((color >> 8) & 0xFF) as u8;
            let r = ((color >> 16) & 0xFF) as u8;

            for y in 0..height {
                for x in 0..width {
                    let offset = (y * stride + x) * 4;
                    unsafe {
                        let p = fb.add(offset);
                        *p = b;
                        *p.add(1) = g;
                        *p.add(2) = r;
                        *p.add(3) = 0;
                    }
                }
            }
        }
    }

    pub fn attach(&mut self, info: FramebufferInfo) {
        self.display = FramebufferDisplay::from_info(info);

        self.cursor_x = 0;
        self.cursor_y = 0;
        self.clear();
    }

    pub fn text_columns(&self) -> Option<usize> {
        self.display.as_ref().map(|display| display.width() / FONT_WIDTH)
    }

    pub fn text_rows(&self) -> Option<usize> {
        self.display.as_ref().map(|display| display.height() / FONT_HEIGHT)
    }
}

impl ConsoleBackend for FramebufferConsole {
    fn put_char(&mut self, c: char) {
        match c {
            '\n' => {
                self.newline();
                return;
            }
            '\r' => {
                self.cursor_x = 0;
                return;
            }
            '\t' => {
                let spaces = TAB_WIDTH - (self.cursor_x % TAB_WIDTH);
                for _ in 0..spaces {
                    self.put_char(' ');
                }
                return;
            }
            _ => {}
        }

        self.put_char_direct(c);
    }

    fn clear(&mut self) {
        self.clear_direct();

        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = x;
        self.cursor_y = y;
    }

    fn scroll_up(&mut self, rows: usize) -> bool {
        let rows = core::cmp::max(1, rows);
        for _ in 0..rows {
            self.scroll_one_text_row();
        }
        true
    }
}
