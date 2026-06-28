use efi_main::graphics::{FramebufferInfo, PixelFormat};

use crate::graphics::Color;
use crate::graphics::fonts::bitmap::BitmapFont;

const FG: Color = Color::rgb(235, 240, 255);
const BG: Color = Color::rgb(14, 60, 128);

pub struct FramebufferSink {
    fb: FramebufferInfo,
    font: BitmapFont,
    col: usize,
    row: usize,
    cols: usize,
    rows: usize,
}

impl FramebufferSink {
    pub fn new(fb: FramebufferInfo) -> Self {
        let font = BitmapFont::new_5x7();
        let cols = fb.width / (font.width as usize + 1);
        let rows = fb.height / (font.height as usize + 1);

        let mut sink = Self {
            fb,
            font,
            col: 0,
            row: 0,
            cols,
            rows,
        };
        sink.clear();
        sink
    }

    pub fn clear(&mut self) {
        let packed = pack_pixel(BG, self.fb.pixel_format);
        let base = self.fb.base as *mut u32;
        unsafe {
            for y in 0..self.fb.height {
                for x in 0..self.fb.width {
                    base.add(y * self.fb.stride + x).write_volatile(packed);
                }
            }
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for ch in s.chars() {
            if ch == '\n' {
                self.new_line();
                continue;
            }

            self.draw_char(ch);
            self.col += 1;
            if self.col >= self.cols {
                self.new_line();
            }
        }
    }

    fn new_line(&mut self) {
        self.col = 0;
        self.row += 1;
        if self.row >= self.rows {
            self.clear();
            self.row = 0;
        }
    }

    fn draw_char(&mut self, ch: char) {
        let origin_x = self.col * (self.font.width as usize + 1);
        let origin_y = self.row * (self.font.height as usize + 1);
        let rows = self.font.glyph_rows(ch);

        for (row, bits) in rows.iter().enumerate() {
            for col in 0..self.font.width as usize {
                let bit = 4usize.saturating_sub(col);
                if ((bits >> bit) & 1) != 0 {
                    self.put_pixel(origin_x + col, origin_y + row, FG);
                }
            }
        }
    }

    fn put_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.fb.width || y >= self.fb.height {
            return;
        }

        let packed = pack_pixel(color, self.fb.pixel_format);
        let base = self.fb.base as *mut u32;
        unsafe {
            base.add(y * self.fb.stride + x).write_volatile(packed);
        }
    }
}

fn pack_pixel(color: Color, pixel_format: PixelFormat) -> u32 {
    match pixel_format {
        PixelFormat::Bgr => ((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32,
        _ => ((color.b as u32) << 16) | ((color.g as u32) << 8) | color.r as u32,
    }
}
