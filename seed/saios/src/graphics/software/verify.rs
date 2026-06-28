use crate::graphics::contracts::Renderer;
use crate::graphics::software::renderer::SoftwareRenderer;
use crate::graphics::{Color, Point, Rect, Surface};
use hal::display::DisplayHal;

fn draw_demo<R: Renderer>(renderer: &mut R) {
    renderer.clear(Color::rgb(12, 18, 38));
    renderer.draw_pixel(Point { x: 10, y: 10 }, Color::rgb(255, 255, 255));
    renderer.draw_line(
        Point { x: 24, y: 32 },
        Point { x: 240, y: 132 },
        Color::rgb(255, 80, 80),
    );
    renderer.draw_rect(
        Rect {
            x: 270,
            y: 42,
            width: 200,
            height: 120,
        },
        Color::rgb(70, 210, 140),
    );
    renderer.draw_circle(Point { x: 180, y: 260 }, 72, Color::rgb(246, 220, 68));
}

pub fn verify_primitives(surface: &mut Surface<'_>) {
    let mut renderer = SoftwareRenderer::from_surface(surface);
    draw_demo(&mut renderer);
}

pub fn verify_primitives_on_display<D: DisplayHal>(display: &mut D) {
    let mut renderer = SoftwareRenderer::from_display(display);
    draw_demo(&mut renderer);
    display.present();
}
