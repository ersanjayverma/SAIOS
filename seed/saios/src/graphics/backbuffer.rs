use crate::graphics::Surface;

const MAX_BACKBUFFER_BYTES: usize = 1920 * 1080 * 4;
static mut BACKBUFFER: [u8; MAX_BACKBUFFER_BYTES] = [0; MAX_BACKBUFFER_BYTES];

pub struct BackBuffer {
    width: u32,
    height: u32,
    stride_pixels: usize,
    bpp: u8,
}

impl BackBuffer {
    pub const fn new(width: u32, height: u32, stride_pixels: usize, bpp: u8) -> Self {
        Self {
            width,
            height,
            stride_pixels,
            bpp,
        }
    }

    pub fn surface(&mut self) -> Surface<'_> {
        let bytes_per_pixel = (self.bpp as usize) / 8;
        let needed = self.stride_pixels * self.height as usize * bytes_per_pixel;
        let max = core::cmp::min(needed, MAX_BACKBUFFER_BYTES);
        let ptr = core::ptr::addr_of_mut!(BACKBUFFER).cast::<u8>();
        let pixels = unsafe { core::slice::from_raw_parts_mut(ptr, max) };

        Surface {
            width: self.width,
            height: self.height,
            stride: self.stride_pixels,
            bpp: self.bpp,
            pixels,
        }
    }

    pub fn blit_to_framebuffer(&self, fb_base: *mut u8) {
        let bytes_per_pixel = (self.bpp as usize) / 8;
        let len = core::cmp::min(
            self.stride_pixels * self.height as usize * bytes_per_pixel,
            MAX_BACKBUFFER_BYTES,
        );
        let src_ptr = core::ptr::addr_of!(BACKBUFFER).cast::<u8>();
        unsafe {
            core::ptr::copy_nonoverlapping(src_ptr, fb_base, len);
        }
    }
}
