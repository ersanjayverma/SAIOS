use crate::graphics::contracts::Renderer;
use crate::graphics::{Color, Point};

pub fn draw_circle<R: Renderer>(renderer: &mut R, center: Point, radius: u32, color: Color) {
	renderer.draw_circle(center, radius, color);
}
