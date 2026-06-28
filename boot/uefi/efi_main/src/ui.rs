use crate::graphics::FramebufferInfo;
use crate::graphics::PixelFormat;
pub static SPLASH: &[u8] = include_bytes!("./assets/splash.bmp");
#[repr(C)]
pub struct Bitmap<'a> {
    pub width: u32,
    pub height: u32,
    pub bpp: u16,
    pub stride: u32,
    pub pixels: &'a [u8],
}

pub struct Framebuffer {
    pub info: FramebufferInfo,
}

impl Framebuffer {
    pub fn put_pixel(&mut self, x: usize, y: usize, color: [u8; 4]) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }

        let bytes_per_pixel = self.info.bpp as usize / 8;
        let pixel_offset = (y * self.info.stride + x) * bytes_per_pixel;
        let framebuffer_slice = unsafe {
            core::slice::from_raw_parts_mut(
                self.info.base as *mut u8,
                self.info.size, // Total framebuffer size in bytes
            )
        };
        match self.info.pixel_format {
            PixelFormat::Rgb => {
                framebuffer_slice[pixel_offset] = color[0]; // R
                framebuffer_slice[pixel_offset + 1] = color[1]; // G
                framebuffer_slice[pixel_offset + 2] = color[2]; // B
            }
            PixelFormat::Bgr => {
                framebuffer_slice[pixel_offset] = color[2]; // B
                framebuffer_slice[pixel_offset + 1] = color[1]; // G
                framebuffer_slice[pixel_offset + 2] = color[0]; // R
            }
            PixelFormat::Bitmask | PixelFormat::BltOnly => {
                framebuffer_slice[pixel_offset] = color[0]; // R
                framebuffer_slice[pixel_offset + 1] = color[1]; // G
                framebuffer_slice[pixel_offset + 2] = color[2]; // B
                framebuffer_slice[pixel_offset + 3] = color[3]; // A
            }
        }
    }

    pub fn clear(&mut self, color: [u8; 4]) {
        for y in 0..self.info.height {
            for x in 0..self.info.width {
                self.put_pixel(x, y, color);
            }
        }
    }
}
impl<'a> Bitmap<'a> {
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
            stride: ((width * (bpp as u32 / 8) + 3) / 4) * 4 as u32, // Align to 4 bytes
            bpp,
            pixels: &data[pixel_offset..],
        })
    }
    pub fn draw(&self, framebuffer: &mut Framebuffer) {
        let bytes_per_pixel = (self.bpp / 8) as usize;
        let stride = self.stride as usize;

        for y in 0..self.height as usize {
            // Flip vertically
            let src_y = self.height as usize - 1 - y;

            for x in 0..self.width as usize {
                let pixel_index = src_y * stride + x * bytes_per_pixel;
                let pixel = &self.pixels[pixel_index..pixel_index + bytes_per_pixel];

                framebuffer.put_pixel(x, y, [pixel[2], pixel[1], pixel[0], 255]);
            }
        }
    }
}
