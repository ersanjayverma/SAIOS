use crate::graphics::contracts::Renderer;
use crate::graphics::{Color, Rect};

pub fn draw_rect<R: Renderer>(renderer: &mut R, rect: Rect, color: Color) {
	renderer.draw_rect(rect, color);
}

pub fn fill_rect<R: Renderer>(renderer: &mut R, rect: Rect, color: Color) {
	renderer.fill_rect(rect, color);
}
