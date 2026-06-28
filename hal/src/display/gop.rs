use crate::display::{Color, DisplayHal, DisplayMode, PixelFormat, Point, Surface};

#[derive(Debug, Copy, Clone)]
pub struct GopDisplay {
	base: *mut u8,
	width: u32,
	height: u32,
	stride: usize,
	bpp: u8,
	format: PixelFormat,
}

impl GopDisplay {
	pub const unsafe fn from_raw(
		base: *mut u8,
		width: u32,
		height: u32,
		stride: usize,
		bpp: u8,
		format: PixelFormat,
	) -> Self {
		Self {
			base,
			width,
			height,
			stride,
			bpp,
			format,
		}
	}

	pub const fn width(&self) -> u32 {
		self.width
	}

	pub const fn height(&self) -> u32 {
		self.height
	}

	pub const fn stride(&self) -> usize {
		self.stride
	}

	pub const fn bytes_per_pixel(&self) -> usize {
		(self.bpp as usize) / 8
	}

	pub const fn frame_len(&self) -> usize {
		self.stride * self.height as usize * self.bytes_per_pixel()
	}

	fn in_bounds(&self, point: Point) -> bool {
		point.x >= 0
			&& point.y >= 0
			&& (point.x as u32) < self.width
			&& (point.y as u32) < self.height
	}

	fn pixel_offset(&self, x: usize, y: usize) -> usize {
		(y * self.stride + x) * self.bytes_per_pixel()
	}

	fn write_color(bytes: &mut [u8], format: PixelFormat, color: Color) {
		match format {
			PixelFormat::Rgb => {
				bytes[0] = color.r;
				bytes[1] = color.g;
				bytes[2] = color.b;
			}
			PixelFormat::Bgr => {
				bytes[0] = color.b;
				bytes[1] = color.g;
				bytes[2] = color.r;
			}
		}

		if bytes.len() > 3 {
			bytes[3] = color.a;
		}
	}
}

impl DisplayHal for GopDisplay {
	fn width(&self) -> u32 {
		self.width()
	}

	fn height(&self) -> u32 {
		self.height()
	}

	fn stride(&self) -> usize {
		self.stride()
	}

	fn bytes_per_pixel(&self) -> usize {
		self.bytes_per_pixel()
	}

	fn frame_len(&self) -> usize {
		self.frame_len()
	}

	fn frame_bytes_mut(&mut self) -> &mut [u8] {
		unsafe { core::slice::from_raw_parts_mut(self.base, self.frame_len()) }
	}

	fn clear(&mut self, color: Color) {
		let bpp = self.bytes_per_pixel();
		if bpp < 3 {
			return;
		}

		let format = self.format;
		for px in self.frame_bytes_mut().chunks_exact_mut(bpp) {
			Self::write_color(px, format, color);
		}
	}

	fn put_pixel(&mut self, point: Point, color: Color) {
		if !self.in_bounds(point) {
			return;
		}

		let offset = self.pixel_offset(point.x as usize, point.y as usize);
		let bpp = self.bytes_per_pixel();
		let format = self.format;
		if offset + bpp > self.frame_len() {
			return;
		}
		let px = &mut self.frame_bytes_mut()[offset..offset + bpp];
		Self::write_color(px, format, color);
	}

	fn surface(&mut self) -> Surface<'_> {
		let width = self.width;
		let height = self.height;
		let stride = self.stride * self.bytes_per_pixel();
		let format = self.format;
		let pixels = self.frame_bytes_mut();
		Surface {
			width,
			height,
			stride,
			format,
			pixels,
		}
	}

	fn present(&mut self) {}

	fn current_mode(&self) -> DisplayMode {
		DisplayMode {
			width: self.width,
			height: self.height,
			stride: self.stride,
			bpp: self.bpp,
			format: self.format,
		}
	}
}
