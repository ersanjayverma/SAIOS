use super::FramebufferBenchResult;
use super::backend::ConsoleBackend;
use crate::graphics::display::{Display, FramebufferDisplay};
use crate::graphics::font::{FONT_HEIGHT, FONT_WIDTH, glyph_bitmap};
use crate::graphics::framebuffer::Color;
use crate::timer;
use alloc::collections::VecDeque;
use alloc::string::String;
use core::ptr;
use efi_main::graphics::FramebufferInfo;
use efi_main::graphics::PixelFormat;

const TAB_WIDTH: usize = 4;
const MAX_TEXT_COLS: usize = 160;
const MAX_TEXT_ROWS: usize = 100;
const MAX_SCROLLBACK_LINES: usize = 2048;
const GLYPH_CACHE_SIZE: usize = 64;

#[derive(Copy, Clone)]
struct GlyphCacheEntry {
    valid: bool,
    ch: char,
    fg: u32,
    bg: u32,
    pixel_format: PixelFormat,
    pixels: [u32; FONT_WIDTH * FONT_HEIGHT],
}

impl GlyphCacheEntry {
    const fn empty() -> Self {
        Self {
            valid: false,
            ch: '\0',
            fg: 0,
            bg: 0,
            pixel_format: PixelFormat::Bgr,
            pixels: [0; FONT_WIDTH * FONT_HEIGHT],
        }
    }
}

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
    dirty: [[bool; MAX_TEXT_COLS]; MAX_TEXT_ROWS],
    dirty_any: bool,
    dirty_min_x: usize,
    dirty_min_y: usize,
    dirty_max_x: usize,
    dirty_max_y: usize,
    batch_depth: usize,
    glyph_cache: [GlyphCacheEntry; GLYPH_CACHE_SIZE],
    view_offset_lines: usize,
    scrollback: VecDeque<String>,
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
            dirty: [[false; MAX_TEXT_COLS]; MAX_TEXT_ROWS],
            dirty_any: false,
            dirty_min_x: 0,
            dirty_min_y: 0,
            dirty_max_x: 0,
            dirty_max_y: 0,
            batch_depth: 0,
            glyph_cache: [GlyphCacheEntry::empty(); GLYPH_CACHE_SIZE],
            view_offset_lines: 0,
            scrollback: VecDeque::new(),
            cursor_inverted: false,
        }
    }

    fn glyph_cache_slot(c: char, fg: u32, bg: u32, pixel_format: PixelFormat) -> usize {
        let pf = match pixel_format {
            PixelFormat::Bgr => 0usize,
            PixelFormat::Rgb => 1usize,
            PixelFormat::Bitmask => 2usize,
            PixelFormat::BltOnly => 3usize,
        };

        let mut h = c as usize;
        h ^= fg as usize;
        h = h.rotate_left(13) ^ (bg as usize).rotate_right(7);
        h ^= pf.wrapping_mul(0x9E37_79B1usize);
        h % GLYPH_CACHE_SIZE
    }

    fn mark_dirty(&mut self, x: usize, y: usize) {
        if x >= MAX_TEXT_COLS || y >= MAX_TEXT_ROWS {
            return;
        }

        if !self.dirty[y][x] {
            self.dirty[y][x] = true;
            if !self.dirty_any {
                self.dirty_any = true;
                self.dirty_min_x = x;
                self.dirty_max_x = x;
                self.dirty_min_y = y;
                self.dirty_max_y = y;
            } else {
                self.dirty_min_x = self.dirty_min_x.min(x);
                self.dirty_max_x = self.dirty_max_x.max(x);
                self.dirty_min_y = self.dirty_min_y.min(y);
                self.dirty_max_y = self.dirty_max_y.max(y);
            }
        }
    }

    fn mark_region_dirty(&mut self, cols: usize, rows: usize) {
        for y in 0..rows.min(MAX_TEXT_ROWS) {
            for x in 0..cols.min(MAX_TEXT_COLS) {
                self.mark_dirty(x, y);
            }
        }
    }

    fn flush_dirty(&mut self) {
        let Some((cols, rows)) = self.text_bounds() else {
            self.dirty_any = false;
            return;
        };

        if self.batch_depth != 0 || !self.dirty_any {
            return;
        }

        if self.view_offset_lines > 0 {
            self.render_viewport();
            for y in self.dirty_min_y..=self.dirty_max_y {
                for x in self.dirty_min_x..=self.dirty_max_x {
                    self.dirty[y][x] = false;
                }
            }
            self.dirty_any = false;
            return;
        }

        let min_y = self.dirty_min_y.min(rows.saturating_sub(1));
        let max_y = self.dirty_max_y.min(rows.saturating_sub(1));
        let min_x = self.dirty_min_x.min(cols.saturating_sub(1));
        let max_x = self.dirty_max_x.min(cols.saturating_sub(1));

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                if self.dirty[y][x] {
                    self.draw_cell(x, y, self.screen[y][x]);
                    self.dirty[y][x] = false;
                }
            }
        }

        self.dirty_any = false;
    }

    fn begin_batch(&mut self) {
        self.batch_depth = self.batch_depth.saturating_add(1);
    }

    fn end_batch(&mut self) {
        if self.batch_depth > 0 {
            self.batch_depth -= 1;
        }
        self.flush_dirty();
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
        let (width, height, stride, pixel_format, pixel_masks, bytes_per_pixel, fb_size, fb) = {
            let Some(display) = self.display.as_mut() else {
                return;
            };

            (
                display.width(),
                display.height(),
                display.stride(),
                display.pixel_format(),
                display.pixel_masks(),
                display.bytes_per_pixel(),
                display.framebuffer_size(),
                display.framebuffer(),
            )
        };

        let px = cell_x * FONT_WIDTH;
        let py = cell_y * FONT_HEIGHT;

        let fg = self.fg.to_u32();
        let bg = self.bg.to_u32();
        let glyph = glyph_bitmap(c);

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
            let slot = Self::glyph_cache_slot(c, fg_packed, bg_packed, pixel_format);
            let cached = &mut self.glyph_cache[slot];
            if !cached.valid
                || cached.ch != c
                || cached.fg != fg_packed
                || cached.bg != bg_packed
                || cached.pixel_format != pixel_format
            {
                for row_idx in 0..FONT_HEIGHT {
                    let row_bits = glyph[row_idx / 2];
                    let row_base = row_idx * FONT_WIDTH;
                    for bit in 0..FONT_WIDTH {
                        cached.pixels[row_base + bit] = if (row_bits & (1u8 << bit)) != 0 {
                            fg_packed
                        } else {
                            bg_packed
                        };
                    }
                }
                cached.valid = true;
                cached.ch = c;
                cached.fg = fg_packed;
                cached.bg = bg_packed;
                cached.pixel_format = pixel_format;
            }

            // Fast path: view the framebuffer as a u32 slice and copy each
            // 8-pixel glyph row as a contiguous slice.  This avoids the
            // per-pixel branch and bounds checks of the generic path.
            let total_pixels = fb_size / 4;
            let fb32 = unsafe { core::slice::from_raw_parts_mut(fb.cast::<u32>(), total_pixels) };

            let draw_width = core::cmp::min(FONT_WIDTH, width.saturating_sub(px));
            let draw_height = core::cmp::min(FONT_HEIGHT, height.saturating_sub(py));

            for row_idx in 0..draw_height {
                let y = py + row_idx;
                let row_colors = &cached.pixels[row_idx * FONT_WIDTH..(row_idx + 1) * FONT_WIDTH];

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

            let row_bits = glyph[row_idx / 2];
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
        let cols = cols.min(MAX_TEXT_COLS);
        let mut end = 0;
        for x in (0..cols).rev() {
            if self.screen[row][x] != ' ' {
                end = x + 1;
                break;
            }
        }

        let mut out = String::with_capacity(end);
        for x in 0..end {
            out.push(self.screen[row][x]);
        }
        out
    }

    /// Append a line to scrollback, evicting the oldest lines once the history
    /// cap ([`MAX_SCROLLBACK_LINES`]) is exceeded.
    fn push_scrollback_line(&mut self, line: String) {
        self.scrollback.push_back(line);
        while self.scrollback.len() > MAX_SCROLLBACK_LINES {
            let _ = self.scrollback.pop_front();
        }
    }

    /// Reset the in-memory character grid to blanks (does not touch pixels).
    fn clear_screen_model(&mut self, cols: usize, rows: usize) {
        let cols = cols.min(MAX_TEXT_COLS);
        let rows = rows.min(MAX_TEXT_ROWS);
        for y in 0..rows {
            for x in 0..cols {
                self.screen[y][x] = ' ';
                self.dirty[y][x] = false;
            }
        }
        self.dirty_any = false;
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

    /// Draw one viewport row from the unified line index space
    /// (`scrollback` + current `screen` model).
    fn draw_view_line(&mut self, row: usize, line_idx: usize, cols: usize, rows: usize) {
        if line_idx < self.scrollback.len() {
            let mut row_chars = [' '; MAX_TEXT_COLS];
            for (x, ch) in self.scrollback[line_idx].chars().take(cols).enumerate() {
                row_chars[x] = ch;
            }
            for (x, ch) in row_chars.iter().take(cols).enumerate() {
                self.draw_cell(x, row, *ch);
            }
            return;
        }

        let screen_row = line_idx - self.scrollback.len();
        if screen_row < rows {
            for x in 0..cols {
                self.draw_cell(x, row, self.screen[screen_row][x]);
            }
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

    /// Scroll the visible framebuffer down by `text_rows` character rows using
    /// a single `memmove`, then clear the newly exposed rows at the top.
    fn scroll_pixels_down(&mut self, text_rows: usize) -> bool {
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

        if shift_px < height {
            let dst_offset = shift_px.saturating_mul(row_bytes);
            let copy_bytes = (height - shift_px).saturating_mul(row_bytes);
            if dst_offset.saturating_add(copy_bytes) > fb_size {
                return false;
            }

            unsafe {
                ptr::copy(fb, fb.add(dst_offset), copy_bytes);
            }
        }

        let clear_end = shift_px;

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

        if bytes_per_pixel == 4 {
            let total_pixels = fb_size / 4;
            let fb32 = unsafe { core::slice::from_raw_parts_mut(fb.cast::<u32>(), total_pixels) };
            for y in 0..clear_end {
                let row_base = y.saturating_mul(stride);
                if row_base >= total_pixels {
                    break;
                }
                let end = core::cmp::min(row_base + width, total_pixels);
                fb32[row_base..end].fill(bg_packed);
            }
            return true;
        }

        let bg_bytes = bg_packed.to_le_bytes();
        for y in 0..clear_end {
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
                        ptr::write(p.add(i), bg_bytes[i]);
                        i += 1;
                    }
                }
            }
        }

        true
    }

    /// Try to update the viewport incrementally by pixel-scrolling the current
    /// image and redrawing only the newly exposed text rows.
    fn scroll_viewport_incremental(&mut self, delta_lines: isize) -> bool {
        let Some((cols, rows)) = self.text_bounds() else {
            return false;
        };

        let delta = delta_lines.unsigned_abs();
        if delta == 0 {
            return true;
        }
        if delta >= rows {
            return false;
        }

        let shifted = if delta_lines > 0 {
            self.scroll_pixels_up(delta)
        } else {
            self.scroll_pixels_down(delta)
        };
        if !shifted {
            return false;
        }

        let total_lines = self.scrollback.len() + rows;
        let start = total_lines.saturating_sub(rows + self.view_offset_lines);

        if delta_lines > 0 {
            for row in rows - delta..rows {
                let line_idx = start + row;
                self.draw_view_line(row, line_idx, cols, rows);
            }
        } else {
            for row in 0..delta {
                let line_idx = start + row;
                self.draw_view_line(row, line_idx, cols, rows);
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
        let old_start = total_lines.saturating_sub(rows + before);

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
            let new_start = total_lines.saturating_sub(rows + self.view_offset_lines);
            let delta_lines = new_start as isize - old_start as isize;
            if !self.scroll_viewport_incremental(delta_lines) {
                self.render_viewport();
            }
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

    /// Bind the console directly to the bootloader-provided framebuffer address
    /// without creating a new VMM mapping. Used during firmware-CR3 fallback.
    pub fn attach_direct(&mut self, info: FramebufferInfo) {
        self.display = FramebufferDisplay::from_info_direct(info);

        self.cursor_x = 0;
        self.cursor_y = 0;
        self.cursor_inverted = false;
        self.clear();
    }

    /// Toggle the visible cursor blink state.  Called from the timer tick path.
    pub fn blink_cursor(&mut self) {
        // Prioritize throughput over cursor animation during heavy scrolling.
        let _ = self;
    }

    /// Draw one character cell with optional cursor overlay.
    ///
    /// When the cursor is visible, render an underscore glyph in the current
    /// foreground color. When hidden, restore the underlying character.
    fn draw_cell_cursor(&mut self, cell_x: usize, cell_y: usize, c: char, invert: bool) {
        self.draw_cell(cell_x, cell_y, if invert { '_' } else { c });
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

    /// Benchmark full-screen clear throughput using the current display path.
    pub fn benchmark_clears(&mut self, passes: usize) -> Option<FramebufferBenchResult> {
        let display = self.display.as_mut()?;
        let passes = core::cmp::max(1, passes);

        let bytes_per_clear = display.framebuffer_size();
        let start_ticks = timer::ticks();
        for i in 0..passes {
            let color = if (i & 1) == 0 {
                0x0000_0000
            } else {
                0x00FF_FFFF
            };
            display.clear_color(color);
        }
        let end_ticks = timer::ticks();

        let elapsed_ticks = end_ticks.saturating_sub(start_ticks);
        let elapsed_ms = (elapsed_ticks * 1000) / 100;
        let bytes_written = bytes_per_clear.saturating_mul(passes);

        let mib_per_sec = if elapsed_ms == 0 {
            0
        } else {
            ((bytes_written as u128) * 1000 / (elapsed_ms as u128) / (1024 * 1024) as u128) as u64
        };

        Some(FramebufferBenchResult {
            passes,
            bytes_written,
            elapsed_ticks,
            elapsed_ms,
            mib_per_sec,
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
                if self.screen[y][x] != c {
                    self.screen[y][x] = c;
                    self.mark_dirty(x, y);
                }
                self.cursor_x = self.cursor_x.saturating_add(1).min(cols.saturating_sub(1));
            }
        }

        self.flush_dirty();
    }

    /// Write a whole string with a single cursor sync after all characters are applied.
    fn put_str(&mut self, s: &str) {
        let Some((_, _)) = self.text_bounds() else {
            return;
        };

        self.begin_batch();
        for c in s.chars() {
            self.put_char(c);
        }
        self.end_batch();
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
        self.dirty_any = false;
        self.batch_depth = 0;
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
            self.screen[y][..cols].fill(' ');
        }

        if self.view_offset_lines == 0 {
            // Throughput-first: avoid framebuffer-to-framebuffer memmove here.
            // On many platforms the framebuffer is uncached, and readback during
            // memmove is slower than redrawing visible glyphs from the text model.
            self.mark_region_dirty(cols, row_count);
            self.flush_dirty();
        } else {
            let max_offset = (self.scrollback.len() + row_count).saturating_sub(row_count);
            self.view_offset_lines = self.view_offset_lines.min(max_offset);
            self.render_viewport();
        }

        true
    }
}
