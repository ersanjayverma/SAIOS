use crate::graphics::{Color, Point, Rect, Surface};

pub struct SoftwareRenderer<'a> {
    pub target: Surface<'a>,
}

impl SoftwareRenderer<'_> {
    pub fn clear(&mut self, color: Color) {
        self.target.clear(color);
    }

    pub fn draw_pixel(&mut self, _point: Point, _color: Color) {}
    pub fn draw_rect(&mut self, _rect: Rect, _color: Color) {}
}
