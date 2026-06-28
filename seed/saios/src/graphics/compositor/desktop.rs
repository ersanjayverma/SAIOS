use crate::graphics::compositor::window_manager::{MAX_WINDOWS, WindowManager};
use crate::graphics::contracts::{Compositor, Renderer};
use crate::graphics::software::renderer::SoftwareRenderer;
use crate::graphics::{Color, Point, Rect, Surface};

pub struct DesktopCompositor {
	pub background: Color,
	pub window_manager: WindowManager,
}

impl DesktopCompositor {
	pub const fn new(background: Color) -> Self {
		Self {
			background,
			window_manager: WindowManager::new(),
		}
	}

	pub fn compose_with_renderer<R: Renderer>(&self, renderer: &mut R) {
		renderer.clear(self.background);

		let windows = self.window_manager.windows_sorted_by_z();
		let mut i = 0;
		while i < MAX_WINDOWS {
			if let Some(win) = windows[i] {
				if win.visible {
					renderer.fill_rect(win.frame, win.bg);
					renderer.draw_rect(win.frame, win.border);

					let title_bar = Rect {
						x: win.frame.x,
						y: win.frame.y,
						width: win.frame.width,
						height: 20,
					};
					renderer.fill_rect(title_bar, Color::rgb(26, 30, 54));

					let _title = win.title;
					renderer.draw_pixel(
						Point {
							x: win.frame.x + 6,
							y: win.frame.y + 10,
						},
						Color::rgb(255, 255, 255),
					);
				}
			}
			i += 1;
		}
	}
}

impl Compositor for DesktopCompositor {
	fn compose(&mut self, target: &mut Surface<'_>) {
		let mut renderer = SoftwareRenderer::from_surface(target);
		self.compose_with_renderer(&mut renderer);
	}
}
