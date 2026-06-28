use crate::graphics::font::PsfFont;
use crate::graphics::{Color, Image, Point, Rect, Size, Surface};

pub trait Renderer {
    fn size(&self) -> Size;
    fn clear(&mut self, color: Color);
    fn draw_pixel(&mut self, point: Point, color: Color);
    fn draw_line(&mut self, start: Point, end: Point, color: Color);
    fn draw_rect(&mut self, rect: Rect, color: Color);
    fn fill_rect(&mut self, rect: Rect, color: Color);
    fn draw_circle(&mut self, center: Point, radius: u32, color: Color);
    fn draw_image(&mut self, image: &Image, point: Point);
    fn draw_bitmap(
        &mut self,
        point: Point,
        width: u32,
        height: u32,
        row_stride: usize,
        bitmap: &[u8],
        color: Color,
    );
    fn draw_text_psf(&mut self, font: &PsfFont<'_>, origin: Point, text: &str, color: Color);
}

pub trait Compositor {
    fn compose(&mut self, target: &mut Surface<'_>);
}
