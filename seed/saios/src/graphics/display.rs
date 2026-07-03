use core::ptr;
use efi_main::graphics::{FramebufferInfo, PixelFormat};

pub trait Display {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn stride(&self) -> usize;
    fn bytes_per_pixel(&self) -> usize;
    fn framebuffer_size(&self) -> usize;
    fn pixel_format(&self) -> PixelFormat;
    fn pixel_masks(&self) -> (u32, u32, u32, u32);
    fn framebuffer(&mut self) -> *mut u8;
    fn flush(&mut self, pixels: &[u32], src_width: usize, src_height: usize);
}

pub struct FramebufferDisplay {
    base: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    bytes_per_pixel: usize,
    size_bytes: usize,
    pixel_format: PixelFormat,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    reserved_mask: u32,
}

impl FramebufferDisplay {
    #[inline(always)]
    fn infer_masks_for_format(format: PixelFormat, bytes_per_pixel: usize) -> (u32, u32, u32, u32) {
        match (format, bytes_per_pixel) {
            (PixelFormat::Rgb, 2) => (0x001F, 0x07E0, 0xF800, 0),
            (PixelFormat::Bgr, 2) => (0xF800, 0x07E0, 0x001F, 0),
            (PixelFormat::Rgb, 3) | (PixelFormat::Rgb, 4) => (0x000000FF, 0x0000FF00, 0x00FF0000, 0),
            (PixelFormat::Bgr, 3) | (PixelFormat::Bgr, 4) => (0x00FF0000, 0x0000FF00, 0x000000FF, 0),
            _ => (0, 0, 0, 0),
        }
    }

    #[inline(always)]
    fn normalize_format(
        pixel_format: PixelFormat,
        bytes_per_pixel: usize,
        red_mask: u32,
        green_mask: u32,
        blue_mask: u32,
        reserved_mask: u32,
    ) -> (PixelFormat, u32, u32, u32, u32) {
        let masks_zero = red_mask == 0 && green_mask == 0 && blue_mask == 0;

        match pixel_format {
            PixelFormat::Bitmask => {
                if masks_zero {
                    let inferred = if bytes_per_pixel == 2 {
                        (0xF800, 0x07E0, 0x001F, 0)
                    } else {
                        (0x00FF0000, 0x0000FF00, 0x000000FF, 0)
                    };
                    (PixelFormat::Bitmask, inferred.0, inferred.1, inferred.2, inferred.3)
                } else {
                    (pixel_format, red_mask, green_mask, blue_mask, reserved_mask)
                }
            }
            PixelFormat::Rgb | PixelFormat::Bgr => {
                if bytes_per_pixel == 2 {
                    let inferred = Self::infer_masks_for_format(pixel_format, bytes_per_pixel);
                    (PixelFormat::Bitmask, inferred.0, inferred.1, inferred.2, inferred.3)
                } else {
                    (pixel_format, red_mask, green_mask, blue_mask, reserved_mask)
                }
            }
            PixelFormat::BltOnly => (PixelFormat::BltOnly, red_mask, green_mask, blue_mask, reserved_mask),
        }
    }

    pub fn from_info(info: FramebufferInfo) -> Option<Self> {
        let bytes_per_pixel = (info.bpp.saturating_add(7)) / 8;
        if info.base == 0
            || info.width == 0
            || info.height == 0
            || bytes_per_pixel == 0
            || bytes_per_pixel > 4
        {
            return None;
        }

        let mut stride_pixels = info.stride;
        let pixels_layout_ok = stride_pixels
            .checked_mul(info.height)
            .and_then(|v| v.checked_mul(bytes_per_pixel))
            .map(|v| v <= info.size)
            .unwrap_or(false);

        if !pixels_layout_ok {
            // Some firmware appears to report stride in bytes rather than pixels.
            if info.stride % bytes_per_pixel == 0 {
                let stride_from_bytes = info.stride / bytes_per_pixel;
                let bytes_layout_ok = info
                    .stride
                    .checked_mul(info.height)
                    .map(|v| v <= info.size)
                    .unwrap_or(false);
                if bytes_layout_ok && stride_from_bytes >= info.width {
                    stride_pixels = stride_from_bytes;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }

        let min_visible = stride_pixels
            .checked_mul(info.height)?
            .checked_mul(bytes_per_pixel)?;
        if info.size < min_visible {
            return None;
        }

        let (pixel_format, red_mask, green_mask, blue_mask, reserved_mask) = Self::normalize_format(
            info.pixel_format,
            bytes_per_pixel,
            info.red_mask,
            info.green_mask,
            info.blue_mask,
            info.reserved_mask,
        );

        Some(Self {
            base: info.base as *mut u8,
            width: info.width,
            height: info.height,
            stride: stride_pixels,
            bytes_per_pixel,
            size_bytes: info.size,
            pixel_format,
            red_mask,
            green_mask,
            blue_mask,
            reserved_mask,
        })
    }

    #[inline(always)]
    fn pack_channel(value: u8, mask: u32) -> u32 {
        if mask == 0 {
            return 0;
        }

        let shift = mask.trailing_zeros();
        let width = mask.count_ones();
        if width == 0 {
            return 0;
        }

        let max = (1u32 << width) - 1;
        let scaled = ((value as u32) * max + 127) / 255;
        (scaled << shift) & mask
    }

    #[inline(always)]
    fn pack_bitmask(&self, r: u8, g: u8, b: u8) -> u32 {
        Self::pack_channel(r, self.red_mask)
            | Self::pack_channel(g, self.green_mask)
            | Self::pack_channel(b, self.blue_mask)
            | self.reserved_mask
    }

    #[inline(always)]
    unsafe fn write_packed(dst: *mut u8, packed: u32, bytes_per_pixel: usize) {
        let bytes = packed.to_le_bytes();
        let count = core::cmp::min(bytes_per_pixel, 4);
        let mut i = 0;
        while i < count {
            ptr::write_volatile(dst.add(i), bytes[i]);
            i += 1;
        }
    }

    #[inline(always)]
    unsafe fn write_rgb_like(dst: *mut u8, r: u8, g: u8, b: u8, bytes_per_pixel: usize, bgr: bool) {
        if bgr {
            ptr::write_volatile(dst, b);
            ptr::write_volatile(dst.add(1), g);
            ptr::write_volatile(dst.add(2), r);
        } else {
            ptr::write_volatile(dst, r);
            ptr::write_volatile(dst.add(1), g);
            ptr::write_volatile(dst.add(2), b);
        }
        if bytes_per_pixel >= 4 {
            ptr::write_volatile(dst.add(3), 0);
        }
    }
}

impl Display for FramebufferDisplay {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn stride(&self) -> usize {
        self.stride
    }

    fn bytes_per_pixel(&self) -> usize {
        self.bytes_per_pixel
    }

    fn framebuffer_size(&self) -> usize {
        self.size_bytes
    }

    fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    fn pixel_masks(&self) -> (u32, u32, u32, u32) {
        (self.red_mask, self.green_mask, self.blue_mask, self.reserved_mask)
    }

    fn framebuffer(&mut self) -> *mut u8 {
        self.base
    }

    fn flush(&mut self, pixels: &[u32], src_width: usize, src_height: usize) {
        let width = core::cmp::min(self.width, src_width);
        let height = core::cmp::min(self.height, src_height);

        for y in 0..height {
            for x in 0..width {
                let src = pixels[y * src_width + x];
                let r = ((src >> 16) & 0xFF) as u8;
                let g = ((src >> 8) & 0xFF) as u8;
                let b = (src & 0xFF) as u8;

                let offset = (y * self.stride + x) * self.bytes_per_pixel;
                if offset + self.bytes_per_pixel > self.size_bytes {
                    continue;
                }
                unsafe {
                    let p = self.base.add(offset);
                    match self.pixel_format {
                        PixelFormat::Rgb => {
                            Self::write_rgb_like(p, r, g, b, self.bytes_per_pixel, false);
                        }
                        PixelFormat::Bgr => {
                            Self::write_rgb_like(p, r, g, b, self.bytes_per_pixel, true);
                        }
                        PixelFormat::Bitmask => {
                            Self::write_packed(p, self.pack_bitmask(r, g, b), self.bytes_per_pixel);
                        }
                        PixelFormat::BltOnly => {}
                    }
                }
            }
        }
    }
}
