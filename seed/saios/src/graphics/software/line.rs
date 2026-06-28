use crate::graphics::contracts::Renderer;
use crate::graphics::{Color, Point};

pub fn draw_line<R: Renderer>(renderer: &mut R, from: Point, to: Point, color: Color) {
	renderer.draw_line(from, to, color);
}
