use crate::graphics::contracts::Renderer;
use crate::graphics::{Image, Point};

pub fn blit<R: Renderer>(renderer: &mut R, image: &Image<'_>, target: Point) {
	renderer.draw_image(image, target);
}
