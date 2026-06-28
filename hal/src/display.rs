extern crate alloc;

use alloc::vec::Vec;
use alloc::vec;
use core::cmp::{max, min};

pub type Fixed = i32; // 16.16
pub const FIXED_ONE: Fixed = 1 << 16;
pub const FIXED_SHIFT: i32 = 16;

#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Angle(pub i32); // millidegrees

impl Angle {
    pub const ZERO: Self = Self(0);
    pub const DEG90: Self = Self(90_000);
    pub const DEG180: Self = Self(180_000);
    pub const DEG270: Self = Self(270_000);
    pub const DEG360: Self = Self(360_000);
}

#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TextureId(pub u32);

#[inline]
pub const fn fixed_from_int(v: i32) -> Fixed {
    v << FIXED_SHIFT
}

#[inline]
pub const fn fixed_to_int(v: Fixed) -> i32 {
    v >> FIXED_SHIFT
}

#[inline]
pub fn fixed_mul(a: Fixed, b: Fixed) -> Fixed {
    ((a as i64 * b as i64) >> FIXED_SHIFT) as Fixed
}

#[inline]
pub fn fixed_div(a: Fixed, b: Fixed) -> Fixed {
    if b == 0 {
        return 0;
    }
    (((a as i64) << FIXED_SHIFT) / b as i64) as Fixed
}

pub trait DisplayHal {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn stride(&self) -> usize;
    fn bytes_per_pixel(&self) -> u8;
    fn pixel_format(&self) -> PixelFormat;

    fn capabilities(&self) -> DisplayCapabilities;

    fn begin_frame(&mut self);
    fn present(&mut self);
    fn flush(&mut self);

    fn surface(&mut self) -> Surface<'_>;

    fn clear(&mut self, color: Color);
    fn put_pixel(&mut self, point: Point, color: Color);

    fn blit(&mut self, src: &Image, dst: Point) -> Result<(), DisplayError>;
    fn current_mode(&self) -> DisplayMode;
    fn available_modes(&self) -> &[DisplayMode];
    fn set_mode(&mut self, mode: DisplayMode) -> Result<(), DisplayError>;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StrokeStyle {
    Solid,
    Dashed,
    Dotted,
    DashDot,
    DashDotDot,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StrokeCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StrokeJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Copy, Clone)]
pub struct Stroke {
    pub color: Color,
    pub width: u32,
    pub style: StrokeStyle,
    pub cap: StrokeCap,
    pub join: StrokeJoin,
}

pub trait Renderer {
    fn draw_text(&mut self, point: Point, text: &str, font: &Font, color: Color);
    fn measure_text(&self, text: &str, font: &Font) -> TextMetrics;

    fn draw_image(&mut self, point: Point, image: &Image);

    fn draw_pixel(&mut self, point: Point, color: Color);
    fn draw_line(&mut self, point1: Point, point2: Point, color: Color);
    fn draw_rect(&mut self, rect: &Rect, stroke: Stroke);
    fn fill_rect(&mut self, rect: &Rect, fill: Fill);
    fn draw_circle(&mut self, point: Point, radius: u32, color: Color);
    fn fill_circle(&mut self, point: Point, radius: u32, color: Color);

    fn draw_triangle(&mut self, point1: Point, point2: Point, point3: Point, color: Color);
    fn fill_triangle(&mut self, point1: Point, point2: Point, point3: Point, color: Color);

    fn draw_polygon(&mut self, points: &[Point], color: Color);
    fn fill_polygon(&mut self, points: &[Point], color: Color);

    fn draw_bezier(&mut self, points: &[Point], color: Color);
    fn draw_arc(
        &mut self,
        point: Point,
        radius: u32,
        start_angle: Angle,
        end_angle: Angle,
        color: Color,
    );
    fn draw_ellipse(&mut self, point: Point, radius_x: u32, radius_y: u32, color: Color);
    fn fill_ellipse(&mut self, point: Point, radius_x: u32, radius_y: u32, color: Color);

    fn set_clip(&mut self, point: Point, width: u32, height: u32);
    fn reset_clip(&mut self);
    fn push_clip(&mut self, rect: Rect);
    fn pop_clip(&mut self);

    fn translate(&mut self, dx: i32, dy: i32);
    fn rotate(&mut self, angle: Angle);
    fn scale(&mut self, sx: Fixed, sy: Fixed);
    fn push_transform(&mut self);
    fn pop_transform(&mut self);
}

pub trait WidgetRenderer {
    fn draw_window(&mut self, point: Point, width: u32, height: u32, title: &str);
    fn draw_button(&mut self, point: Point, width: u32, height: u32, text: &str);
    fn draw_progress(&mut self, point: Point, width: u32, height: u32, progress: Fixed);
    fn draw_checkbox(&mut self, point: Point, width: u32, height: u32, checked: bool);
    fn draw_radio(&mut self, point: Point, width: u32, height: u32, selected: bool);
    fn draw_slider(&mut self, point: Point, width: u32, height: u32, value: Fixed);
    fn draw_scrollbar(&mut self, point: Point, width: u32, height: u32, value: Fixed);
    fn draw_menu(&mut self, point: Point, width: u32, height: u32, items: &[&str]);
    fn draw_toolbar(&mut self, point: Point, width: u32, height: u32, items: &[&str]);
    fn draw_statusbar(&mut self, point: Point, width: u32, height: u32, text: &str);
    fn draw_textbox(&mut self, point: Point, width: u32, height: u32, text: &str);
    fn draw_list(&mut self, point: Point, width: u32, height: u32, items: &[&str]);
    fn draw_tree(&mut self, point: Point, width: u32, height: u32, nodes: &[&str]);
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self::rgb(r, g, b)
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const RED: Self = Self::rgb(255, 0, 0);
    pub const GREEN: Self = Self::rgb(0, 255, 0);
    pub const BLUE: Self = Self::rgb(0, 0, 255);
    pub const YELLOW: Self = Self::rgb(255, 255, 0);
    pub const CYAN: Self = Self::rgb(0, 255, 255);
    pub const MAGENTA: Self = Self::rgb(255, 0, 255);
    pub const GRAY: Self = Self::rgb(128, 128, 128);
    pub const LIGHT_GRAY: Self = Self::rgb(211, 211, 211);
    pub const DARK_GRAY: Self = Self::rgb(64, 64, 64);
    pub const SILVER: Self = Self::rgb(192, 192, 192);
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    pub fn premultiply(self) -> Self {
        let a = self.a as u16;
        Self {
            r: ((self.r as u16 * a + 127) / 255) as u8,
            g: ((self.g as u16 * a + 127) / 255) as u8,
            b: ((self.b as u16 * a + 127) / 255) as u8,
            a: self.a,
        }
    }

    pub fn lerp(a: Self, b: Self, t: Fixed) -> Self {
        let t = t.clamp(0, FIXED_ONE);
        let it = FIXED_ONE - t;

        let lerp_u8 = |av: u8, bv: u8| -> u8 {
            let v = fixed_mul((av as i32) << FIXED_SHIFT, it)
                + fixed_mul((bv as i32) << FIXED_SHIFT, t);
            fixed_to_int(v).clamp(0, 255) as u8
        };

        Self {
            r: lerp_u8(a.r, b.r),
            g: lerp_u8(a.g, b.g),
            b: lerp_u8(a.b, b.b),
            a: lerp_u8(a.a, b.a),
        }
    }

    pub fn blend(src: Self, dst: Self) -> Self {
        let sa = src.a as u16;
        let inv_sa = 255u16.saturating_sub(sa);

        let out_a = sa + ((dst.a as u16 * inv_sa + 127) / 255);
        if out_a == 0 {
            return Self::TRANSPARENT;
        }

        let blend_chan = |sc: u8, dc: u8| -> u8 {
            let sp = (sc as u16 * sa + 127) / 255;
            let dp = (dc as u16 * dst.a as u16 + 127) / 255;
            let out_p = sp + ((dp * inv_sa + 127) / 255);
            ((out_p * 255 + out_a / 2) / out_a).clamp(0, 255) as u8
        };

        Self {
            r: blend_chan(src.r, dst.r),
            g: blend_chan(src.g, dst.g),
            b: blend_chan(src.b, dst.b),
            a: out_a.clamp(0, 255) as u8,
        }
    }
}

pub struct DisplayCapabilities {
    pub double_buffer: bool,
    pub hardware_cursor: bool,
    pub alpha_blending: bool,
    pub scaling: bool,
    pub rotation: bool,
    pub vsync: bool,
    pub max_width: u32,
    pub max_height: u32,
    pub accelerated_blit: bool,
    pub accelerated_fill: bool,
    pub supports_modesetting: bool,
    pub hardware_acceleration: bool,
    pub gamma: bool,
    pub cursor: bool,
    pub overlays: bool,
}

pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub pixel_format: PixelFormat,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct TextMetrics {
    pub width: u32,
    pub height: u32,
    pub ascent: i32,
    pub descent: i32,
    pub baseline: i32,
}

impl TextMetrics {
    pub const fn size(self) -> Size {
        Size::new(self.width, self.height)
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width - 1
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height - 1
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.width && p.y < self.y + self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let x0 = max(self.x, other.x);
        let y0 = max(self.y, other.y);
        let x1 = min(self.x + self.width, other.x + other.width);
        let y1 = min(self.y + self.height, other.y + other.height);

        Rect::new(x0, y0, max(0, x1 - x0), max(0, y1 - y0))
    }

    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }

        let x0 = min(self.x, other.x);
        let y0 = min(self.y, other.y);
        let x1 = max(self.x + self.width, other.x + other.width);
        let y1 = max(self.y + self.height, other.y + other.height);

        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    pub fn inflate(&self, dx: i32, dy: i32) -> Rect {
        let w = max(0, self.width + dx.saturating_mul(2));
        let h = max(0, self.height + dy.saturating_mul(2));
        Rect::new(self.x - dx, self.y - dy, w, h)
    }
}

#[derive(Debug, Clone)]
pub struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub pixel_format: PixelFormat,
    pub pixels: &'a [u8],
}

pub struct Font {
    pub family: &'static str,
    pub size: u16,
    pub style: FontStyle,
    pub weight: FontWeight,
}

#[derive(Debug, Clone)]
pub enum Fill {
    Solid(Color),
    LinearGradient(LinearGradient),
    RadialGradient(RadialGradient),
    Pattern(TextureId),
}

#[derive(Debug, Copy, Clone)]
pub struct GradientStop {
    pub position: Fixed,
    pub color: Color,
}

#[derive(Debug, Clone)]
pub struct LinearGradient {
    pub start: Point,
    pub end: Point,
    pub stops: &'static [GradientStop],
}

#[derive(Debug, Clone)]
pub struct RadialGradient {
    pub center: Point,
    pub radius: Fixed,
    pub stops: &'static [GradientStop],
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb,
    Bgr,
    Rgba,
    Bgra,
    Argb,
    Abgr,
    Rgb565,
    Grayscale,
}

pub enum DisplayError {
    Unsupported,
    InvalidMode,
    OutOfBounds,
    OutOfMemory,
    InvalidImage,
    HardwareFailure,
}

pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Underline,
    Strikethrough,
}

pub enum FontWeight {
    Thin,
    Light,
    Normal,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
    Black,
}

#[derive(Debug, Copy, Clone)]
pub struct Transform {
    pub translate: Point,
    pub rotation: Angle,
    pub scale_x: Fixed,
    pub scale_y: Fixed,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translate: Point::new(0, 0),
            rotation: Angle(0),
            scale_x: FIXED_ONE,
            scale_y: FIXED_ONE,
        }
    }
}

pub struct Surface<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: PixelFormat,
    pub pixels: &'a mut [u8],
}

impl Surface<'_> {
    pub fn put_pixel(&mut self, point: Point, color: Color) {
        if point.x < 0 || point.y < 0 {
            return;
        }
        let x = point.x as usize;
        let y = point.y as usize;
        if x >= self.width as usize || y >= self.height as usize {
            return;
        }

        let bpp = bytes_per_pixel(self.format);
        let idx = y.saturating_mul(self.stride).saturating_add(x.saturating_mul(bpp));
        if idx + bpp > self.pixels.len() {
            return;
        }

        match self.format {
            PixelFormat::Rgb => {
                self.pixels[idx] = color.r;
                self.pixels[idx + 1] = color.g;
                self.pixels[idx + 2] = color.b;
            }
            PixelFormat::Bgr => {
                self.pixels[idx] = color.b;
                self.pixels[idx + 1] = color.g;
                self.pixels[idx + 2] = color.r;
            }
            PixelFormat::Rgba => {
                self.pixels[idx] = color.r;
                self.pixels[idx + 1] = color.g;
                self.pixels[idx + 2] = color.b;
                self.pixels[idx + 3] = color.a;
            }
            PixelFormat::Bgra => {
                self.pixels[idx] = color.b;
                self.pixels[idx + 1] = color.g;
                self.pixels[idx + 2] = color.r;
                self.pixels[idx + 3] = color.a;
            }
            PixelFormat::Argb => {
                self.pixels[idx] = color.a;
                self.pixels[idx + 1] = color.r;
                self.pixels[idx + 2] = color.g;
                self.pixels[idx + 3] = color.b;
            }
            PixelFormat::Abgr => {
                self.pixels[idx] = color.a;
                self.pixels[idx + 1] = color.b;
                self.pixels[idx + 2] = color.g;
                self.pixels[idx + 3] = color.r;
            }
            PixelFormat::Rgb565 => {
                let r = (color.r as u16 >> 3) & 0x1f;
                let g = (color.g as u16 >> 2) & 0x3f;
                let b = (color.b as u16 >> 3) & 0x1f;
                let raw = (r << 11) | (g << 5) | b;
                self.pixels[idx] = (raw & 0xff) as u8;
                self.pixels[idx + 1] = (raw >> 8) as u8;
            }
            PixelFormat::Grayscale => {
                let l = ((color.r as u16 * 77 + color.g as u16 * 150 + color.b as u16 * 29) >> 8) as u8;
                self.pixels[idx] = l;
            }
        }
    }

    pub fn clear(&mut self, color: Color) {
        self.fill_rect(Rect::new(0, 0, self.width as i32, self.height as i32), color);
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        if rect.is_empty() {
            return;
        }

        let bounds = Rect::new(0, 0, self.width as i32, self.height as i32);
        let r = rect.intersect(&bounds);
        if r.is_empty() {
            return;
        }

        for y in r.y..(r.y + r.height) {
            for x in r.x..(r.x + r.width) {
                self.put_pixel(Point::new(x, y), color);
            }
        }
    }

    pub fn blit(&mut self, image: &Image, dst: Point) {
        for y in 0..image.height {
            for x in 0..image.width {
                if let Some(color) = read_image_pixel(image, x, y) {
                    self.put_pixel(Point::new(dst.x + x as i32, dst.y + y as i32), color);
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct SurfaceCache {
    width: u32,
    height: u32,
    stride: usize,
    format: PixelFormat,
    ptr: *mut u8,
    len: usize,
}

pub struct SoftwareRenderer<'a, D: DisplayHal> {
    display: &'a mut D,
    clips: Vec<Rect>,
    transforms: Vec<Transform>,
    surface_cache: Option<SurfaceCache>,
}

impl<'a, D: DisplayHal> SoftwareRenderer<'a, D> {
    pub fn new(display: &'a mut D) -> Self {
        Self {
            display,
            clips: Vec::new(),
            transforms: vec![Transform::default()],
            surface_cache: None,
        }
    }

    pub fn invalidate_surface_cache(&mut self) {
        self.surface_cache = None;
    }

    fn current_transform(&self) -> Transform {
        self.transforms.last().copied().unwrap_or_default()
    }

    fn in_clip(&self, p: Point) -> bool {
        for r in &self.clips {
            if !r.contains(p) {
                return false;
            }
        }
        true
    }

    fn apply_transform(&self, p: Point) -> Point {
        let t = self.current_transform();

        let sx = fixed_mul(p.x << FIXED_SHIFT, t.scale_x);
        let sy = fixed_mul(p.y << FIXED_SHIFT, t.scale_y);

        let (sin_q15, cos_q15) = sin_cos_q15(t.rotation);
        let rx = ((sx as i64 * cos_q15 as i64 - sy as i64 * sin_q15 as i64) >> 15) as i32;
        let ry = ((sx as i64 * sin_q15 as i64 + sy as i64 * cos_q15 as i64) >> 15) as i32;

        Point::new(fixed_to_int(rx) + t.translate.x, fixed_to_int(ry) + t.translate.y)
    }

    #[inline]
    fn pixel(&mut self, point: Point, color: Color) {
        let transformed = self.apply_transform(point);
        if self.in_clip(transformed) {
            self.put_cached_pixel(transformed, color);
        }
    }

    fn ensure_surface_cache(&mut self) -> Option<SurfaceCache> {
        if let Some(cache) = self.surface_cache {
            return Some(cache);
        }

        let surface = self.display.surface();
        let cache = SurfaceCache {
            width: surface.width,
            height: surface.height,
            stride: surface.stride,
            format: surface.format,
            ptr: surface.pixels.as_ptr() as *mut u8,
            len: surface.pixels.len(),
        };
        self.surface_cache = Some(cache);
        self.surface_cache
    }

    fn put_cached_pixel(&mut self, point: Point, color: Color) {
        let Some(cache) = self.ensure_surface_cache() else {
            self.display.put_pixel(point, color);
            return;
        };

        if point.x < 0 || point.y < 0 {
            return;
        }
        let x = point.x as usize;
        let y = point.y as usize;
        if x >= cache.width as usize || y >= cache.height as usize {
            return;
        }

        let bpp = bytes_per_pixel(cache.format);
        let idx = y.saturating_mul(cache.stride).saturating_add(x.saturating_mul(bpp));
        if idx + bpp > cache.len {
            return;
        }

        // SAFETY: `ptr/len` are captured from a live mutable framebuffer slice
        // and all access is bounds-checked against `len` above.
        let pixels = unsafe { core::slice::from_raw_parts_mut(cache.ptr, cache.len) };

        match cache.format {
            PixelFormat::Rgb => {
                pixels[idx] = color.r;
                pixels[idx + 1] = color.g;
                pixels[idx + 2] = color.b;
            }
            PixelFormat::Bgr => {
                pixels[idx] = color.b;
                pixels[idx + 1] = color.g;
                pixels[idx + 2] = color.r;
            }
            PixelFormat::Rgba => {
                pixels[idx] = color.r;
                pixels[idx + 1] = color.g;
                pixels[idx + 2] = color.b;
                pixels[idx + 3] = color.a;
            }
            PixelFormat::Bgra => {
                pixels[idx] = color.b;
                pixels[idx + 1] = color.g;
                pixels[idx + 2] = color.r;
                pixels[idx + 3] = color.a;
            }
            PixelFormat::Argb => {
                pixels[idx] = color.a;
                pixels[idx + 1] = color.r;
                pixels[idx + 2] = color.g;
                pixels[idx + 3] = color.b;
            }
            PixelFormat::Abgr => {
                pixels[idx] = color.a;
                pixels[idx + 1] = color.b;
                pixels[idx + 2] = color.g;
                pixels[idx + 3] = color.r;
            }
            PixelFormat::Rgb565 => {
                let r = (color.r as u16 >> 3) & 0x1f;
                let g = (color.g as u16 >> 2) & 0x3f;
                let b = (color.b as u16 >> 3) & 0x1f;
                let raw = (r << 11) | (g << 5) | b;
                pixels[idx] = (raw & 0xff) as u8;
                pixels[idx + 1] = (raw >> 8) as u8;
            }
            PixelFormat::Grayscale => {
                let l = ((color.r as u16 * 77 + color.g as u16 * 150 + color.b as u16 * 29) >> 8) as u8;
                pixels[idx] = l;
            }
        }
    }
}

impl<D: DisplayHal> Renderer for SoftwareRenderer<'_, D> {
    fn draw_text(&mut self, point: Point, text: &str, font: &Font, color: Color) {
        let scale = max(1, font.size as i32 / 8);
        let mut x = point.x;

        for ch in text.chars() {
            if ch == '\n' {
                x = point.x;
                continue;
            }
            draw_glyph(self, Point::new(x, point.y), ch, color, scale);
            x += (GLYPH_WIDTH + 1) * scale;
        }
    }

    fn measure_text(&self, text: &str, font: &Font) -> TextMetrics {
        let scale = max(1, font.size as i32 / 8);
        let count = text.chars().count() as i32;
        let width = if count <= 0 {
            0
        } else {
            count * (GLYPH_WIDTH + 1) * scale - scale
        };
        let height = GLYPH_HEIGHT * scale;

        TextMetrics {
            width: width.max(0) as u32,
            height: height.max(0) as u32,
            ascent: (GLYPH_HEIGHT - 1) * scale,
            descent: scale,
            baseline: (GLYPH_HEIGHT - 1) * scale,
        }
    }

    fn draw_image(&mut self, point: Point, image: &Image) {
        for y in 0..image.height as i32 {
            for x in 0..image.width as i32 {
                if let Some(color) = read_image_pixel(image, x as u32, y as u32) {
                    self.pixel(Point::new(point.x + x, point.y + y), color);
                }
            }
        }
    }

    fn draw_pixel(&mut self, point: Point, color: Color) {
        self.pixel(point, color);
    }

    fn draw_line(&mut self, p0: Point, p1: Point, color: Color) {
        let dx = (p1.x - p0.x).abs();
        let dy = (p1.y - p0.y).abs();
        let sx = if p0.x < p1.x { 1 } else { -1 };
        let sy = if p0.y < p1.y { 1 } else { -1 };
        let mut err = dx - dy;

        let mut x0 = p0.x;
        let mut y0 = p0.y;
        loop {
            self.pixel(Point::new(x0, y0), color);
            if x0 == p1.x && y0 == p1.y {
                break;
            }
            let e2 = err * 2;
            if e2 > -dy {
                err -= dy;
                x0 += sx;
            }
            if e2 < dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn draw_rect(&mut self, rect: &Rect, stroke: Stroke) {
        if rect.width <= 0 || rect.height <= 0 {
            return;
        }
        let x0 = rect.x;
        let y0 = rect.y;
        let x1 = rect.right();
        let y1 = rect.bottom();
        self.draw_line(Point::new(x0, y0), Point::new(x1, y0), stroke.color);
        self.draw_line(Point::new(x0, y1), Point::new(x1, y1), stroke.color);
        self.draw_line(Point::new(x0, y0), Point::new(x0, y1), stroke.color);
        self.draw_line(Point::new(x1, y0), Point::new(x1, y1), stroke.color);
    }

    fn fill_rect(&mut self, rect: &Rect, fill: Fill) {
        if rect.width <= 0 || rect.height <= 0 {
            return;
        }
        let color = match fill {
            Fill::Solid(c) => c,
            _ => Color::BLACK,
        };

        for y in rect.y..=rect.bottom() {
            for x in rect.x..=rect.right() {
                self.pixel(Point::new(x, y), color);
            }
        }
    }

    fn draw_circle(&mut self, point: Point, radius: u32, color: Color) {
        let mut x = radius as i32;
        let mut y = 0;
        let mut err = 0;

        while x >= y {
            self.pixel(Point::new(point.x + x, point.y + y), color);
            self.pixel(Point::new(point.x + y, point.y + x), color);
            self.pixel(Point::new(point.x - y, point.y + x), color);
            self.pixel(Point::new(point.x - x, point.y + y), color);
            self.pixel(Point::new(point.x - x, point.y - y), color);
            self.pixel(Point::new(point.x - y, point.y - x), color);
            self.pixel(Point::new(point.x + y, point.y - x), color);
            self.pixel(Point::new(point.x + x, point.y - y), color);

            if err <= 0 {
                y += 1;
                err += 2 * y + 1;
            }
            if err > 0 {
                x -= 1;
                err -= 2 * x + 1;
            }
        }
    }

    fn fill_circle(&mut self, point: Point, radius: u32, color: Color) {
        let mut x = radius as i32;
        let mut y = 0;
        let mut err = 0;

        while x >= y {
            for i in -x..=x {
                self.pixel(Point::new(point.x + i, point.y + y), color);
                self.pixel(Point::new(point.x + i, point.y - y), color);
            }
            for i in -y..=y {
                self.pixel(Point::new(point.x + i, point.y + x), color);
                self.pixel(Point::new(point.x + i, point.y - x), color);
            }

            if err <= 0 {
                y += 1;
                err += 2 * y + 1;
            }
            if err > 0 {
                x -= 1;
                err -= 2 * x + 1;
            }
        }
    }

    fn draw_triangle(&mut self, p1: Point, p2: Point, p3: Point, color: Color) {
        self.draw_line(p1, p2, color);
        self.draw_line(p2, p3, color);
        self.draw_line(p3, p1, color);
    }

    fn fill_triangle(&mut self, p1: Point, p2: Point, p3: Point, color: Color) {
        let mut points = [p1, p2, p3];
        points.sort_by(|a, b| a.y.cmp(&b.y));
        let (a, b, c) = (points[0], points[1], points[2]);

        let total_height = c.y - a.y;
        if total_height <= 0 {
            return;
        }

        for i in 0..=total_height {
            let second_half = i > (b.y - a.y) || b.y == a.y;
            let segment_height = if second_half { c.y - b.y } else { b.y - a.y };
            if segment_height == 0 {
                continue;
            }

            let alpha = fixed_div(i << FIXED_SHIFT, total_height << FIXED_SHIFT);
            let beta = fixed_div(
                (i - if second_half { b.y - a.y } else { 0 }) << FIXED_SHIFT,
                segment_height << FIXED_SHIFT,
            );

            let ax = a.x + fixed_to_int(fixed_mul((c.x - a.x) << FIXED_SHIFT, alpha));
            let bx = if second_half {
                b.x + fixed_to_int(fixed_mul((c.x - b.x) << FIXED_SHIFT, beta))
            } else {
                a.x + fixed_to_int(fixed_mul((b.x - a.x) << FIXED_SHIFT, beta))
            };

            for x in min(ax, bx)..=max(ax, bx) {
                self.pixel(Point::new(x, a.y + i), color);
            }
        }
    }

    fn draw_polygon(&mut self, points: &[Point], color: Color) {
        if points.len() < 2 {
            return;
        }
        for i in 0..points.len() {
            self.draw_line(points[i], points[(i + 1) % points.len()], color);
        }
    }

    fn fill_polygon(&mut self, points: &[Point], color: Color) {
        let n = points.len();
        if n < 3 {
            return;
        }

        let mut min_y = points[0].y;
        let mut max_y = points[0].y;
        for p in points {
            min_y = min(min_y, p.y);
            max_y = max(max_y, p.y);
        }

        for y in min_y..=max_y {
            let mut intersections = Vec::new();
            for i in 0..n {
                let p1 = points[i];
                let p2 = points[(i + 1) % n];
                if (p1.y <= y && p2.y > y) || (p2.y <= y && p1.y > y) {
                    let x = p1.x + (y - p1.y) * (p2.x - p1.x) / (p2.y - p1.y);
                    intersections.push(x);
                }
            }
            intersections.sort_unstable();
            for i in (0..intersections.len()).step_by(2) {
                if i + 1 < intersections.len() {
                    for x in intersections[i]..=intersections[i + 1] {
                        self.pixel(Point::new(x, y), color);
                    }
                }
            }
        }
    }

    fn draw_bezier(&mut self, points: &[Point], color: Color) {
        if points.len() < 2 {
            return;
        }

        let n = points.len() - 1;
        let steps = 100;
        for i in 0..=steps {
            let t = fixed_div((i as i32) << FIXED_SHIFT, (steps as i32) << FIXED_SHIFT);
            let one_minus_t = FIXED_ONE - t;
            let mut x = 0i64;
            let mut y = 0i64;

            for (j, p) in points.iter().enumerate() {
                let coeff = binomial_coefficient(n, j) as i64;
                let a = fixed_pow(t, j as u32) as i64;
                let b = fixed_pow(one_minus_t, (n - j) as u32) as i64;
                let term = coeff * a * b;
                x += term * p.x as i64;
                y += term * p.y as i64;
            }

            let denom = (FIXED_ONE as i64).pow(n as u32);
            if denom != 0 {
                self.pixel(Point::new((x / denom) as i32, (y / denom) as i32), color);
            }
        }
    }

    fn draw_arc(&mut self, point: Point, radius: u32, start: Angle, end: Angle, color: Color) {
        if radius == 0 {
            return;
        }
        let steps = max(1, radius as i32 * 2);
        let delta = end.0 - start.0;

        for i in 0..=steps {
            let t = fixed_div(i << FIXED_SHIFT, steps << FIXED_SHIFT);
            let ang = start.0 + fixed_to_int(fixed_mul(delta << FIXED_SHIFT, t));
            let (sin_q15, cos_q15) = sin_cos_q15(Angle(ang));
            let x = point.x + ((radius as i32 * cos_q15) >> 15);
            let y = point.y + ((radius as i32 * sin_q15) >> 15);
            self.pixel(Point::new(x, y), color);
        }
    }

    fn draw_ellipse(&mut self, point: Point, rx: u32, ry: u32, color: Color) {
        let steps = max(64, max(rx, ry) as i32 * 2);
        for i in 0..=steps {
            let ang = (i * 360_000) / steps;
            let (sin_q15, cos_q15) = sin_cos_q15(Angle(ang));
            let x = point.x + ((rx as i32 * cos_q15) >> 15);
            let y = point.y + ((ry as i32 * sin_q15) >> 15);
            self.pixel(Point::new(x, y), color);
        }
    }

    fn fill_ellipse(&mut self, point: Point, rx: u32, ry: u32, color: Color) {
        if rx == 0 || ry == 0 {
            return;
        }
        let rx2 = (rx as i64) * (rx as i64);
        let ry2 = (ry as i64) * (ry as i64);

        for y in -(ry as i32)..=(ry as i32) {
            let yy = (y as i64) * (y as i64);
            let inside = rx2.saturating_mul(ry2.saturating_sub(yy));
            let x_extent = isqrt(inside / ry2) as i32;
            for x in -x_extent..=x_extent {
                self.pixel(Point::new(point.x + x, point.y + y), color);
            }
        }
    }

    fn set_clip(&mut self, point: Point, width: u32, height: u32) {
        self.clips.clear();
        self.clips.push(Rect::new(point.x, point.y, width as i32, height as i32));
    }

    fn reset_clip(&mut self) {
        self.clips.clear();
    }

    fn push_clip(&mut self, rect: Rect) {
        self.clips.push(rect);
    }

    fn pop_clip(&mut self) {
        let _ = self.clips.pop();
    }

    fn translate(&mut self, dx: i32, dy: i32) {
        if let Some(last) = self.transforms.last_mut() {
            last.translate.x += dx;
            last.translate.y += dy;
        }
    }

    fn rotate(&mut self, angle: Angle) {
        if let Some(last) = self.transforms.last_mut() {
            last.rotation.0 += angle.0;
        }
    }

    fn scale(&mut self, sx: Fixed, sy: Fixed) {
        if let Some(last) = self.transforms.last_mut() {
            last.scale_x = fixed_mul(last.scale_x, sx);
            last.scale_y = fixed_mul(last.scale_y, sy);
        }
    }

    fn push_transform(&mut self) {
        let t = self.current_transform();
        self.transforms.push(t);
    }

    fn pop_transform(&mut self) {
        if self.transforms.len() > 1 {
            let _ = self.transforms.pop();
        }
    }
}

impl<D: DisplayHal> WidgetRenderer for SoftwareRenderer<'_, D> {
    fn draw_window(&mut self, point: Point, width: u32, height: u32, title: &str) {
        let body = Rect::new(point.x, point.y, width as i32, height as i32);
        self.fill_rect(&body, Fill::Solid(Color::LIGHT_GRAY));

        let title_bar = Rect::new(point.x, point.y, width as i32, 20);
        self.fill_rect(&title_bar, Fill::Solid(Color::DARK_GRAY));

        let font = Font {
            family: "Bitmap",
            size: 8,
            style: FontStyle::Regular,
            weight: FontWeight::Bold,
        };
        self.draw_text(Point::new(point.x + 6, point.y + 6), title, &font, Color::WHITE);
    }

    fn draw_button(&mut self, point: Point, width: u32, height: u32, text: &str) {
        let rect = Rect::new(point.x, point.y, width as i32, height as i32);
        self.fill_rect(&rect, Fill::Solid(Color::GRAY));
        self.draw_rect(
            &rect,
            Stroke {
                color: Color::BLACK,
                width: 1,
                style: StrokeStyle::Solid,
                cap: StrokeCap::Butt,
                join: StrokeJoin::Miter,
            },
        );

        let font = Font {
            family: "Bitmap",
            size: 8,
            style: FontStyle::Regular,
            weight: FontWeight::Normal,
        };
        let m = self.measure_text(text, &font);
        let tx = point.x + (width as i32 - m.width as i32) / 2;
        let ty = point.y + (height as i32 - m.height as i32) / 2;
        self.draw_text(Point::new(tx, ty), text, &font, Color::WHITE);
    }

    fn draw_progress(&mut self, point: Point, width: u32, height: u32, progress: Fixed) {
        let rect = Rect::new(point.x, point.y, width as i32, height as i32);
        self.fill_rect(&rect, Fill::Solid(Color::LIGHT_GRAY));

        let p = progress.clamp(0, FIXED_ONE);
        let pw = fixed_to_int(fixed_mul((width as i32) << FIXED_SHIFT, p)) as i32;
        let p_rect = Rect::new(point.x, point.y, pw, height as i32);
        self.fill_rect(&p_rect, Fill::Solid(Color::GREEN));
    }

    fn draw_checkbox(&mut self, point: Point, width: u32, height: u32, checked: bool) {
        let rect = Rect::new(point.x, point.y, width as i32, height as i32);
        self.fill_rect(&rect, Fill::Solid(Color::WHITE));
        self.draw_rect(
            &rect,
            Stroke {
                color: Color::BLACK,
                width: 1,
                style: StrokeStyle::Solid,
                cap: StrokeCap::Butt,
                join: StrokeJoin::Miter,
            },
        );
        if checked && width > 4 && height > 4 {
            self.fill_rect(
                &Rect::new(point.x + 2, point.y + 2, width as i32 - 4, height as i32 - 4),
                Fill::Solid(Color::BLACK),
            );
        }
    }

    fn draw_radio(&mut self, point: Point, width: u32, height: u32, selected: bool) {
        let r = width.min(height) / 2;
        let c = Point::new(point.x + width as i32 / 2, point.y + height as i32 / 2);
        self.draw_circle(c, r, Color::BLACK);
        if selected {
            self.fill_circle(c, r / 2, Color::BLACK);
        }
    }

    fn draw_slider(&mut self, point: Point, width: u32, height: u32, value: Fixed) {
        let track = Rect::new(point.x, point.y + height as i32 / 2 - 2, width as i32, 4);
        self.fill_rect(&track, Fill::Solid(Color::DARK_GRAY));

        let v = value.clamp(0, FIXED_ONE);
        let knob_x = point.x + fixed_to_int(fixed_mul((width.saturating_sub(height) as i32) << FIXED_SHIFT, v));
        let knob = Rect::new(knob_x, point.y, height as i32, height as i32);
        self.fill_rect(&knob, Fill::Solid(Color::SILVER));
    }

    fn draw_scrollbar(&mut self, point: Point, width: u32, height: u32, value: Fixed) {
        let rect = Rect::new(point.x, point.y, width as i32, height as i32);
        self.fill_rect(&rect, Fill::Solid(Color::LIGHT_GRAY));

        let thumb_h = max(8, (height as i32) / 5);
        let max_off = max(0, height as i32 - thumb_h);
        let v = value.clamp(0, FIXED_ONE);
        let thumb_y = point.y + fixed_to_int(fixed_mul(max_off << FIXED_SHIFT, v));
        self.fill_rect(
            &Rect::new(point.x, thumb_y, width as i32, thumb_h),
            Fill::Solid(Color::DARK_GRAY),
        );
    }

    fn draw_menu(&mut self, point: Point, width: u32, height: u32, items: &[&str]) {
        let rect = Rect::new(point.x, point.y, width as i32, height as i32);
        self.fill_rect(&rect, Fill::Solid(Color::WHITE));

        let font = Font {
            family: "Bitmap",
            size: 8,
            style: FontStyle::Regular,
            weight: FontWeight::Normal,
        };

        let mut x = point.x + 4;
        for item in items {
            self.draw_text(Point::new(x, point.y + 4), item, &font, Color::BLACK);
            x += self.measure_text(item, &font).width as i32 + 10;
        }
    }

    fn draw_toolbar(&mut self, point: Point, width: u32, height: u32, items: &[&str]) {
        self.fill_rect(
            &Rect::new(point.x, point.y, width as i32, height as i32),
            Fill::Solid(Color::SILVER),
        );
        let mut x = point.x + 4;
        for item in items {
            self.draw_button(Point::new(x, point.y + 2), 60, height.saturating_sub(4), item);
            x += 64;
        }
    }

    fn draw_statusbar(&mut self, point: Point, width: u32, height: u32, text: &str) {
        self.fill_rect(
            &Rect::new(point.x, point.y, width as i32, height as i32),
            Fill::Solid(Color::LIGHT_GRAY),
        );

        let font = Font {
            family: "Bitmap",
            size: 8,
            style: FontStyle::Regular,
            weight: FontWeight::Normal,
        };
        self.draw_text(Point::new(point.x + 4, point.y + 4), text, &font, Color::BLACK);
    }

    fn draw_textbox(&mut self, point: Point, width: u32, height: u32, text: &str) {
        let rect = Rect::new(point.x, point.y, width as i32, height as i32);
        self.fill_rect(&rect, Fill::Solid(Color::WHITE));
        self.draw_rect(
            &rect,
            Stroke {
                color: Color::BLACK,
                width: 1,
                style: StrokeStyle::Solid,
                cap: StrokeCap::Butt,
                join: StrokeJoin::Miter,
            },
        );

        let font = Font {
            family: "Bitmap",
            size: 8,
            style: FontStyle::Regular,
            weight: FontWeight::Normal,
        };
        self.draw_text(Point::new(point.x + 4, point.y + 4), text, &font, Color::BLACK);
    }

    fn draw_list(&mut self, point: Point, width: u32, height: u32, items: &[&str]) {
        self.fill_rect(
            &Rect::new(point.x, point.y, width as i32, height as i32),
            Fill::Solid(Color::WHITE),
        );

        let font = Font {
            family: "Bitmap",
            size: 8,
            style: FontStyle::Regular,
            weight: FontWeight::Normal,
        };

        let mut y = point.y + 4;
        for item in items {
            self.draw_text(Point::new(point.x + 4, y), item, &font, Color::BLACK);
            y += 10;
            if y > point.y + height as i32 - 8 {
                break;
            }
        }
    }

    fn draw_tree(&mut self, point: Point, width: u32, height: u32, nodes: &[&str]) {
        self.fill_rect(
            &Rect::new(point.x, point.y, width as i32, height as i32),
            Fill::Solid(Color::WHITE),
        );

        let font = Font {
            family: "Bitmap",
            size: 8,
            style: FontStyle::Regular,
            weight: FontWeight::Normal,
        };

        let mut y = point.y + 4;
        for node in nodes {
            self.draw_text(Point::new(point.x + 12, y), node, &font, Color::BLACK);
            self.draw_line(Point::new(point.x + 4, y + 3), Point::new(point.x + 10, y + 3), Color::BLACK);
            y += 10;
            if y > point.y + height as i32 - 8 {
                break;
            }
        }
    }
}

pub fn binomial_coefficient(n: usize, k: usize) -> u64 {
    if k > n {
        return 0;
    }
    if k == 0 || k == n {
        return 1;
    }
    let k = min(k, n - k);
    let mut result = 1u64;
    for i in 0..k {
        result = result * (n - i) as u64 / (i as u64 + 1);
    }
    result
}

fn fixed_pow(mut base: Fixed, mut exp: u32) -> Fixed {
    let mut result = FIXED_ONE;
    while exp > 0 {
        if (exp & 1) != 0 {
            result = fixed_mul(result, base);
        }
        base = fixed_mul(base, base);
        exp >>= 1;
    }
    result
}

fn read_image_pixel(image: &Image, x: u32, y: u32) -> Option<Color> {
    if x >= image.width || y >= image.height {
        return None;
    }

    let bpp = bytes_per_pixel(image.pixel_format);
    let idx = y as usize * image.stride + x as usize * bpp;
    if idx + bpp > image.pixels.len() {
        return None;
    }

    Some(match image.pixel_format {
        PixelFormat::Rgb => {
            let p = &image.pixels[idx..idx + 3];
            Color::rgb(p[0], p[1], p[2])
        }
        PixelFormat::Bgr => {
            let p = &image.pixels[idx..idx + 3];
            Color::rgb(p[2], p[1], p[0])
        }
        PixelFormat::Rgba => {
            let p = &image.pixels[idx..idx + 4];
            Color::rgba(p[0], p[1], p[2], p[3])
        }
        PixelFormat::Bgra => {
            let p = &image.pixels[idx..idx + 4];
            Color::rgba(p[2], p[1], p[0], p[3])
        }
        PixelFormat::Argb => {
            let p = &image.pixels[idx..idx + 4];
            Color::rgba(p[1], p[2], p[3], p[0])
        }
        PixelFormat::Abgr => {
            let p = &image.pixels[idx..idx + 4];
            Color::rgba(p[3], p[2], p[1], p[0])
        }
        PixelFormat::Rgb565 => {
            let raw = u16::from_le_bytes([image.pixels[idx], image.pixels[idx + 1]]);
            let r = ((raw >> 11) & 0x1f) as u8;
            let g = ((raw >> 5) & 0x3f) as u8;
            let b = (raw & 0x1f) as u8;
            Color::rgb((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2))
        }
        PixelFormat::Grayscale => {
            let v = image.pixels[idx];
            Color::rgb(v, v, v)
        }
    })
}

fn bytes_per_pixel(format: PixelFormat) -> usize {
    match format {
        PixelFormat::Rgb | PixelFormat::Bgr => 3,
        PixelFormat::Rgba | PixelFormat::Bgra | PixelFormat::Argb | PixelFormat::Abgr => 4,
        PixelFormat::Rgb565 => 2,
        PixelFormat::Grayscale => 1,
    }
}

fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) >> 1;
    while y < x {
        x = y;
        y = (x + n / x) >> 1;
    }
    x
}

fn sin_cos_q15(angle: Angle) -> (i32, i32) {
    // Integer-only approximation in Q15.
    let mut deg = angle.0 % 360_000;
    if deg < 0 {
        deg += 360_000;
    }

    // Convert to [0, 180000] with sign tracking.
    let (deg180, sin_sign) = if deg <= 180_000 {
        (deg, 1)
    } else {
        (360_000 - deg, -1)
    };

    let sin = sin_bhaskara_q15(deg180) * sin_sign;

    let cos_deg = if deg <= 90_000 {
        90_000 - deg
    } else if deg <= 270_000 {
        deg - 90_000
    } else {
        450_000 - deg
    };
    let cos_sign = if deg <= 90_000 || deg >= 270_000 { 1 } else { -1 };
    let cos = sin_bhaskara_q15(cos_deg) * cos_sign;

    (sin, cos)
}

fn sin_bhaskara_q15(deg_milli: i32) -> i32 {
    // sin(x) ~= 4x(180-x)/(40500-x(180-x)), x in degrees.
    let x = deg_milli as i64;
    let pi180 = 180_000i64;
    let num = 4i64 * x * (pi180 - x);
    let den = 40_500_000_000i64 - x * (pi180 - x);
    if den == 0 {
        return 0;
    }
    ((num << 15) / den) as i32
}

const GLYPH_WIDTH: i32 = 5;
const GLYPH_HEIGHT: i32 = 7;

fn draw_glyph<D: DisplayHal>(
    renderer: &mut SoftwareRenderer<'_, D>,
    origin: Point,
    ch: char,
    color: Color,
    scale: i32,
) {
    let rows = glyph_rows(ch);
    for (row, bits) in rows.iter().enumerate() {
        for col in 0..GLYPH_WIDTH as usize {
            let mask = 1u8 << (GLYPH_WIDTH as usize - 1 - col);
            if (bits & mask) != 0 {
                for sy in 0..scale {
                    for sx in 0..scale {
                        renderer.pixel(
                            Point::new(
                                origin.x + col as i32 * scale + sx,
                                origin.y + row as i32 * scale + sy,
                            ),
                            color,
                        );
                    }
                }
            }
        }
    }
}

fn glyph_rows(ch: char) -> [u8; 7] {
    let c = if ch.is_ascii_lowercase() {
        (ch as u8 - b'a' + b'A') as char
    } else {
        ch
    };

    match c {
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100],
        '.' => [0, 0, 0, 0, 0, 0b00110, 0b00110],
        ',' => [0, 0, 0, 0, 0, 0b00110, 0b00100],
        ':' => [0, 0b00110, 0b00110, 0, 0b00110, 0b00110, 0],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0b11111],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0, 0],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b00110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b11100],
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '?' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00100, 0, 0b00100],
        _ => [0b11111, 0b10001, 0b00110, 0b00100, 0b00110, 0b10001, 0b11111],
    }
}
