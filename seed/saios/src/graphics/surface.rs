use alloc::vec;
use alloc::vec::Vec;

pub struct Surface {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl Surface {
    pub fn new(width: usize, height: usize) -> Self {
        let pixels = vec![0; width.saturating_mul(height)];
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        self.pixels.as_slice()
    }

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        self.pixels[y * self.width + x] = color;
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        let x_end = core::cmp::min(x.saturating_add(w), self.width);
        let y_end = core::cmp::min(y.saturating_add(h), self.height);

        for py in y..y_end {
            for px in x..x_end {
                self.put_pixel(px, py, color);
            }
        }
    }

    pub fn draw_line(&mut self, x0: isize, y0: isize, x1: isize, y1: isize, color: u32) {
        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;

        loop {
            if x >= 0 && y >= 0 {
                self.put_pixel(x as usize, y as usize, color);
            }

            if x == x1 && y == y1 {
                break;
            }

            let e2 = err.saturating_mul(2);
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    pub fn copy_region(
        &mut self,
        src_x: usize,
        src_y: usize,
        width: usize,
        height: usize,
        dst_x: usize,
        dst_y: usize,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        let mut temp: Vec<u32> = Vec::new();
        let _ = temp.try_reserve(width.saturating_mul(height));

        for y in 0..height {
            for x in 0..width {
                let sx = src_x.saturating_add(x);
                let sy = src_y.saturating_add(y);
                if sx < self.width && sy < self.height {
                    temp.push(self.pixels[sy * self.width + sx]);
                } else {
                    temp.push(0);
                }
            }
        }

        let mut idx = 0usize;
        for y in 0..height {
            for x in 0..width {
                let dx = dst_x.saturating_add(x);
                let dy = dst_y.saturating_add(y);
                if dx < self.width && dy < self.height {
                    self.pixels[dy * self.width + dx] = temp[idx];
                }
                idx += 1;
            }
        }
    }
}
