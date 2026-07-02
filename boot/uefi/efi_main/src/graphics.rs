use uefi::Identify;
use uefi::boot::{self, SearchType};
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::console::gop::PixelFormat as UefiPixelFormat;
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FramebufferInfo {
    pub base: u64,
    pub size: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub pixel_format: PixelFormat,
    pub bpp: usize, // Bits per pixel
}

impl FramebufferInfo {
    pub const fn empty() -> Self {
        Self {
            base: 0,
            size: 0,
            width: 0,
            height: 0,
            stride: 0,
            pixel_format: PixelFormat::BltOnly,
            bpp: 0,
        }
    }
}
pub fn initialize() -> uefi::Result<FramebufferInfo> {
    let handle = *boot::locate_handle_buffer(SearchType::ByProtocol(&GraphicsOutput::GUID))?
        .first()
        .expect("Graphics Output Protocol not found");

    let mut gop = boot::open_protocol_exclusive::<GraphicsOutput>(handle)?;

    let mode = gop.current_mode_info();
    let mut fb = gop.frame_buffer();

    let fb_info = FramebufferInfo {
        base: fb.as_mut_ptr() as u64,
        size: fb.size(),
        width: mode.resolution().0 as usize,
        height: mode.resolution().1 as usize,
        stride: mode.stride(),
        pixel_format: convert_pixel_format(mode.pixel_format()),
        bpp: match mode.pixel_format() {
            UefiPixelFormat::Rgb => 32,
            UefiPixelFormat::Bgr => 32,
            UefiPixelFormat::Bitmask => 32,
            UefiPixelFormat::BltOnly => panic!("BLT-only framebuffer not supported"),
        },
    };
    drop(gop);
    Ok(fb_info)
}
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(C)]
pub enum PixelFormat {
    Rgb,
    Bgr,
    Bitmask,
    BltOnly,
}

pub fn convert_pixel_format(fmt: UefiPixelFormat) -> PixelFormat {
    match fmt {
        UefiPixelFormat::Rgb => PixelFormat::Rgb,
        UefiPixelFormat::Bgr => PixelFormat::Bgr,
        UefiPixelFormat::Bitmask => PixelFormat::Bitmask,
        UefiPixelFormat::BltOnly => PixelFormat::BltOnly,
    }
}
