use super::backend::ConsoleBackend;
use alloc::string::String;
use alloc::vec::Vec;
use efi_main::graphics::PixelFormat;
use efi_main::graphics::FramebufferInfo;
use crate::graphics::display::{Display, FramebufferDisplay};
use crate::graphics::framebuffer::Color;
use crate::graphics::font::{glyph_row, FONT_HEIGHT, FONT_WIDTH};

const TAB_WIDTH: usize = 4;
const MAX_TEXT_COLS: usize = 160;
const MAX_TEXT_ROWS: usize = 100;
const MAX_SCROLLBACK_LINES: usize = 2048;

pub struct FramebufferConsole {
    display: Option<FramebufferDisplay>,
    cursor_x: usize,
    cursor_y: usize,
    fg: Color,
    bg: Color,
    screen: [[char; MAX_TEXT_COLS]; MAX_TEXT_ROWS],
    view_offset_lines: usize,
    scrollback: Vec<String>,
}

impl FramebufferConsole {
    pub const fn new() -> Self {
        Self {
            display: None,
            cursor_x: 0,
            cursor_y: 0,
            fg: Color::WHITE,
            bg: Color::BLACK,
            screen: [[' '; MAX_TEXT_COLS]; MAX_TEXT_ROWS],
            view_offset_lines: 0,
            scrollback: Vec::new(),
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
        Some((cols.min(MAX_TEXT_COLS), rows.min(MAX_TEXT_ROWS)))
    }

    fn draw_cell(&mut self, cell_x: usize, cell_y: usize, c: char) {
        let Some(display) = self.display.as_mut() else {
            return;
        };

        let px = cell_x * FONT_WIDTH;
        let py = cell_y * FONT_HEIGHT;

        let width = display.width();
        let height = display.height();
        let stride = display.stride();
        let pixel_format = display.pixel_format();
        let pixel_masks = display.pixel_masks();
        let bytes_per_pixel = display.bytes_per_pixel();
        let fb_size = display.framebuffer_size();
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
                let offset = (y * stride + x) * bytes_per_pixel;
                if offset + bytes_per_pixel > fb_size {
                    continue;
                }

                unsafe {
                    let p = fb.add(offset);
                    Self::write_pixel(p, color, pixel_format, pixel_masks, bytes_per_pixel);
                }
            }
        }
    }

    #[inline(always)]
    unsafe fn write_pixel(
        dst: *mut u8,
        color: u32,
        pixel_format: PixelFormat,
        masks: (u32, u32, u32, u32),
        bytes_per_pixel: usize,
    ) {
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;

        match pixel_format {
            PixelFormat::Rgb => {
                *dst = r;
                *dst.add(1) = g;
                *dst.add(2) = b;
                // Keep alpha non-zero for framebuffers that use channel 3.
                if bytes_per_pixel >= 4 {
                    *dst.add(3) = 0xFF;
                }
            }
            PixelFormat::Bgr => {
                *dst = b;
                *dst.add(1) = g;
                *dst.add(2) = r;
                // Keep alpha non-zero for framebuffers that use channel 3.
                if bytes_per_pixel >= 4 {
                    *dst.add(3) = 0xFF;
                }
            }
            PixelFormat::Bitmask => {
                let pixel = Self::pack_bitmask(r, g, b, masks);
                Self::write_packed(dst, pixel, bytes_per_pixel);
            }
            PixelFormat::BltOnly => {
                *dst = b;
                *dst.add(1) = g;
                *dst.add(2) = r;
                if bytes_per_pixel >= 4 {
                    *dst.add(3) = 0xFF;
                }
            }
        }
    }

    #[inline(always)]
    unsafe fn write_packed(dst: *mut u8, packed: u32, bytes_per_pixel: usize) {
        let bytes = packed.to_le_bytes();
        let count = core::cmp::min(bytes_per_pixel, 4);
        let mut i = 0;
        while i < count {
            *dst.add(i) = bytes[i];
            i += 1;
        }
    }

    #[inline(always)]
    fn pack_channel(value: u8, mask: u32) -> u32 {
        if mask == 0 {
            return 0;
        }

        let shift = mask.trailing_zeros();
        let width = mask.count_ones();
        if width == 0 {
            return 0;
        }

        let max = (1u32 << width) - 1;
        let scaled = ((value as u32) * max + 127) / 255;
        (scaled << shift) & mask
    }

    #[inline(always)]
    fn pack_bitmask(r: u8, g: u8, b: u8, masks: (u32, u32, u32, u32)) -> u32 {
        let (red_mask, green_mask, blue_mask, reserved_mask) = masks;
        Self::pack_channel(r, red_mask)
            | Self::pack_channel(g, green_mask)
            | Self::pack_channel(b, blue_mask)
            | reserved_mask
    }

    fn row_to_scrollback(&self, row: usize, cols: usize) -> String {
        let mut out = String::new();
        let cols = cols.min(MAX_TEXT_COLS);
        for x in 0..cols {
            out.push(self.screen[row][x]);
        }

        while out.ends_with(' ') {
            out.pop();
        }

        out
    }

    fn push_scrollback_line(&mut self, line: String) {
        self.scrollback.push(line);
        if self.scrollback.len() > MAX_SCROLLBACK_LINES {
            let overflow = self.scrollback.len() - MAX_SCROLLBACK_LINES;
            self.scrollback.drain(0..overflow);
        }
    }

    fn clear_screen_model(&mut self, cols: usize, rows: usize) {
        let cols = cols.min(MAX_TEXT_COLS);
        let rows = rows.min(MAX_TEXT_ROWS);
        for y in 0..rows {
            for x in 0..cols {
                self.screen[y][x] = ' ';
            }
        }
    }

    fn render_viewport(&mut self) {
        let Some((cols, rows)) = self.text_bounds() else {
            return;
        };

        self.clear_direct();

        let total_lines = self.scrollback.len() + rows;
        let max_offset = total_lines.saturating_sub(rows);
        if self.view_offset_lines > max_offset {
            self.view_offset_lines = max_offset;
        }

        let start = total_lines.saturating_sub(rows + self.view_offset_lines);
        for row in 0..rows {
            let line_idx = start + row;
            if line_idx < self.scrollback.len() {
                let line = self.scrollback[line_idx].clone();
                for (x, ch) in line.chars().take(cols).enumerate() {
                    self.draw_cell(x, row, ch);
                }
            } else {
                let screen_row = line_idx - self.scrollback.len();
                if screen_row < rows {
                    for x in 0..cols {
                        self.draw_cell(x, row, self.screen[screen_row][x]);
                    }
                }
            }
        }

        if self.view_offset_lines == 0 {
            self.cursor_x = self.cursor_x.min(cols.saturating_sub(1));
            self.cursor_y = self.cursor_y.min(rows.saturating_sub(1));
        }
    }

    fn clear_direct(&mut self) {
        if let Some(display) = self.display.as_mut() {
            let width = display.width();
            let height = display.height();
            let stride = display.stride();
            let pixel_format = display.pixel_format();
            let pixel_masks = display.pixel_masks();
            let bytes_per_pixel = display.bytes_per_pixel();
            let fb_size = display.framebuffer_size();
            let fb = display.framebuffer();
            let color = self.bg.to_u32();

            for y in 0..height {
                for x in 0..width {
                    let offset = (y * stride + x) * bytes_per_pixel;
                    if offset + bytes_per_pixel > fb_size {
                        continue;
                    }
                    unsafe {
                        let p = fb.add(offset);
                        Self::write_pixel(p, color, pixel_format, pixel_masks, bytes_per_pixel);
                    }
                }
            }
        }
    }

    pub fn scroll_view_lines(&mut self, lines: isize) -> bool {
        let Some((_, rows)) = self.text_bounds() else {
            return false;
        };

        let total_lines = self.scrollback.len() + rows;
        let max_offset = total_lines.saturating_sub(rows);
        let before = self.view_offset_lines;

        if lines > 0 {
            self.view_offset_lines = self
                .view_offset_lines
                .saturating_add(lines as usize)
                .min(max_offset);
        } else if lines < 0 {
            self.view_offset_lines = self.view_offset_lines.saturating_sub((-lines) as usize);
        }

        let changed = before != self.view_offset_lines;
        if changed {
            self.render_viewport();
        }
        changed
    }

    pub fn scroll_to_bottom(&mut self) -> bool {
        if self.view_offset_lines == 0 {
            return false;
        }
        self.view_offset_lines = 0;
        self.render_viewport();
        true
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
        let Some((cols, rows)) = self.text_bounds() else {
            return;
        };

        match c {
            '\n' => {
                self.cursor_x = 0;
                self.cursor_y = self.cursor_y.saturating_add(1).min(rows.saturating_sub(1));
            }
            '\r' => {
                self.cursor_x = 0;
            }
            '\t' => {
                let spaces = TAB_WIDTH - (self.cursor_x % TAB_WIDTH.max(1));
                for _ in 0..spaces {
                    self.put_char(' ');
                }
                return;
            }
            _ => {
                let x = self.cursor_x.min(cols.saturating_sub(1));
                let y = self.cursor_y.min(rows.saturating_sub(1));
                self.screen[y][x] = c;
                if self.view_offset_lines == 0 {
                    self.draw_cell(x, y, c);
                }
                self.cursor_x = self.cursor_x.saturating_add(1).min(cols.saturating_sub(1));
            }
        }

        if self.view_offset_lines > 0 {
            self.render_viewport();
        }
    }

    fn clear(&mut self) {
        self.clear_direct();
        if let Some((cols, rows)) = self.text_bounds() {
            self.clear_screen_model(cols, rows);
        }

        self.cursor_x = 0;
        self.cursor_y = 0;
        self.view_offset_lines = 0;
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = x;
        self.cursor_y = y;
    }

    fn scroll_up(&mut self, rows: usize) -> bool {
        let Some((cols, row_count)) = self.text_bounds() else {
            return false;
        };

        let rows = core::cmp::max(1, rows).min(row_count);
        for _ in 0..rows {
            let vanished = self.row_to_scrollback(0, cols);
            self.push_scrollback_line(vanished);

            for y in 1..row_count {
                for x in 0..cols {
                    self.screen[y - 1][x] = self.screen[y][x];
                }
            }
            for x in 0..cols {
                self.screen[row_count - 1][x] = ' ';
            }
        }

        if self.view_offset_lines == 0 {
            self.render_viewport();
        } else {
            let max_offset = (self.scrollback.len() + row_count).saturating_sub(row_count);
            self.view_offset_lines = self.view_offset_lines.min(max_offset);
            self.render_viewport();
        }

        true
    }
}
