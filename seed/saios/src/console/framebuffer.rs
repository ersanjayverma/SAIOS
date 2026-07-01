use super::backend::ConsoleBackend;
use super::font::{FONT_HEIGHT, FONT_WIDTH};
use super::glyph::draw_glyph;
use efi_main::graphics::{FramebufferInfo, PixelFormat};

#[derive(Copy, Clone)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
    };
}

pub struct Framebuffer {
    base: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    pixel_format: PixelFormat,
}

impl Framebuffer {
    pub fn from_info(info: FramebufferInfo) -> Option<Self> {
        if info.base == 0 || info.width == 0 || info.height == 0 || info.bpp != 32 {
            return None;
        }

        Some(Self {
            base: info.base as *mut u8,
            width: info.width,
            height: info.height,
            stride: info.stride,
            pixel_format: info.pixel_format,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn clear(&mut self, color: Color) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.put_pixel(x, y, color);
            }
        }
    }

    pub fn put_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.width || y >= self.height {
            return;
        }

        let offset = (y * self.stride + x) * 4;
        unsafe {
            let p = self.base.add(offset);
            match self.pixel_format {
                PixelFormat::Rgb => {
                    *p = color.r;
                    *p.add(1) = color.g;
                    *p.add(2) = color.b;
                    *p.add(3) = 0;
                }
                PixelFormat::Bgr | PixelFormat::Bitmask => {
                    *p = color.b;
                    *p.add(1) = color.g;
                    *p.add(2) = color.r;
                    *p.add(3) = 0;
                }
                PixelFormat::BltOnly => {}
            }
        }
    }
}

pub struct FramebufferConsole {
    fb: Option<Framebuffer>,
    cursor_x: usize,
    cursor_y: usize,
    fg: Color,
    bg: Color,
}

impl FramebufferConsole {
    pub const fn new() -> Self {
        Self {
            fb: None,
            cursor_x: 0,
            cursor_y: 0,
            fg: Color::WHITE,
            bg: Color::BLACK,
        }
    }

    pub fn attach(&mut self, info: FramebufferInfo) {
        self.fb = Framebuffer::from_info(info);
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    pub fn text_columns(&self) -> Option<usize> {
        self.fb.as_ref().map(|fb| fb.width() / FONT_WIDTH)
    }

    pub fn text_rows(&self) -> Option<usize> {
        self.fb.as_ref().map(|fb| fb.height() / FONT_HEIGHT)
    }
}

impl ConsoleBackend for FramebufferConsole {
    fn put_char(&mut self, c: char) {
        if c == '\n' || c == '\r' || c == '\t' {
            return;
        }

        if let Some(fb) = self.fb.as_mut() {
            let px = self.cursor_x * FONT_WIDTH;
            let py = self.cursor_y * FONT_HEIGHT;
            draw_glyph(fb, px, py, c, self.fg, self.bg);
            self.cursor_x += 1;
        }
    }

    fn clear(&mut self) {
        if let Some(fb) = self.fb.as_mut() {
            fb.clear(self.bg);
        }
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = x;
        self.cursor_y = y;
    }
}
