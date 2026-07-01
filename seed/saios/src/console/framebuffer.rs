use super::backend::ConsoleBackend;
use efi_main::graphics::FramebufferInfo;
use crate::graphics::display::{Display, FramebufferDisplay};
use crate::graphics::framebuffer::Color;
use crate::graphics::font::{FONT_HEIGHT, FONT_WIDTH};
use crate::graphics::renderer::Renderer;
use crate::graphics::surface::Surface;

pub struct FramebufferConsole {
    display: Option<FramebufferDisplay>,
    surface: Option<Surface>,
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
            cursor_x: 0,
            cursor_y: 0,
            fg: Color::WHITE,
            bg: Color::BLACK,
        }
    }

    pub fn attach(&mut self, info: FramebufferInfo) {
        self.display = FramebufferDisplay::from_info(info);
        self.surface = self
            .display
            .as_ref()
            .map(|display| Surface::new(display.width(), display.height()));
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

        if let Some(surface) = self.surface.as_mut() {
            let px = self.cursor_x * FONT_WIDTH;
            let py = self.cursor_y * FONT_HEIGHT;
            let mut renderer = Renderer::new(surface);
            renderer.draw_char(px, py, c, self.fg.to_u32(), self.bg.to_u32());
            self.flush();
            self.cursor_x += 1;
        }
    }

    fn clear(&mut self) {
        if let Some(surface) = self.surface.as_mut() {
            surface.clear(self.bg.to_u32());
            self.flush();
        }
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = x;
        self.cursor_y = y;
    }
}
