use super::backend::ConsoleBackend;
use efi_main::graphics::FramebufferInfo;
use crate::graphics::display::{Display, FramebufferDisplay};
use crate::graphics::framebuffer::Color;
use crate::graphics::font::{glyph_row, FONT_HEIGHT, FONT_WIDTH};
use crate::graphics::renderer::Renderer;
use crate::graphics::surface::Surface;
use hal::arch::x86_64::sync::StaticCell;

const BACKBUFFER_WIDTH: usize = 1024;
const BACKBUFFER_HEIGHT: usize = 768;
const BACKBUFFER_CAPACITY: usize = BACKBUFFER_WIDTH * BACKBUFFER_HEIGHT;

static BACKBUFFER_PIXELS: StaticCell<[u32; BACKBUFFER_CAPACITY]> =
    StaticCell::new([0; BACKBUFFER_CAPACITY]);

pub struct FramebufferConsole {
    display: Option<FramebufferDisplay>,
    surface: Option<Surface>,
    renderer_ready: bool,
    cursor_x: usize,
    cursor_y: usize,
    fg: Color,
    bg: Color,
}

impl FramebufferConsole {
    pub const fn new() -> Self {
        Self {
            display: None,
            surface: None,
            renderer_ready: false,
            cursor_x: 0,
            cursor_y: 0,
            fg: Color::WHITE,
            bg: Color::BLACK,
        }
    }

    fn try_init_surface(&mut self) {
        if self.renderer_ready {
            return;
        }

        if let Some(display) = self.display.as_ref() {
            let width = display.width();
            let height = display.height();
            let needed = width.saturating_mul(height);

            if needed <= BACKBUFFER_CAPACITY {
                // SAFETY: single-console singleton in early kernel context.
                let pixels = unsafe { &mut *BACKBUFFER_PIXELS.get() };
                self.surface = Surface::new_borrowed(width, height, &mut pixels[..needed]);
            }

            if self.surface.is_none() {
                self.surface = Surface::try_new(width, height);
            }

            self.renderer_ready = self.surface.is_some();
        }
    }

    pub fn ensure_renderer_ready(&mut self) -> bool {
        self.try_init_surface();
        self.renderer_ready
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

        self.surface = None;
        self.renderer_ready = false;
        self.try_init_surface();

        self.cursor_x = 0;
        self.cursor_y = 0;
        self.clear();
    }

    fn flush(&mut self) {
        if let (Some(display), Some(surface)) = (self.display.as_mut(), self.surface.as_ref()) {
            display.flush(surface.pixels(), surface.width(), surface.height());
        }
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
        if c == '\n' || c == '\r' || c == '\t' {
            return;
        }

        if !self.renderer_ready {
            self.try_init_surface();
        }

        if let Some(surface) = self.surface.as_mut() {
            let px = self.cursor_x * FONT_WIDTH;
            let py = self.cursor_y * FONT_HEIGHT;
            let mut renderer = Renderer::new(surface);
            renderer.draw_char(px, py, c, self.fg.to_u32(), self.bg.to_u32());
            self.flush();
            self.cursor_x += 1;
        } else {
            self.put_char_direct(c);
        }
    }

    fn clear(&mut self) {
        if !self.renderer_ready {
            self.try_init_surface();
        }

        if let Some(surface) = self.surface.as_mut() {
            surface.clear(self.bg.to_u32());
            self.flush();
        } else {
            self.clear_direct();
        }
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = x;
        self.cursor_y = y;
    }
}
