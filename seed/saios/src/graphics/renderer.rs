use super::font::{FONT_HEIGHT, FONT_WIDTH, glyph_row};
use super::framebuffer::Color;
use super::surface::Surface;

/// The 16 standard ANSI terminal colors (8 normal + 8 bright).
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
    /// Map an ANSI color to its concrete RGB [`Color`] (VGA-style palette).
    pub const fn to_color(self) -> Color {
        match self {
            AnsiColor::Black => Color {
                r: 0x00,
                g: 0x00,
                b: 0x00,
            },
            AnsiColor::Red => Color {
                r: 0xAA,
                g: 0x00,
                b: 0x00,
            },
            AnsiColor::Green => Color {
                r: 0x00,
                g: 0xAA,
                b: 0x00,
            },
            AnsiColor::Yellow => Color {
                r: 0xAA,
                g: 0x55,
                b: 0x00,
            },
            AnsiColor::Blue => Color {
                r: 0x00,
                g: 0x00,
                b: 0xAA,
            },
            AnsiColor::Magenta => Color {
                r: 0xAA,
                g: 0x00,
                b: 0xAA,
            },
            AnsiColor::Cyan => Color {
                r: 0x00,
                g: 0xAA,
                b: 0xAA,
            },
            AnsiColor::White => Color {
                r: 0xAA,
                g: 0xAA,
                b: 0xAA,
            },
            AnsiColor::BrightBlack => Color {
                r: 0x55,
                g: 0x55,
                b: 0x55,
            },
            AnsiColor::BrightRed => Color {
                r: 0xFF,
                g: 0x55,
                b: 0x55,
            },
            AnsiColor::BrightGreen => Color {
                r: 0x55,
                g: 0xFF,
                b: 0x55,
            },
            AnsiColor::BrightYellow => Color {
                r: 0xFF,
                g: 0xFF,
                b: 0x55,
            },
            AnsiColor::BrightBlue => Color {
                r: 0x55,
                g: 0x55,
                b: 0xFF,
            },
            AnsiColor::BrightMagenta => Color {
                r: 0xFF,
                g: 0x55,
                b: 0xFF,
            },
            AnsiColor::BrightCyan => Color {
                r: 0x55,
                g: 0xFF,
                b: 0xFF,
            },
            AnsiColor::BrightWhite => Color {
                r: 0xFF,
                g: 0xFF,
                b: 0xFF,
            },
        }
    }
}

/// Stateless drawing helper that renders primitives (pixels, lines, rects,
/// text, bitmaps) into a borrowed [`Surface`].
pub struct Renderer<'a> {
    surface: &'a mut Surface,
}

impl<'a> Renderer<'a> {
    /// Wrap a mutable surface for drawing.
    pub fn new(surface: &'a mut Surface) -> Self {
        Self { surface }
    }

    /// Pack 8-bit red/green/blue components into a 0x00RRGGBB word.
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
        ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }

    /// Convert a [`Color`] into a packed 0x00RRGGBB word.
    #[inline]
    pub const fn color(color: Color) -> u32 {
        color.to_u32()
    }

    /// Draw a single pixel.
    pub fn draw_pixel(&mut self, x: usize, y: usize, color: u32) {
        self.surface.put_pixel(x, y, color);
    }

    /// Draw a line between two points.
    pub fn draw_line(&mut self, x0: isize, y0: isize, x1: isize, y1: isize, color: u32) {
        self.surface.draw_line(x0, y0, x1, y1, color);
    }

    /// Draw the outline of a rectangle.
    pub fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 {
            return;
        }

        self.draw_line(
            x as isize,
            y as isize,
            (x + w - 1) as isize,
            y as isize,
            color,
        );
        self.draw_line(
            x as isize,
            (y + h - 1) as isize,
            (x + w - 1) as isize,
            (y + h - 1) as isize,
            color,
        );
        self.draw_line(
            x as isize,
            y as isize,
            x as isize,
            (y + h - 1) as isize,
            color,
        );
        self.draw_line(
            (x + w - 1) as isize,
            y as isize,
            (x + w - 1) as isize,
            (y + h - 1) as isize,
            color,
        );
    }

    /// Fill a solid rectangle.
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        self.surface.fill_rect(x, y, w, h, color);
    }

    /// Draw one character glyph at pixel `(x, y)` with explicit foreground and
    /// background colors. Writes glyph rows as contiguous slices for speed.
    pub fn draw_char(&mut self, x: usize, y: usize, ch: char, fg: u32, bg: u32) {
        let surface_width = self.surface.width();
        let surface_height = self.surface.height();

        if x >= surface_width || y >= surface_height {
            return;
        }

        let draw_width = core::cmp::min(FONT_WIDTH, surface_width - x);
        let draw_height = core::cmp::min(FONT_HEIGHT, surface_height - y);
        let pixels = self.surface.pixels_mut();

        for row_idx in 0..draw_height {
            let row_bits = glyph_row(ch, row_idx);
            let row_start = (y + row_idx) * surface_width + x;
            let row = &mut pixels[row_start..row_start + draw_width];
            for (bit, px) in row.iter_mut().enumerate() {
                let mask = 1u8 << bit;
                *px = if (row_bits & mask) != 0 { fg } else { bg };
            }
        }
    }

    /// Draw a string left-to-right starting at pixel `(x, y)`.
    pub fn draw_string(&mut self, x: usize, y: usize, text: &str, fg: u32, bg: u32) {
        let mut cx = x;
        for ch in text.chars() {
            self.draw_char(cx, y, ch, fg, bg);
            cx = cx.saturating_add(FONT_WIDTH);
        }
    }

    /// Draw a character using ANSI palette colors.
    pub fn draw_char_ansi(&mut self, x: usize, y: usize, ch: char, fg: AnsiColor, bg: AnsiColor) {
        self.draw_char(
            x,
            y,
            ch,
            Self::color(fg.to_color()),
            Self::color(bg.to_color()),
        );
    }

    /// Draw a string using ANSI palette colors.
    pub fn draw_string_ansi(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        fg: AnsiColor,
        bg: AnsiColor,
    ) {
        self.draw_string(
            x,
            y,
            text,
            Self::color(fg.to_color()),
            Self::color(bg.to_color()),
        );
    }

    /// Blit a raw `width` x `height` bitmap of packed pixels at `(x, y)`,
    /// clipping to the surface and copying each row as a contiguous slice.
    pub fn draw_bitmap(&mut self, x: usize, y: usize, width: usize, height: usize, pixels: &[u32]) {
        let surface_width = self.surface.width();
        let surface_height = self.surface.height();

        if width == 0 || height == 0 || x >= surface_width || y >= surface_height {
            return;
        }

        let draw_width = core::cmp::min(width, surface_width - x);
        let draw_height = core::cmp::min(height, surface_height - y);
        let dst = self.surface.pixels_mut();

        for py in 0..draw_height {
            let src_row_start = py * width;
            if src_row_start >= pixels.len() {
                break;
            }

            let copy_width = core::cmp::min(draw_width, pixels.len() - src_row_start);
            let dst_row_start = (y + py) * surface_width + x;
            dst[dst_row_start..dst_row_start + copy_width]
                .copy_from_slice(&pixels[src_row_start..src_row_start + copy_width]);
        }
    }
}
