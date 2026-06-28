use crate::graphics::contracts::Renderer;
use crate::graphics::fonts::bitmap::BitmapFont;
use crate::graphics::fonts::psf::PsfFont;
use crate::graphics::{Color, Image, Point, Rect, Size, Surface as GraphicsSurface};
use hal::display::{Color as HalColor, DisplayHal, Point as HalPoint};

pub struct SoftwareRenderer<T> {
    pub target: T,
}

trait PixelTarget {
    fn size(&self) -> Size;
    fn clear(&mut self, color: Color);
    fn put_pixel(&mut self, point: Point, color: Color);
}

impl<'a, 'fb> PixelTarget for &'a mut GraphicsSurface<'fb> {
    fn size(&self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn clear(&mut self, color: Color) {
        (*self).clear(color);
    }

    fn put_pixel(&mut self, point: Point, color: Color) {
        if point.x < 0 || point.y < 0 || (point.x as u32) >= self.width || (point.y as u32) >= self.height {
            return;
        }

        let bpp = (self.bpp as usize) / 8;
        if bpp < 3 {
            return;
        }

        let offset = (point.y as usize * self.stride + point.x as usize) * bpp;
        if offset + bpp > self.pixels.len() {
            return;
        }

        self.pixels[offset] = color.r;
        self.pixels[offset + 1] = color.g;
        self.pixels[offset + 2] = color.b;
        if bpp > 3 {
            self.pixels[offset + 3] = color.a;
        }
    }
}

impl<D: DisplayHal> PixelTarget for &mut D {
    fn size(&self) -> Size {
        Size {
            width: self.width(),
            height: self.height(),
        }
    }

    fn clear(&mut self, color: Color) {
        DisplayHal::clear(*self, HalColor::rgba(color.r, color.g, color.b, color.a));
    }

    fn put_pixel(&mut self, point: Point, color: Color) {
        DisplayHal::put_pixel(
            *self,
            HalPoint {
                x: point.x,
                y: point.y,
            },
            HalColor::rgba(color.r, color.g, color.b, color.a),
        );
    }
}

impl<'a, 'fb> SoftwareRenderer<&'a mut GraphicsSurface<'fb>> {
    pub fn from_surface(surface: &'a mut GraphicsSurface<'fb>) -> Self {
        Self { target: surface }
    }
}

impl<'a, D: DisplayHal> SoftwareRenderer<&'a mut D> {
    pub fn from_display(display: &'a mut D) -> Self {
        Self { target: display }
    }
}

impl<T: PixelTarget> Renderer for SoftwareRenderer<T> {
    fn size(&self) -> Size {
        self.target.size()
    }

    fn clear(&mut self, color: Color) {
        self.target.clear(color);
    }

    fn draw_pixel(&mut self, point: Point, color: Color) {
        self.target.put_pixel(point, color);
    }

    fn draw_line(&mut self, start: Point, end: Point, color: Color) {
        let mut x0 = start.x;
        let mut y0 = start.y;
        let x1 = end.x;
        let y1 = end.y;

        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.draw_pixel(Point { x: x0, y: y0 }, color);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = err * 2;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn draw_rect(&mut self, rect: Rect, color: Color) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }

        let x2 = rect.x + rect.width as i32 - 1;
        let y2 = rect.y + rect.height as i32 - 1;
        self.draw_line(Point { x: rect.x, y: rect.y }, Point { x: x2, y: rect.y }, color);
        self.draw_line(Point { x: rect.x, y: y2 }, Point { x: x2, y: y2 }, color);
        self.draw_line(Point { x: rect.x, y: rect.y }, Point { x: rect.x, y: y2 }, color);
        self.draw_line(Point { x: x2, y: rect.y }, Point { x: x2, y: y2 }, color);
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }

        for row in 0..rect.height as i32 {
            self.draw_line(
                Point { x: rect.x, y: rect.y + row },
                Point {
                    x: rect.x + rect.width as i32 - 1,
                    y: rect.y + row,
                },
                color,
            );
        }
    }

    fn draw_circle(&mut self, center: Point, radius: u32, color: Color) {
        let mut x = radius as i32;
        let mut y = 0i32;
        let mut err = 1 - x;

        while x >= y {
            self.draw_pixel(Point { x: center.x + x, y: center.y + y }, color);
            self.draw_pixel(Point { x: center.x + y, y: center.y + x }, color);
            self.draw_pixel(Point { x: center.x - y, y: center.y + x }, color);
            self.draw_pixel(Point { x: center.x - x, y: center.y + y }, color);
            self.draw_pixel(Point { x: center.x - x, y: center.y - y }, color);
            self.draw_pixel(Point { x: center.x - y, y: center.y - x }, color);
            self.draw_pixel(Point { x: center.x + y, y: center.y - x }, color);
            self.draw_pixel(Point { x: center.x + x, y: center.y - y }, color);

            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }

    fn draw_image(&mut self, image: &Image, point: Point) {
        let src_bpp = (image.bpp as usize) / 8;
        if src_bpp < 3 {
            return;
        }

        for y in 0..image.height as usize {
            for x in 0..image.width as usize {
                let idx = (y * image.width as usize + x) * src_bpp;
                if idx + src_bpp > image.pixels.len() {
                    continue;
                }
                let src = &image.pixels[idx..idx + src_bpp];
                let color = if src_bpp > 3 {
                    Color::rgba(src[0], src[1], src[2], src[3])
                } else {
                    Color::rgb(src[0], src[1], src[2])
                };
                self.draw_pixel(Point { x: point.x + x as i32, y: point.y + y as i32 }, color);
            }
        }
    }

    fn draw_bitmap(
        &mut self,
        point: Point,
        width: u32,
        height: u32,
        row_stride: usize,
        bitmap: &[u8],
        color: Color,
    ) {
        for y in 0..height as usize {
            for x in 0..width as usize {
                let byte_index = y * row_stride + (x / 8);
                if byte_index >= bitmap.len() {
                    continue;
                }
                let bit = 7 - (x % 8);
                if ((bitmap[byte_index] >> bit) & 1) != 0 {
                    self.draw_pixel(Point { x: point.x + x as i32, y: point.y + y as i32 }, color);
                }
            }
        }
    }

    fn draw_text_psf(&mut self, font: &PsfFont<'_>, origin: Point, text: &str, color: Color) {
        let glyph_w = font.glyph_width();
        let glyph_h = font.glyph_height();
        let row_stride = glyph_w.div_ceil(8) as usize;
        let mut pen_x = origin.x;
        let mut pen_y = origin.y;

        for ch in text.chars() {
            if ch == '\n' {
                pen_x = origin.x;
                pen_y += glyph_h as i32;
                continue;
            }

            if let Some(glyph) = font.glyph_for_char(ch) {
                self.draw_bitmap(
                    Point { x: pen_x, y: pen_y },
                    glyph_w,
                    glyph_h,
                    row_stride,
                    glyph,
                    color,
                );
            }

            pen_x += glyph_w as i32;
        }
    }

    fn draw_text(&mut self, origin: Point, text: &str, font: &BitmapFont, color: Color) {
        let glyph_w = font.width as i32;
        let glyph_h = font.height as i32;
        let mut pen_x = origin.x;
        let mut pen_y = origin.y;

        for ch in text.chars() {
            if ch == '\n' {
                pen_x = origin.x;
                pen_y += glyph_h + 1;
                continue;
            }

            let rows = font.glyph_rows(ch);
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..font.width as usize {
                    let bit = 4usize.saturating_sub(col);
                    if ((bits >> bit) & 1) != 0 {
                        self.draw_pixel(
                            Point {
                                x: pen_x + col as i32,
                                y: pen_y + row as i32,
                            },
                            color,
                        );
                    }
                }
            }

            pen_x += glyph_w + 1;
        }
    }
}
