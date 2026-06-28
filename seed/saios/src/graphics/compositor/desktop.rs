use crate::graphics::compositor::window_manager::{MAX_WINDOWS, WindowManager};
use crate::graphics::contracts::{Compositor, Renderer};
use crate::graphics::fonts::bitmap::BitmapFont;
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

    pub fn seed_demo_windows(&mut self) {
        let _ = self.window_manager.create_window(
            "Terminal",
            Rect {
                x: 620,
                y: 220,
                width: 360,
                height: 260,
            },
            Color::rgb(24, 26, 38),
            Color::rgb(72, 84, 128),
        );
        let _ = self.window_manager.create_window(
            "Files",
            Rect {
                x: 120,
                y: 160,
                width: 420,
                height: 300,
            },
            Color::rgb(30, 38, 54),
            Color::rgb(90, 110, 148),
        );
    }

    pub fn compose_with_renderer<R: Renderer>(&self, renderer: &mut R) {
        renderer.clear(self.background);
        let font = BitmapFont::new_5x7();
        let size = renderer.size();

        renderer.draw_rect(
            Rect {
                x: 0,
                y: 0,
                width: size.width,
                height: size.height,
            },
            Color::rgb(38, 44, 72),
        );
        renderer.draw_text(
            Point { x: 14, y: 12 },
            "SAIOS",
            &font,
            Color::rgb(230, 236, 255),
        );

        let taskbar_h = 28u32;
        renderer.fill_rect(
            Rect {
                x: 0,
                y: size.height as i32 - taskbar_h as i32,
                width: size.width,
                height: taskbar_h,
            },
            Color::rgb(18, 22, 40),
        );
        renderer.draw_text(
            Point {
                x: 10,
                y: size.height as i32 - 20,
            },
            "Start  Terminal  Files  Clock 12:34",
            &font,
            Color::rgb(235, 240, 255),
        );

        let windows = self.window_manager.windows_sorted_by_z();
        let mut i = 0;
        while i < MAX_WINDOWS {
            if let Some(win) = windows[i] {
                if win.visible {
                    renderer.fill_rect(win.frame, win.surface.clear_color);
                    renderer.draw_rect(win.frame, win.border);

                    let title_bar = Rect {
                        x: win.frame.x,
                        y: win.frame.y,
                        width: win.frame.width,
                        height: 20,
                    };
                    renderer.fill_rect(title_bar, Color::rgb(26, 30, 54));

                    renderer.draw_text(
                        Point {
                            x: win.frame.x + 8,
                            y: win.frame.y + 6,
                        },
                        win.title,
                        &font,
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
