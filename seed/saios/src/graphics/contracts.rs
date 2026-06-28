use crate::graphics::{Color, Image, Point, Rect, Surface};

pub trait Renderer {
    fn clear(&mut self, color: Color);
    fn draw_pixel(&mut self, point: Point, color: Color);
    fn draw_rect(&mut self, rect: Rect, color: Color);
    fn draw_image(&mut self, image: &Image, point: Point);
}

pub trait Compositor {
    fn compose(&mut self, target: &mut Surface<'_>);
}
