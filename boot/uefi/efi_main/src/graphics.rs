use uefi::Identify;
use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams, SearchType};
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::console::gop::PixelFormat as UefiPixelFormat;

fn detect_bpp(
    pixel_format: UefiPixelFormat,
    masks: (u32, u32, u32, u32),
    stride: usize,
    height: usize,
    fb_size: usize,
) -> usize {
    let bytes_from_layout = if stride != 0 && height != 0 {
        fb_size / (stride * height)
    } else {
        0
    };

    if pixel_format == UefiPixelFormat::Bitmask {
        let (r, g, b, a) = masks;
        let union = r | g | b | a;
        if union != 0 {
            let used_bits = 32usize - (union.leading_zeros() as usize);
            return (used_bits + 7) & !7;
        }
    }

    match pixel_format {
        UefiPixelFormat::Rgb | UefiPixelFormat::Bgr => {
            // On some firmware, framebuffer size may include extra backing memory;
            // only trust common packed pixel depths here.
            match bytes_from_layout {
                3 => 24,
                4 => 32,
                _ => 32,
            }
        }
        UefiPixelFormat::Bitmask => {
            if bytes_from_layout > 0 {
                return bytes_from_layout * 8;
            }
            32
        }
        UefiPixelFormat::BltOnly => 0,
    }
}
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
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub reserved_mask: u32,
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
            red_mask: 0,
            green_mask: 0,
            blue_mask: 0,
            reserved_mask: 0,
        }
    }
}
pub fn initialize() -> uefi::Result<FramebufferInfo> {
    let handles = boot::locate_handle_buffer(SearchType::ByProtocol(&GraphicsOutput::GUID))?;
    let handle = *handles.first().ok_or(uefi::Status::NOT_FOUND)?;

    let mut gop = unsafe {
        boot::open_protocol::<GraphicsOutput>(
            OpenProtocolParams {
                handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )?
    };

    let mode = gop.current_mode_info();
    if mode.pixel_format() == UefiPixelFormat::BltOnly {
        return Err(uefi::Status::UNSUPPORTED.into());
    }
    let mut fb = gop.frame_buffer();
    let resolution = mode.resolution();
    let stride = mode.stride();
    let pixel_format = mode.pixel_format();
    let (red_mask, green_mask, blue_mask, reserved_mask) = mode
        .pixel_bitmask()
        .map(|mask| (mask.red, mask.green, mask.blue, mask.reserved))
        .unwrap_or((0, 0, 0, 0));
    let bpp = detect_bpp(
        pixel_format,
        (red_mask, green_mask, blue_mask, reserved_mask),
        stride,
        resolution.1,
        fb.size(),
    );

    let fb_info = FramebufferInfo {
        base: fb.as_mut_ptr() as u64,
        size: fb.size(),
        width: resolution.0 as usize,
        height: resolution.1 as usize,
        stride,
        pixel_format: convert_pixel_format(pixel_format),
        bpp,
        red_mask,
        green_mask,
        blue_mask,
        reserved_mask,
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
