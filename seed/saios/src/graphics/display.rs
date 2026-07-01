use efi_main::graphics::{FramebufferInfo, PixelFormat};

pub trait Display {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn stride(&self) -> usize;
    fn pixel_format(&self) -> PixelFormat;
    fn framebuffer(&mut self) -> *mut u8;
    fn flush(&mut self, pixels: &[u32], src_width: usize, src_height: usize);
}

pub struct FramebufferDisplay {
    base: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    pixel_format: PixelFormat,
}

impl FramebufferDisplay {
    pub fn from_info(info: FramebufferInfo) -> Option<Self> {
        if info.base == 0 || info.width == 0 || info.height == 0 || info.bpp != 32 {
            return None;
        }

        Some(Self {
            base: info.base as *mut u8,
            width: info.width,
            height: info.height,
            stride: info.stride,
            pixel_format: info.pixel_format,
        })
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

    fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
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

                let offset = (y * self.stride + x) * 4;
                unsafe {
                    let p = self.base.add(offset);
                    match self.pixel_format {
                        PixelFormat::Rgb => {
                            *p = r;
                            *p.add(1) = g;
                            *p.add(2) = b;
                            *p.add(3) = 0;
                        }
                        PixelFormat::Bgr | PixelFormat::Bitmask => {
                            *p = b;
                            *p.add(1) = g;
                            *p.add(2) = r;
                            *p.add(3) = 0;
                        }
                        PixelFormat::BltOnly => {}
                    }
                }
            }
        }
    }
}
