use crate::pixelformat::{convert_pixel_format, PixelFormat};
use uefi::boot::{self, SearchType};
use uefi::proto::console::gop::GraphicsOutput;
use uefi::println;
use uefi::Identify;
pub struct FramebufferInfo {
    pub base: u64,
    pub size: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub pixel_format: PixelFormat,

}
pub fn initialize() -> uefi::Result<FramebufferInfo>  {
    let handle = *boot::locate_handle_buffer(
    SearchType::ByProtocol(&GraphicsOutput::GUID),
    )?
    .first()
    .expect("Graphics Output Protocol not found");

    let mut gop = boot::open_protocol_exclusive::<GraphicsOutput>(handle)?;

    let mode = gop.current_mode_info();
    let mut fb = gop.frame_buffer();

    println!(
        "Framebuffer: {}x{}",
        mode.resolution().0,
        mode.resolution().1
    );

    Ok(FramebufferInfo {
        base: fb.as_mut_ptr() as u64,
        size: fb.size(),
        width: mode.resolution().0 as usize,
        height: mode.resolution().1 as usize,
        stride: mode.stride(),
        pixel_format: convert_pixel_format(mode.pixel_format()),
    })
}