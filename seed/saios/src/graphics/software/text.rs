use crate::graphics::contracts::Renderer;
use crate::graphics::fonts::bitmap::BitmapFont;
use crate::graphics::fonts::psf::PsfFont;
use crate::graphics::{Color, Point};

pub fn draw_text<R: Renderer>(
	renderer: &mut R,
	origin: Point,
	text: &str,
	font: &BitmapFont,
	color: Color,
) {
	renderer.draw_text(origin, text, font, color);
}

pub fn draw_text_psf<R: Renderer>(
	renderer: &mut R,
	font: &PsfFont<'_>,
	origin: Point,
	text: &str,
	color: Color,
) {
	renderer.draw_text_psf(font, origin, text, color);
}
