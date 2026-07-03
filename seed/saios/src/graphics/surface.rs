use alloc::vec::Vec;

enum PixelStorage {
    Owned(Vec<u32>),
    Borrowed(&'static mut [u32]),
}

pub struct Surface {
    width: usize,
    height: usize,
    pixels: PixelStorage,
}

impl Surface {
    pub fn try_new(width: usize, height: usize) -> Option<Self> {
        let len = width.saturating_mul(height);
        let mut pixels = Vec::new();
        if pixels.try_reserve_exact(len).is_err() {
            return None;
        }
        pixels.resize(len, 0);
        Some(Self {
            width,
            height,
            pixels: PixelStorage::Owned(pixels),
        })
    }

    pub fn new_borrowed(width: usize, height: usize, pixels: &'static mut [u32]) -> Option<Self> {
        let len = width.saturating_mul(height);
        if pixels.len() < len {
            return None;
        }

        for p in pixels.iter_mut().take(len) {
            *p = 0;
        }

        Some(Self {
            width,
            height,
            pixels: PixelStorage::Borrowed(pixels),
        })
    }

    fn pixels_slice(&self) -> &[u32] {
        match &self.pixels {
            PixelStorage::Owned(v) => v.as_slice(),
            PixelStorage::Borrowed(s) => s,
        }
    }

    fn pixels_slice_mut(&mut self) -> &mut [u32] {
        match &mut self.pixels {
            PixelStorage::Owned(v) => v.as_mut_slice(),
            PixelStorage::Borrowed(s) => s,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        self.pixels_slice()
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        self.pixels_slice_mut()
    }

    pub fn clear(&mut self, color: u32) {
        self.pixels_slice_mut().fill(color);
    }

    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let width = self.width;
        let idx = y.saturating_mul(width).saturating_add(x);
        self.pixels_slice_mut()[idx] = color;
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 {
            return;
        }

        let x_end = core::cmp::min(x.saturating_add(w), self.width);
        let y_end = core::cmp::min(y.saturating_add(h), self.height);

        if x >= x_end || y >= y_end {
            return;
        }

        let width = self.width;
        let pixels = self.pixels_slice_mut();
        for py in y..y_end {
            let start = py * width + x;
            let end = py * width + x_end;
            pixels[start..end].fill(color);
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
                    temp.push(self.pixels_slice()[sy * self.width + sx]);
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
                    let width_self = self.width;
                    let out_idx = dy.saturating_mul(width_self).saturating_add(dx);
                    self.pixels_slice_mut()[out_idx] = temp[idx];
                }
                idx += 1;
            }
        }
    }

    pub fn scroll_up(&mut self, rows: usize, fill: u32) {
        if rows == 0 || self.width == 0 || self.height == 0 {
            return;
        }

        let shift_rows = core::cmp::min(rows, self.height);
        let row_pixels = self.width;
        let shift = shift_rows.saturating_mul(row_pixels);
        let len = self.width.saturating_mul(self.height);

        let pixels = self.pixels_slice_mut();
        pixels.copy_within(shift..len, 0);
        pixels[len.saturating_sub(shift)..len].fill(fill);
    }
}
