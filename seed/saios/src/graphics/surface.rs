use alloc::vec::Vec;

/// Backing storage for a [`Surface`]: either a heap-owned pixel buffer or a
/// borrowed static slice (e.g. a reserved region used as a scratch buffer).
enum PixelStorage {
    Owned(Vec<u32>),
    Borrowed(&'static mut [u32]),
}

/// An off-screen, CPU-side image of `width` x `height` packed 0x00RRGGBB
/// pixels.
///
/// Higher layers draw into a `Surface` and then flush it to a
/// [`crate::graphics::display::Display`]. Keeping drawing off-screen lets the
/// renderer batch work and avoids slow read-modify-write traffic to the
/// hardware framebuffer.
pub struct Surface {
    width: usize,
    height: usize,
    pixels: PixelStorage,
}

impl Surface {
    /// Allocate a heap-backed surface, returning `None` if the pixel buffer
    /// cannot be reserved.
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

    /// Wrap an externally owned static pixel buffer as a surface, clearing it to
    /// black. Returns `None` if the slice is too small for the geometry.
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

    /// Immutable access to the underlying pixel slice regardless of storage kind.
    fn pixels_slice(&self) -> &[u32] {
        match &self.pixels {
            PixelStorage::Owned(v) => v.as_slice(),
            PixelStorage::Borrowed(s) => s,
        }
    }

    /// Mutable access to the underlying pixel slice regardless of storage kind.
    fn pixels_slice_mut(&mut self) -> &mut [u32] {
        match &mut self.pixels {
            PixelStorage::Owned(v) => v.as_mut_slice(),
            PixelStorage::Borrowed(s) => s,
        }
    }

    /// Width of the surface in pixels.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height of the surface in pixels.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Immutable view of the raw pixel buffer (row-major, `width * height`).
    pub fn pixels(&self) -> &[u32] {
        self.pixels_slice()
    }

    /// Mutable view of the raw pixel buffer (row-major, `width * height`).
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        self.pixels_slice_mut()
    }

    /// Return a mutable slice covering row `y`, or `None` if out of bounds.
    ///
    /// This lets callers write directly into a scanline without repeated
    /// bounds checks and index recalculation, which is much faster than
    /// calling [`put_pixel`](Self::put_pixel) in an inner loop.
    pub fn row_mut(&mut self, y: usize) -> Option<&mut [u32]> {
        if y >= self.height {
            return None;
        }
        let width = self.width;
        let start = y * width;
        Some(&mut self.pixels_slice_mut()[start..start + width])
    }

    /// Return an immutable slice covering row `y`, or `None` if out of bounds.
    pub fn row(&self, y: usize) -> Option<&[u32]> {
        if y >= self.height {
            return None;
        }
        let width = self.width;
        let start = y * width;
        Some(&self.pixels_slice()[start..start + width])
    }

    /// Fill the entire surface with `color`.
    pub fn clear(&mut self, color: u32) {
        self.pixels_slice_mut().fill(color);
    }

    /// Set a single pixel, silently ignoring out-of-bounds coordinates.
    pub fn put_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }

        let width = self.width;
        let idx = y.saturating_mul(width).saturating_add(x);
        self.pixels_slice_mut()[idx] = color;
    }

    /// Fill an axis-aligned rectangle with `color`, clipping to the surface
    /// bounds. Uses per-row slice fills for speed.
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

    /// Draw a line between two points using Bresenham's algorithm.
    ///
    /// Horizontal and vertical lines are handled as single slice fills, which
    /// avoids the per-pixel bounds checking and index math of the generic
    /// Bresenham loop.
    pub fn draw_line(&mut self, x0: isize, y0: isize, x1: isize, y1: isize, color: u32) {
        // Fast path: horizontal line → one slice fill.
        if y0 == y1 && y0 >= 0 && (y0 as usize) < self.height {
            let y = y0 as usize;
            let (x_start, x_end) = if x0 <= x1 {
                (
                    x0.max(0) as usize,
                    (x1 + 1).min(self.width as isize).max(0) as usize,
                )
            } else {
                (
                    x1.max(0) as usize,
                    (x0 + 1).min(self.width as isize).max(0) as usize,
                )
            };
            if let Some(row) = self.row_mut(y) {
                row[x_start..x_end].fill(color);
            }
            return;
        }

        // Fast path: vertical line → fill one pixel per row via stride.
        if x0 == x1 && x0 >= 0 && (x0 as usize) < self.width {
            let x = x0 as usize;
            let (y_start, y_end) = if y0 <= y1 {
                (
                    y0.max(0) as usize,
                    (y1 + 1).min(self.height as isize).max(0) as usize,
                )
            } else {
                (
                    y1.max(0) as usize,
                    (y0 + 1).min(self.height as isize).max(0) as usize,
                )
            };
            let width = self.width;
            let pixels = self.pixels_slice_mut();
            for py in y_start..y_end {
                pixels[py * width + x] = color;
            }
            return;
        }

        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let width = self.width;
        let height = self.height;
        let pixels = self.pixels_slice_mut();

        loop {
            if x >= 0 && y >= 0 {
                let px = x as usize;
                let py = y as usize;
                if px < width && py < height {
                    pixels[py * width + px] = color;
                }
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

    /// Copy a rectangular block of pixels from one location to another within
    /// the same surface. A temporary buffer is used so source and destination
    /// regions may overlap.
    ///
    /// The copy is performed row-by-row with slice operations instead of
    /// per-pixel loops, which removes most bounds-check overhead.
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

        let src_x_end = core::cmp::min(src_x.saturating_add(width), self.width);
        let src_y_end = core::cmp::min(src_y.saturating_add(height), self.height);
        let copy_width = src_x_end.saturating_sub(src_x);
        let copy_height = src_y_end.saturating_sub(src_y);
        if copy_width == 0 || copy_height == 0 {
            return;
        }

        // Read the source region into a temporary row buffer.  Using one row at
        // a time avoids a large allocation and still lets us copy via slices.
        let mut temp: Vec<u32> = Vec::new();
        if temp.try_reserve(copy_width).is_err() {
            return;
        }
        temp.resize(copy_width, 0);

        for y in 0..copy_height {
            let sy = src_y + y;
            let dy = dst_y.saturating_add(y);
            if dy >= self.height {
                continue;
            }

            let src_start = sy * self.width + src_x;
            temp.copy_from_slice(&self.pixels_slice()[src_start..src_start + copy_width]);

            let dst_start = dy * self.width + dst_x;
            let available = self.width.saturating_sub(dst_x);
            let write_width = core::cmp::min(copy_width, available);
            if write_width == 0 {
                continue;
            }
            self.pixels_slice_mut()[dst_start..dst_start + write_width]
                .copy_from_slice(&temp[..write_width]);
        }
    }

    /// Scroll the whole surface up by `rows` pixel rows, filling the newly
    /// exposed rows at the bottom with `fill`. Implemented as a single
    /// `copy_within` plus a slice fill.
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
