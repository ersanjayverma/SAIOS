use crate::graphics::contracts::Renderer;
use crate::graphics::font::PsfFont;
use crate::graphics::{Color, Point};

pub fn draw_text_psf<R: Renderer>(
	renderer: &mut R,
	font: &PsfFont<'_>,
	origin: Point,
	text: &str,
	color: Color,
) {
	renderer.draw_text_psf(font, origin, text, color);
}
