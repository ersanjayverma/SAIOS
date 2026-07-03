//! UEFI boot-time UI helpers.
//!
//! Provides bitmap parsing and framebuffer drawing routines used to display
//! the boot splash screen before the kernel takes over the display.

use crate::graphics::FramebufferInfo;
use crate::graphics::PixelFormat;

/// Boot splash bitmap embedded at compile time.
pub static SPLASH: &[u8] = include_bytes!("./assets/splash.bmp");

/// Packs an 8-bit color channel into the position described by `mask`.
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
fn pack_bitmask(info: &FramebufferInfo, color: [u8; 4]) -> u32 {
    pack_channel(color[0], info.red_mask)
        | pack_channel(color[1], info.green_mask)
        | pack_channel(color[2], info.blue_mask)
        | info.reserved_mask
}

#[inline(always)]
unsafe fn write_packed(dst: *mut u8, packed: u32, bytes_per_pixel: usize) {
    let bytes = packed.to_le_bytes();
    let count = core::cmp::min(bytes_per_pixel, 4);
    let mut i = 0;
    while i < count {
        unsafe {
            core::ptr::write_volatile(dst.add(i), bytes[i]);
        }
        i += 1;
    }
}

#[repr(C)]
pub struct Bitmap<'a> {
    /// Bitmap width in pixels.
    pub width: u32,
    /// Bitmap height in pixels.
    pub height: u32,
    /// Bits per pixel (24 or 32).
    pub bpp: u16,
    /// Bytes between the start of two bitmap rows.
    pub stride: u32,
    /// Raw pixel data.
    pub pixels: &'a [u8],
}

/// Wrapper around the firmware framebuffer.
pub struct Framebuffer {
    /// Framebuffer geometry and pixel format information.
    pub info: FramebufferInfo,
}

impl Framebuffer {
    /// Returns the number of bytes per framebuffer pixel.
    #[inline(always)]
    fn bytes_per_pixel(&self) -> usize {
        core::cmp::max(self.info.bpp / 8, 1)
    }

    /// Fills a rectangle with `color`.
    pub fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: [u8; 4]) {
        let x_end = core::cmp::min(x.saturating_add(width), self.info.width);
        let y_end = core::cmp::min(y.saturating_add(height), self.info.height);

        for py in y..y_end {
            for px in x..x_end {
                self.put_pixel(px, py, color);
            }
        }
    }

    /// Draws the outline of a rectangle with `color`.
    pub fn draw_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: [u8; 4]) {
        if width == 0 || height == 0 {
            return;
        }

        self.fill_rect(x, y, width, 1, color);
        self.fill_rect(x, y + height.saturating_sub(1), width, 1, color);
        self.fill_rect(x, y, 1, height, color);
        self.fill_rect(x + width.saturating_sub(1), y, 1, height, color);
    }

    /// Writes a single pixel to the framebuffer.
    pub fn put_pixel(&mut self, x: usize, y: usize, color: [u8; 4]) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }

        let bytes_per_pixel = self.bytes_per_pixel();
        let pixel_offset = (y * self.info.stride + x) * bytes_per_pixel;
        if pixel_offset + bytes_per_pixel > self.info.size {
            return;
        }

        let dst = unsafe { (self.info.base as *mut u8).add(pixel_offset) };
        match self.info.pixel_format {
            PixelFormat::Rgb => unsafe {
                core::ptr::write_volatile(dst, color[0]);
                if bytes_per_pixel >= 2 {
                    core::ptr::write_volatile(dst.add(1), color[1]);
                }
                if bytes_per_pixel >= 3 {
                    core::ptr::write_volatile(dst.add(2), color[2]);
                }
                if bytes_per_pixel >= 4 {
                    core::ptr::write_volatile(dst.add(3), color[3]);
                }
            },
            PixelFormat::Bgr => unsafe {
                core::ptr::write_volatile(dst, color[2]);
                if bytes_per_pixel >= 2 {
                    core::ptr::write_volatile(dst.add(1), color[1]);
                }
                if bytes_per_pixel >= 3 {
                    core::ptr::write_volatile(dst.add(2), color[0]);
                }
                if bytes_per_pixel >= 4 {
                    core::ptr::write_volatile(dst.add(3), color[3]);
                }
            },
            PixelFormat::Bitmask => unsafe {
                write_packed(dst, pack_bitmask(&self.info, color), bytes_per_pixel);
            },
            PixelFormat::BltOnly => unsafe {
                write_packed(dst, pack_bitmask(&self.info, color), bytes_per_pixel);
            },
        }
    }

    pub fn clear(&mut self, color: [u8; 4]) {
        let bytes_per_pixel = self.bytes_per_pixel();
        if self.info.base == 0 || self.info.stride == 0 || bytes_per_pixel == 0 {
            return;
        }

        let mut packed = [0u8; 4];
        match self.info.pixel_format {
            PixelFormat::Rgb => {
                packed[0] = color[0];
                packed[1] = color[1];
                packed[2] = color[2];
                packed[3] = color[3];
            }
            PixelFormat::Bgr => {
                packed[0] = color[2];
                packed[1] = color[1];
                packed[2] = color[0];
                packed[3] = color[3];
            }
            PixelFormat::Bitmask | PixelFormat::BltOnly => {
                packed = pack_bitmask(&self.info, color).to_le_bytes();
            }
        }

        for y in 0..self.info.height {
            let row_base = y * self.info.stride * bytes_per_pixel;
            for x in 0..self.info.width {
                let offset = row_base + x * bytes_per_pixel;
                if offset + bytes_per_pixel > self.info.size {
                    break;
                }
                let dst = unsafe { (self.info.base as *mut u8).add(offset) };
                unsafe {
                    let mut i = 0;
                    while i < bytes_per_pixel {
                        core::ptr::write_volatile(dst.add(i), packed[i]);
                        i += 1;
                    }
                }
            }
        }
    }
}
impl<'a> Bitmap<'a> {
    fn can_fast_blit(&self, framebuffer: &Framebuffer) -> bool {
        let fb_bpp = framebuffer.bytes_per_pixel();
        matches!(
            framebuffer.info.pixel_format,
            PixelFormat::Rgb | PixelFormat::Bgr
        ) && fb_bpp == 4
            && (self.bpp == 24 || self.bpp == 32)
    }

    fn draw_at_fast(&self, framebuffer: &mut Framebuffer, dst_x: usize, dst_y: usize) {
        let src_bpp = (self.bpp / 8) as usize;
        let dst_bpp = framebuffer.bytes_per_pixel();

        for y in 0..self.height as usize {
            let src_y = self.height as usize - 1 - y;
            let target_y = dst_y + y;
            if target_y >= framebuffer.info.height {
                break;
            }

            let row_start = src_y * self.stride as usize;
            for x in 0..self.width as usize {
                let target_x = dst_x + x;
                if target_x >= framebuffer.info.width {
                    break;
                }

                let src_offset = row_start + x * src_bpp;
                let dst_offset = (target_y * framebuffer.info.stride + target_x) * dst_bpp;
                if dst_offset + dst_bpp > framebuffer.info.size {
                    break;
                }

                let pixel = &self.pixels[src_offset..src_offset + src_bpp];
                let dst = unsafe { (framebuffer.info.base as *mut u8).add(dst_offset) };
                unsafe {
                    match framebuffer.info.pixel_format {
                        PixelFormat::Bgr => {
                            core::ptr::write_volatile(dst, pixel[0]);
                            core::ptr::write_volatile(dst.add(1), pixel[1]);
                            core::ptr::write_volatile(dst.add(2), pixel[2]);
                            core::ptr::write_volatile(dst.add(3), 0xFF);
                        }
                        PixelFormat::Rgb => {
                            core::ptr::write_volatile(dst, pixel[2]);
                            core::ptr::write_volatile(dst.add(1), pixel[1]);
                            core::ptr::write_volatile(dst.add(2), pixel[0]);
                            core::ptr::write_volatile(dst.add(3), 0xFF);
                        }
                        // can_fast_blit only returns true for Rgb/Bgr, so
                        // this arm is defensive and should not be reached.
                        PixelFormat::Bitmask | PixelFormat::BltOnly => {}
                    }
                }
            }
        }
    }

    /// Parses a 24-bit or 32-bit uncompressed BMP from `data`.
    pub fn from_bytes(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.len() < 54 {
            return Err("BMP too small");
        }

        // "BM"
        if &data[0..2] != b"BM" {
            return Err("Invalid BMP signature");
        }

        let pixel_offset = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;

        let width = u32::from_le_bytes(data[18..22].try_into().unwrap());

        let height = u32::from_le_bytes(data[22..26].try_into().unwrap());

        let planes = u16::from_le_bytes(data[26..28].try_into().unwrap());

        let bpp = u16::from_le_bytes(data[28..30].try_into().unwrap());

        let compression = u32::from_le_bytes(data[30..34].try_into().unwrap());

        if planes != 1 {
            return Err("Invalid planes");
        }

        if compression != 0 {
            return Err("Compressed BMP not supported");
        }

        if bpp != 24 && bpp != 32 {
            return Err("Only 24/32-bit BMP supported");
        }

        Ok(Self {
            width,
            height,
            stride: (width * (bpp as u32 / 8)).div_ceil(4) * 4, // Align to 4 bytes
            bpp,
            pixels: &data[pixel_offset..],
        })
    }
    /// Draws the bitmap at `(dst_x, dst_y)` in the framebuffer.
    pub fn draw_at(&self, framebuffer: &mut Framebuffer, dst_x: usize, dst_y: usize) {
        if self.can_fast_blit(framebuffer) {
            self.draw_at_fast(framebuffer, dst_x, dst_y);
            return;
        }

        let bytes_per_pixel = (self.bpp / 8) as usize;
        let stride = self.stride as usize;

        for y in 0..self.height as usize {
            // Flip vertically
            let src_y = self.height as usize - 1 - y;
            let target_y = dst_y + y;
            if target_y >= framebuffer.info.height {
                break;
            }

            for x in 0..self.width as usize {
                let target_x = dst_x + x;
                if target_x >= framebuffer.info.width {
                    break;
                }
                let pixel_index = src_y * stride + x * bytes_per_pixel;
                let pixel = &self.pixels[pixel_index..pixel_index + bytes_per_pixel];

                framebuffer.put_pixel(target_x, target_y, [pixel[2], pixel[1], pixel[0], 255]);
            }
        }
    }

    pub fn draw_centered(&self, framebuffer: &mut Framebuffer) {
        let dst_x = framebuffer.info.width.saturating_sub(self.width as usize) / 2;
        let dst_y = framebuffer.info.height.saturating_sub(self.height as usize) / 2;
        self.draw_at(framebuffer, dst_x, dst_y);
    }
}

pub fn draw_boot_splash(info: FramebufferInfo) {
    if info.base == 0 || info.width == 0 || info.height == 0 {
        return;
    }

    let mut framebuffer = Framebuffer { info };
    framebuffer.clear([8, 12, 20, 255]);

    if let Ok(bitmap) = Bitmap::from_bytes(SPLASH) {
        bitmap.draw_centered(&mut framebuffer);
    }
}
