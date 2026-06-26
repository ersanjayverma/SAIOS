use uefi::proto::console::gop::PixelFormat as UefiPixelFormat;
use uefi::Identify;
use uefi::boot::{self, SearchType};
use uefi::println;
use uefi::proto::console::gop::GraphicsOutput;
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct FramebufferInfo {
    pub base: u64,
    pub size: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub pixel_format: PixelFormat,
}
pub fn initialize() -> uefi::Result<FramebufferInfo> {
    let handle = *boot::locate_handle_buffer(SearchType::ByProtocol(&GraphicsOutput::GUID))?
        .first()
        .expect("Graphics Output Protocol not found");

    let mut gop = boot::open_protocol_exclusive::<GraphicsOutput>(handle)?;

    let mode = gop.current_mode_info();
    let mut fb = gop.frame_buffer();

    println!("Framebuffer base address: {:#x}", fb.as_mut_ptr() as u64);
    println!("Framebuffer size: {} bytes", fb.size());
    println!("Framebuffer stride: {} pixels", mode.stride());
    println!(
        "Framebuffer pixel format: {:?}",
        convert_pixel_format(mode.pixel_format())
    );
    println!("{}x{}", mode.resolution().0, mode.resolution().1);

    Ok(FramebufferInfo {
        base: fb.as_mut_ptr() as u64,
        size: fb.size(),
        width: mode.resolution().0 as usize,
        height: mode.resolution().1 as usize,
        stride: mode.stride(),
        pixel_format: convert_pixel_format(mode.pixel_format()),
    })
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
