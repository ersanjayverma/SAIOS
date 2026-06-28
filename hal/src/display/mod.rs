pub mod gop;
pub mod vga;
pub mod virtio_gpu;

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb,
    Bgr,
}

#[derive(Debug)]
pub struct Surface<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: PixelFormat,
    pub pixels: &'a mut [u8],
}

#[derive(Debug, Copy, Clone)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub bpp: u8,
    pub format: PixelFormat,
}

pub trait DisplayHal {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn stride(&self) -> usize;
    fn bytes_per_pixel(&self) -> usize;
    fn frame_len(&self) -> usize;
    fn frame_bytes_mut(&mut self) -> &mut [u8];

    fn clear(&mut self, color: Color);
    fn put_pixel(&mut self, point: Point, color: Color);

    fn surface(&mut self) -> Surface<'_>;
    fn present(&mut self);
    fn current_mode(&self) -> DisplayMode;
    fn capabilities(&self) -> DisplayCapabilities;
}
pub struct DisplayCapabilities {
    pub hardware_cursor: bool,
    pub double_buffer: bool,
    pub acceleration: bool,
    pub vsync: bool,
    pub max_resolution: Option<(u32, u32)>,
    pub mode_switching: bool,
    pub hotplug: bool,
}
