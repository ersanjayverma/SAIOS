use uefi::proto::console::gop::PixelFormat as UefiPixelFormat;
#[derive(Debug, Clone, Copy)]
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
