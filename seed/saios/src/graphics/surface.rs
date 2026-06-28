use crate::graphics::Color;

pub struct Surface<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub bpp: u8,
    pub is_bgr: bool,
    pub pixels: &'a mut [u8],
}

impl Surface<'_> {
    pub fn clear(&mut self, color: Color) {
        let bytes_per_pixel = (self.bpp as usize).saturating_div(8);
        if bytes_per_pixel < 3 {
            return;
        }

        for chunk in self.pixels.chunks_exact_mut(bytes_per_pixel) {
            chunk[0] = if self.is_bgr { color.b } else { color.r };
            chunk[1] = color.g;
            chunk[2] = if self.is_bgr { color.r } else { color.b };
            if bytes_per_pixel > 3 {
                chunk[3] = color.a;
            }
        }
    }
}
