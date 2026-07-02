use super::font::{glyph_row, FONT_HEIGHT, FONT_WIDTH};
use super::framebuffer::Color;
use super::surface::Surface;

#[derive(Copy, Clone)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl AnsiColor {
    pub const fn to_color(self) -> Color {
        match self {
            AnsiColor::Black => Color { r: 0x00, g: 0x00, b: 0x00 },
            AnsiColor::Red => Color { r: 0xAA, g: 0x00, b: 0x00 },
            AnsiColor::Green => Color { r: 0x00, g: 0xAA, b: 0x00 },
            AnsiColor::Yellow => Color { r: 0xAA, g: 0x55, b: 0x00 },
            AnsiColor::Blue => Color { r: 0x00, g: 0x00, b: 0xAA },
            AnsiColor::Magenta => Color { r: 0xAA, g: 0x00, b: 0xAA },
            AnsiColor::Cyan => Color { r: 0x00, g: 0xAA, b: 0xAA },
            AnsiColor::White => Color { r: 0xAA, g: 0xAA, b: 0xAA },
            AnsiColor::BrightBlack => Color { r: 0x55, g: 0x55, b: 0x55 },
            AnsiColor::BrightRed => Color { r: 0xFF, g: 0x55, b: 0x55 },
            AnsiColor::BrightGreen => Color { r: 0x55, g: 0xFF, b: 0x55 },
            AnsiColor::BrightYellow => Color { r: 0xFF, g: 0xFF, b: 0x55 },
            AnsiColor::BrightBlue => Color { r: 0x55, g: 0x55, b: 0xFF },
            AnsiColor::BrightMagenta => Color { r: 0xFF, g: 0x55, b: 0xFF },
            AnsiColor::BrightCyan => Color { r: 0x55, g: 0xFF, b: 0xFF },
            AnsiColor::BrightWhite => Color { r: 0xFF, g: 0xFF, b: 0xFF },
        }
    }
}

pub struct Renderer<'a> {
    surface: &'a mut Surface,
}

impl<'a> Renderer<'a> {
    pub fn new(surface: &'a mut Surface) -> Self {
        Self { surface }
    }

    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
        ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }

    #[inline]
    pub const fn color(color: Color) -> u32 {
        color.to_u32()
    }

    pub fn draw_pixel(&mut self, x: usize, y: usize, color: u32) {
        self.surface.put_pixel(x, y, color);
    }

    pub fn draw_line(&mut self, x0: isize, y0: isize, x1: isize, y1: isize, color: u32) {
        self.surface.draw_line(x0, y0, x1, y1, color);
    }

    pub fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 {
            return;
        }

        self.draw_line(x as isize, y as isize, (x + w - 1) as isize, y as isize, color);
        self.draw_line(x as isize, (y + h - 1) as isize, (x + w - 1) as isize, (y + h - 1) as isize, color);
        self.draw_line(x as isize, y as isize, x as isize, (y + h - 1) as isize, color);
        self.draw_line((x + w - 1) as isize, y as isize, (x + w - 1) as isize, (y + h - 1) as isize, color);
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        self.surface.fill_rect(x, y, w, h, color);
    }

    pub fn draw_char(&mut self, x: usize, y: usize, ch: char, fg: u32, bg: u32) {
        for row_idx in 0..FONT_HEIGHT {
            let row_bits = glyph_row(ch, row_idx);
            for bit in 0..FONT_WIDTH {
                let mask = 1u8 << bit;
                let color = if (row_bits & mask) != 0 { fg } else { bg };
                self.surface.put_pixel(x + bit, y + row_idx, color);
            }
        }
    }

    pub fn draw_string(&mut self, x: usize, y: usize, text: &str, fg: u32, bg: u32) {
        let mut cx = x;
        for ch in text.chars() {
            self.draw_char(cx, y, ch, fg, bg);
            cx = cx.saturating_add(FONT_WIDTH);
        }
    }

    pub fn draw_char_ansi(&mut self, x: usize, y: usize, ch: char, fg: AnsiColor, bg: AnsiColor) {
        self.draw_char(x, y, ch, Self::color(fg.to_color()), Self::color(bg.to_color()));
    }

    pub fn draw_string_ansi(&mut self, x: usize, y: usize, text: &str, fg: AnsiColor, bg: AnsiColor) {
        self.draw_string(x, y, text, Self::color(fg.to_color()), Self::color(bg.to_color()));
    }

    pub fn draw_bitmap(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        pixels: &[u32],
    ) {
        for py in 0..height {
            for px in 0..width {
                let idx = py.saturating_mul(width).saturating_add(px);
                if idx < pixels.len() {
                    self.surface.put_pixel(x + px, y + py, pixels[idx]);
                }
            }
        }
    }
}
