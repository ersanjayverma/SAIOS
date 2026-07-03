use super::backend::ConsoleBackend;
use crate::graphics::display::{Display, FramebufferDisplay};
use crate::graphics::font::{FONT_HEIGHT, FONT_WIDTH, glyph_row};
use crate::graphics::framebuffer::Color;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr;
use efi_main::graphics::FramebufferInfo;
use efi_main::graphics::PixelFormat;

const TAB_WIDTH: usize = 4;
const MAX_TEXT_COLS: usize = 160;
const MAX_TEXT_ROWS: usize = 100;
const MAX_SCROLLBACK_LINES: usize = 2048;

/// A text console rendered onto a linear framebuffer.
///
/// It keeps a character grid ([`screen`](Self::screen)) plus a scrollback
/// buffer of lines that have scrolled off the top, and draws glyphs directly to
/// the hardware framebuffer through a [`FramebufferDisplay`]. Rendering is
/// incremental: normal output touches only the changed cell, while scrolling
/// uses a fast framebuffer `memmove`. When the user scrolls back through
/// history the whole viewport is repainted from the character model.
pub struct FramebufferConsole {
    display: Option<FramebufferDisplay>,
    cursor_x: usize,
    cursor_y: usize,
    fg: Color,
    bg: Color,
    screen: [[char; MAX_TEXT_COLS]; MAX_TEXT_ROWS],
    view_offset_lines: usize,
    scrollback: Vec<String>,
    /// Whether the cursor cell is currently inverted (drawn) or normal.
    cursor_inverted: bool,
}

impl FramebufferConsole {
    /// Create an unattached console. No drawing happens until
    /// [`attach`](Self::attach) supplies framebuffer info.
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
            cursor_inverted: false,
        }
    }

    /// Returns `true` once a framebuffer display has been attached and drawing
    /// is possible.
    pub fn ensure_renderer_ready(&mut self) -> bool {
        self.display.is_some()
    }

    /// Current usable text grid size as `(columns, rows)`, clamped to the fixed
    /// screen-model capacity. Returns `None` when no display is attached.
    fn text_bounds(&self) -> Option<(usize, usize)> {
        let cols = self.text_columns()?;
        let rows = self.text_rows()?;
        if cols == 0 || rows == 0 {
            return None;
        }
        Some((cols.min(MAX_TEXT_COLS), rows.min(MAX_TEXT_ROWS)))
    }

    /// Draw a single character at text-grid cell `(cell_x, cell_y)` using the
    /// current foreground/background colors. Chooses a `u32`-slice fast path for
    /// 32-bit RGB/BGR displays and a generic byte writer otherwise.
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

        // OPTIMIZATION: Pre-pack colors once instead of per-pixel
        let (fg_packed, bg_packed) = if bytes_per_pixel == 4
            && matches!(pixel_format, PixelFormat::Bgr | PixelFormat::Rgb)
        {
            let pack = |color: u32| -> u32 {
                let r = (color >> 16) & 0xFF;
                let g = (color >> 8) & 0xFF;
                let b = color & 0xFF;
                match pixel_format {
                    PixelFormat::Bgr => b | (g << 8) | (r << 16) | (0xFF << 24),
                    PixelFormat::Rgb => r | (g << 8) | (b << 16) | (0xFF << 24),
                    PixelFormat::Bitmask | PixelFormat::BltOnly => 0,
                }
            };
            (pack(fg), pack(bg))
        } else {
            (0, 0)
        };

        if bytes_per_pixel == 4 && matches!(pixel_format, PixelFormat::Bgr | PixelFormat::Rgb) {
            // Fast path: view the framebuffer as a u32 slice and copy each
            // 8-pixel glyph row as a contiguous slice.  This avoids the
            // per-pixel branch and bounds checks of the generic path.
            let total_pixels = fb_size / 4;
            let fb32 = unsafe { core::slice::from_raw_parts_mut(fb.cast::<u32>(), total_pixels) };

            let draw_width = core::cmp::min(FONT_WIDTH, width.saturating_sub(px));
            let draw_height = core::cmp::min(FONT_HEIGHT, height.saturating_sub(py));

            for row_idx in 0..draw_height {
                let y = py + row_idx;
                let row_bits = glyph_row(c, row_idx);
                let mut row_colors = [bg_packed; FONT_WIDTH];
                let mut bits = row_bits;
                for px in row_colors.iter_mut().take(draw_width) {
                    if (bits & 1) != 0 {
                        *px = fg_packed;
                    }
                    bits >>= 1;
                }

                let row_base = y.saturating_mul(stride);
                let dst_start = row_base + px;
                let dst_end = core::cmp::min(dst_start + draw_width, total_pixels);
                let copy_width = dst_end.saturating_sub(dst_start);
                if copy_width == 0 {
                    continue;
                }
                fb32[dst_start..dst_start + copy_width].copy_from_slice(&row_colors[..copy_width]);
            }
            return;
        }

        // Fallback for other pixel formats (slower path, but still optimizable)
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

        unsafe {
            match pixel_format {
                PixelFormat::Rgb => {
                    // OPTIMIZATION: Use ptr::write instead of write_volatile
                    // Framebuffer memory is normal RAM, not MMIO
                    ptr::write(dst, r);
                    ptr::write(dst.add(1), g);
                    ptr::write(dst.add(2), b);
                    // Keep alpha non-zero for framebuffers that use channel 3.
                    if bytes_per_pixel >= 4 {
                        ptr::write(dst.add(3), 0xFF);
                    }
                }
                PixelFormat::Bgr => {
                    ptr::write(dst, b);
                    ptr::write(dst.add(1), g);
                    ptr::write(dst.add(2), r);
                    // Keep alpha non-zero for framebuffers that use channel 3.
                    if bytes_per_pixel >= 4 {
                        ptr::write(dst.add(3), 0xFF);
                    }
                }
                PixelFormat::Bitmask => {
                    let pixel = Self::pack_bitmask(r, g, b, masks);
                    Self::write_packed(dst, pixel, bytes_per_pixel);
                }
                PixelFormat::BltOnly => {
                    ptr::write(dst, b);
                    ptr::write(dst.add(1), g);
                    ptr::write(dst.add(2), r);
                    if bytes_per_pixel >= 4 {
                        ptr::write(dst.add(3), 0xFF);
                    }
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
            unsafe {
                // OPTIMIZATION: Use ptr::write instead of write_volatile
                ptr::write(dst.add(i), bytes[i]);
            }
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

    /// Snapshot a screen-model row into an owned `String`, trimming trailing
    /// blanks, for storage in the scrollback history.
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

    /// Append a line to scrollback, evicting the oldest lines once the history
    /// cap ([`MAX_SCROLLBACK_LINES`]) is exceeded.
    fn push_scrollback_line(&mut self, line: String) {
        self.scrollback.push(line);
        if self.scrollback.len() > MAX_SCROLLBACK_LINES {
            let overflow = self.scrollback.len() - MAX_SCROLLBACK_LINES;
            self.scrollback.drain(0..overflow);
        }
    }

    /// Reset the in-memory character grid to blanks (does not touch pixels).
    fn clear_screen_model(&mut self, cols: usize, rows: usize) {
        let cols = cols.min(MAX_TEXT_COLS);
        let rows = rows.min(MAX_TEXT_ROWS);
        for y in 0..rows {
            for x in 0..cols {
                self.screen[y][x] = ' ';
            }
        }
    }

    /// Fully repaint the screen from the character model plus scrollback,
    /// honoring the current scroll-back offset. Used when the visible window is
    /// not simply the live bottom of the buffer.
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
                let mut row_chars = [' '; MAX_TEXT_COLS];
                for (x, ch) in self.scrollback[line_idx].chars().take(cols).enumerate() {
                    row_chars[x] = ch;
                }
                for (x, ch) in row_chars.iter().take(cols).enumerate() {
                    self.draw_cell(x, row, *ch);
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

    /// Clear the physical framebuffer to the current background color.
    fn clear_direct(&mut self) {
        if let Some(display) = self.display.as_mut() {
            display.clear_color(self.bg.to_u32());
        }
    }

    /// Scroll the visible framebuffer up by `text_rows` character rows using a
    /// single `memmove`, then clear the newly exposed rows at the bottom.
    /// Returns `false` if it cannot do the fast in-place scroll (the caller then
    /// falls back to a full repaint).
    fn scroll_pixels_up(&mut self, text_rows: usize) -> bool {
        let Some(display) = self.display.as_mut() else {
            return false;
        };

        let shift_px = text_rows.saturating_mul(FONT_HEIGHT);
        if shift_px == 0 {
            return true;
        }

        let width = display.width();
        let height = display.height();
        let stride = display.stride();
        let bytes_per_pixel = display.bytes_per_pixel();
        let fb_size = display.framebuffer_size();
        let pixel_format = display.pixel_format();
        let pixel_masks = display.pixel_masks();
        let fb = display.framebuffer();
        let bg = self.bg.to_u32();

        if width == 0 || height == 0 || bytes_per_pixel == 0 || stride == 0 {
            return false;
        }

        let shift_px = shift_px.min(height);
        let row_bytes = stride.saturating_mul(bytes_per_pixel);
        if row_bytes == 0 {
            return false;
        }

        // OPTIMIZATION: Use memmove for scroll instead of pixel operations
        if shift_px < height {
            let src_offset = shift_px.saturating_mul(row_bytes);
            let copy_bytes = (height - shift_px).saturating_mul(row_bytes);
            if src_offset.saturating_add(copy_bytes) > fb_size {
                return false;
            }

            unsafe {
                ptr::copy(fb.add(src_offset), fb, copy_bytes);
            }
        }

        let clear_start = height - shift_px;

        let pack_color = |color: u32| -> u32 {
            let r = ((color >> 16) & 0xFF) as u8;
            let g = ((color >> 8) & 0xFF) as u8;
            let b = (color & 0xFF) as u8;
            match pixel_format {
                PixelFormat::Bgr => {
                    (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | (0xFF << 24)
                }
                PixelFormat::Rgb => {
                    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16) | (0xFF << 24)
                }
                PixelFormat::Bitmask => Self::pack_bitmask(r, g, b, pixel_masks),
                PixelFormat::BltOnly => {
                    (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | (0xFF << 24)
                }
            }
        };

        let bg_packed = pack_color(bg);

        // Clear the freshly exposed rows at the bottom of the screen.
        if bytes_per_pixel == 4 {
            // Fast path: fill whole rows through a u32 slice view of the
            // framebuffer using `slice::fill` (a tight memory-set loop).
            let total_pixels = fb_size / 4;
            let fb32 = unsafe { core::slice::from_raw_parts_mut(fb.cast::<u32>(), total_pixels) };
            for y in clear_start..height {
                let row_base = y.saturating_mul(stride);
                if row_base >= total_pixels {
                    break;
                }
                let end = core::cmp::min(row_base + width, total_pixels);
                fb32[row_base..end].fill(bg_packed);
            }
            return true;
        }

        // Fallback for non-4-byte formats
        let bg_bytes = bg_packed.to_le_bytes();
        for y in clear_start..height {
            let row_base = y.saturating_mul(stride);
            for x in 0..width {
                let offset = (row_base + x).saturating_mul(bytes_per_pixel);
                if offset + bytes_per_pixel > fb_size {
                    break;
                }
                unsafe {
                    let p = fb.add(offset);
                    let mut i = 0;
                    while i < bytes_per_pixel {
                        // OPTIMIZATION: Use ptr::write instead of write_volatile
                        ptr::write(p.add(i), bg_bytes[i]);
                        i += 1;
                    }
                }
            }
        }

        true
    }

    /// Move the scroll-back view by `lines` (positive scrolls into history,
    /// negative back toward the live output) and repaint if it changed.
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

    /// Jump the view back to the live bottom of the output, repainting if the
    /// view was previously scrolled up.
    pub fn scroll_to_bottom(&mut self) -> bool {
        if self.view_offset_lines == 0 {
            return false;
        }
        self.view_offset_lines = 0;
        self.render_viewport();
        true
    }

    /// Bind the console to a hardware framebuffer described by `info`, resetting
    /// the cursor and clearing the screen.
    pub fn attach(&mut self, info: FramebufferInfo) {
        self.display = FramebufferDisplay::from_info(info);

        self.cursor_x = 0;
        self.cursor_y = 0;
        self.cursor_inverted = false;
        self.clear();
    }

    /// Toggle the visible cursor blink state.  Called from the timer tick path.
    pub fn blink_cursor(&mut self) {
        let Some((cols, rows)) = self.text_bounds() else {
            return;
        };
        if cols == 0 || rows == 0 {
            return;
        }

        let x = self.cursor_x.min(cols.saturating_sub(1));
        let y = self.cursor_y.min(rows.saturating_sub(1));
        let ch = self.screen[y][x];

        self.cursor_inverted = !self.cursor_inverted;
        if self.view_offset_lines == 0 {
            self.draw_cell_cursor(x, y, ch, self.cursor_inverted);
        }
    }

    /// Draw one character cell with optional cursor inversion.  This is a
    /// specialized variant of [`draw_cell`](Self::draw_cell) that swaps the
    /// foreground and background colors when `invert` is true.
    fn draw_cell_cursor(&mut self, cell_x: usize, cell_y: usize, c: char, invert: bool) {
        let (fg, bg) = if invert {
            (self.bg.to_u32(), self.fg.to_u32())
        } else {
            (self.fg.to_u32(), self.bg.to_u32())
        };

        // Reuse the fast 32-bit packed path from draw_cell by temporarily
        // swapping the console colors.  We restore them afterwards so later
        // output is unaffected.
        let saved_fg = self.fg;
        let saved_bg = self.bg;
        self.fg = Color {
            r: ((fg >> 16) & 0xFF) as u8,
            g: ((fg >> 8) & 0xFF) as u8,
            b: (fg & 0xFF) as u8,
        };
        self.bg = Color {
            r: ((bg >> 16) & 0xFF) as u8,
            g: ((bg >> 8) & 0xFF) as u8,
            b: (bg & 0xFF) as u8,
        };
        self.draw_cell(cell_x, cell_y, c);
        self.fg = saved_fg;
        self.bg = saved_bg;
    }

    /// Number of text columns that fit on the attached display, if any.
    pub fn text_columns(&self) -> Option<usize> {
        self.display
            .as_ref()
            .map(|display| display.width() / FONT_WIDTH)
    }

    /// Number of text rows that fit on the attached display, if any.
    pub fn text_rows(&self) -> Option<usize> {
        self.display
            .as_ref()
            .map(|display| display.height() / FONT_HEIGHT)
    }

    /// Number of lines currently stored in scrollback history.
    pub fn scrollback_lines(&self) -> usize {
        self.scrollback.len()
    }

    /// Current scroll-back view offset in lines (0 means live bottom).
    pub fn view_offset(&self) -> usize {
        self.view_offset_lines
    }

    /// Raw display geometry and pixel layout, if a framebuffer is attached.
    pub fn display_properties(&self) -> Option<DisplayProperties> {
        self.display.as_ref().map(|d| DisplayProperties {
            width: d.width(),
            height: d.height(),
            stride: d.stride(),
            bytes_per_pixel: d.bytes_per_pixel(),
            pixel_format: d.pixel_format(),
            framebuffer_size: d.framebuffer_size(),
        })
    }
}

/// Snapshot of the attached framebuffer geometry and pixel layout.
#[derive(Debug, Copy, Clone)]
pub struct DisplayProperties {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub bytes_per_pixel: usize,
    pub pixel_format: PixelFormat,
    pub framebuffer_size: usize,
}

impl ConsoleBackend for FramebufferConsole {
    /// Advance the visible cursor blink state on timer ticks.
    fn blink_cursor(&mut self) {
        FramebufferConsole::blink_cursor(self);
    }

    /// Write one character at the cursor, handling `\n`, `\r` and `\t`
    /// specially, advancing the cursor, and drawing the glyph immediately when
    /// viewing the live output.
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

    /// Clear the screen and character model and home the cursor.
    fn clear(&mut self) {
        self.clear_direct();
        if let Some((cols, rows)) = self.text_bounds() {
            self.clear_screen_model(cols, rows);
        }

        self.cursor_x = 0;
        self.cursor_y = 0;
        self.cursor_inverted = false;
        self.view_offset_lines = 0;
    }

    /// Move the text cursor to cell `(x, y)`.
    fn set_cursor(&mut self, x: usize, y: usize) {
        // Restore the previous cursor cell to normal before moving.
        if self.cursor_inverted {
            if self.view_offset_lines == 0 {
                let cols = self.text_columns().unwrap_or(0);
                let rows = self.text_rows().unwrap_or(0);
                if cols > 0 && rows > 0 {
                    let old_x = self.cursor_x.min(cols.saturating_sub(1));
                    let old_y = self.cursor_y.min(rows.saturating_sub(1));
                    let ch = self.screen[old_y][old_x];
                    self.draw_cell_cursor(old_x, old_y, ch, false);
                }
            }
            self.cursor_inverted = false;
        }

        self.cursor_x = x;
        self.cursor_y = y;
    }

    /// Scroll the console up by `rows` text rows: push the vanishing rows into
    /// scrollback, shift the character model, and update the framebuffer (fast
    /// in-place scroll when possible, otherwise a full repaint).
    fn scroll_up(&mut self, rows: usize) -> bool {
        let Some((cols, row_count)) = self.text_bounds() else {
            return false;
        };

        let rows = core::cmp::max(1, rows).min(row_count);
        for row in 0..rows {
            let vanished = self.row_to_scrollback(row, cols);
            self.push_scrollback_line(vanished);
        }

        self.screen[..row_count].copy_within(rows..row_count, 0);

        for y in row_count - rows..row_count {
            for x in 0..cols {
                self.screen[y][x] = ' ';
            }
        }

        if self.view_offset_lines == 0 {
            if !self.scroll_pixels_up(rows) {
                self.render_viewport();
            }
        } else {
            let max_offset = (self.scrollback.len() + row_count).saturating_sub(row_count);
            self.view_offset_lines = self.view_offset_lines.min(max_offset);
            self.render_viewport();
        }

        true
    }
}
