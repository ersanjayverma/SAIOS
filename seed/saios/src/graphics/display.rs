use core::ptr;
use efi_main::graphics::{FramebufferInfo, PixelFormat};

use crate::vmm;

/// Abstract drawing target that owns a linear framebuffer.
///
/// A `Display` describes the geometry and pixel layout of a screen and knows
/// how to copy a source image (a slice of packed 0x00RRGGBB `u32` pixels) onto
/// the real hardware framebuffer. The renderer/console layers draw into an
/// off-screen [`crate::graphics::surface::Surface`] and then hand the finished
/// pixels to [`Display::flush`], keeping all format-specific conversion in one
/// place.
pub trait Display {
    /// Number of visible pixels per row.
    fn width(&self) -> usize;
    /// Number of visible pixel rows.
    fn height(&self) -> usize;
    /// Number of pixels between the start of one row and the next. This can be
    /// larger than [`Display::width`] when the firmware pads each scanline.
    fn stride(&self) -> usize;
    /// Number of bytes each pixel occupies in the hardware framebuffer.
    fn bytes_per_pixel(&self) -> usize;
    /// Total size of the hardware framebuffer in bytes; used to bound all writes.
    fn framebuffer_size(&self) -> usize;
    /// The pixel encoding used by the hardware (RGB, BGR, bitmask, ...).
    fn pixel_format(&self) -> PixelFormat;
    /// The red/green/blue/reserved channel masks for [`PixelFormat::Bitmask`].
    fn pixel_masks(&self) -> (u32, u32, u32, u32);
    /// Raw mutable pointer to the start of the hardware framebuffer.
    fn framebuffer(&mut self) -> *mut u8;
    /// Copy a full off-screen image onto the framebuffer, converting each pixel
    /// to the hardware format as needed. `src_width`/`src_height` describe the
    /// source image dimensions so rows can be indexed correctly.
    fn flush(&mut self, pixels: &[u32], src_width: usize, src_height: usize);
}

/// Concrete [`Display`] backed by the UEFI Graphics Output Protocol framebuffer.
///
/// Construct one with [`FramebufferDisplay::from_info`], which validates the
/// firmware-reported geometry and normalizes odd pixel formats into a form the
/// fast paths can handle.
pub struct FramebufferDisplay {
    base: *mut u8,
    width: usize,
    height: usize,
    stride: usize,
    bytes_per_pixel: usize,
    size_bytes: usize,
    pixel_format: PixelFormat,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    reserved_mask: u32,
}

impl FramebufferDisplay {
    fn build_from_info(info: FramebufferInfo, mapped_base: u64) -> Option<Self> {
        let bytes_per_pixel = (info.bpp.saturating_add(7)) / 8;
        if mapped_base == 0
            || info.width == 0
            || info.height == 0
            || bytes_per_pixel == 0
            || bytes_per_pixel > 4
        {
            return None;
        }

        let mut stride_pixels = info.stride;
        let pixels_layout_ok = stride_pixels
            .checked_mul(info.height)
            .and_then(|v| v.checked_mul(bytes_per_pixel))
            .map(|v| v <= info.size)
            .unwrap_or(false);

        if !pixels_layout_ok {
            if info.stride.is_multiple_of(bytes_per_pixel) {
                let stride_from_bytes = info.stride / bytes_per_pixel;
                let bytes_layout_ok = info
                    .stride
                    .checked_mul(info.height)
                    .map(|v| v <= info.size)
                    .unwrap_or(false);
                if bytes_layout_ok && stride_from_bytes >= info.width {
                    stride_pixels = stride_from_bytes;
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }

        let min_visible = stride_pixels
            .checked_mul(info.height)?
            .checked_mul(bytes_per_pixel)?;
        if info.size < min_visible {
            return None;
        }

        let (pixel_format, red_mask, green_mask, blue_mask, reserved_mask) = Self::normalize_format(
            info.pixel_format,
            bytes_per_pixel,
            info.red_mask,
            info.green_mask,
            info.blue_mask,
            info.reserved_mask,
        );

        Some(Self {
            base: mapped_base as *mut u8,
            width: info.width,
            height: info.height,
            stride: stride_pixels,
            bytes_per_pixel,
            size_bytes: info.size,
            pixel_format,
            red_mask,
            green_mask,
            blue_mask,
            reserved_mask,
        })
    }

    /// Convert a logical 0x00RRGGBB color into the 32-bit word that must be
    /// written to a 4-byte hardware pixel for this display's format.
    #[inline(always)]
    fn rgb_to_native_u32(&self, color: u32) -> u32 {
        match self.pixel_format {
            PixelFormat::Bgr => color & 0x00FF_FFFF,
            PixelFormat::Rgb => {
                let r = (color >> 16) & 0xFF;
                let g = color & 0x0000_FF00;
                let b = (color & 0xFF) << 16;
                r | g | b
            }
            PixelFormat::Bitmask => {
                let r = ((color >> 16) & 0xFF) as u8;
                let g = ((color >> 8) & 0xFF) as u8;
                let b = (color & 0xFF) as u8;
                self.pack_bitmask(r, g, b)
            }
            PixelFormat::BltOnly => color & 0x00FF_FFFF,
        }
    }

    /// Pick reasonable red/green/blue/reserved channel masks for a plain
    /// RGB/BGR format at a given pixel size, used when the firmware reports a
    /// packed 16-bit layout but no explicit masks.
    #[inline(always)]
    fn infer_masks_for_format(format: PixelFormat, bytes_per_pixel: usize) -> (u32, u32, u32, u32) {
        match (format, bytes_per_pixel) {
            (PixelFormat::Rgb, 2) => (0x001F, 0x07E0, 0xF800, 0),
            (PixelFormat::Bgr, 2) => (0xF800, 0x07E0, 0x001F, 0),
            (PixelFormat::Rgb, 3) | (PixelFormat::Rgb, 4) => {
                (0x000000FF, 0x0000FF00, 0x00FF0000, 0)
            }
            (PixelFormat::Bgr, 3) | (PixelFormat::Bgr, 4) => {
                (0x00FF0000, 0x0000FF00, 0x000000FF, 0)
            }
            _ => (0, 0, 0, 0),
        }
    }

    /// Collapse firmware-reported formats into a canonical form. In particular
    /// 16-bit RGB/BGR and mask-less bitmask modes are turned into a
    /// [`PixelFormat::Bitmask`] with concrete channel masks so the write paths
    /// only have to handle a small number of cases.
    #[inline(always)]
    fn normalize_format(
        pixel_format: PixelFormat,
        bytes_per_pixel: usize,
        red_mask: u32,
        green_mask: u32,
        blue_mask: u32,
        reserved_mask: u32,
    ) -> (PixelFormat, u32, u32, u32, u32) {
        let masks_zero = red_mask == 0 && green_mask == 0 && blue_mask == 0;

        match pixel_format {
            PixelFormat::Bitmask => {
                if masks_zero {
                    let inferred = if bytes_per_pixel == 2 {
                        (0xF800, 0x07E0, 0x001F, 0)
                    } else {
                        (0x00FF0000, 0x0000FF00, 0x000000FF, 0)
                    };
                    (
                        PixelFormat::Bitmask,
                        inferred.0,
                        inferred.1,
                        inferred.2,
                        inferred.3,
                    )
                } else {
                    (pixel_format, red_mask, green_mask, blue_mask, reserved_mask)
                }
            }
            PixelFormat::Rgb | PixelFormat::Bgr => {
                if bytes_per_pixel == 2 {
                    let inferred = Self::infer_masks_for_format(pixel_format, bytes_per_pixel);
                    (
                        PixelFormat::Bitmask,
                        inferred.0,
                        inferred.1,
                        inferred.2,
                        inferred.3,
                    )
                } else {
                    (pixel_format, red_mask, green_mask, blue_mask, reserved_mask)
                }
            }
            PixelFormat::BltOnly => (
                PixelFormat::BltOnly,
                red_mask,
                green_mask,
                blue_mask,
                reserved_mask,
            ),
        }
    }

    /// Build a display from firmware-provided framebuffer info.
    ///
    /// Returns `None` when the geometry is unusable (null base, zero size, or a
    /// pixel depth outside 1..=4 bytes). Some firmware reports the scanline
    /// stride in bytes instead of pixels; this routine detects and corrects
    /// that, and rejects layouts that would not fit within the reported
    /// framebuffer size.
    pub fn from_info(info: FramebufferInfo) -> Option<Self> {
        let framebuffer_pages = ((info.size as u64).div_ceil(4096)).max(1) as usize;
        let mapped_base = vmm::map_physical_anywhere(
            info.base,
            framebuffer_pages,
            vmm::FLAG_READ | vmm::FLAG_WRITE | vmm::FLAG_GLOBAL | vmm::FLAG_WRITE_COMBINE,
            "framebuffer",
        )
        .ok()?;

        Self::build_from_info(info, mapped_base)
    }

    /// Build a display directly from the bootloader-provided framebuffer pointer
    /// without creating a new VMM mapping. Used during firmware-CR3 fallback.
    pub fn from_info_direct(info: FramebufferInfo) -> Option<Self> {
        Self::build_from_info(info, info.base)
    }

    /// Scale an 8-bit channel value into an arbitrary bit field described by
    /// `mask` and shift it into place. Used to build bitmask-format pixels.
    #[inline(always)]
    fn pack_channel(value: u8, mask: u32) -> u32 {
        if mask == 0 {
            return 0;
        }

        let shift = mask.trailing_zeros();
        let width = mask.count_ones();
        if width == 0 {
            return 0;
        }

        let max = (1u32 << width) - 1;
        let scaled = ((value as u32) * max + 127) / 255;
        (scaled << shift) & mask
    }

    /// Assemble a full bitmask-format pixel from 8-bit R/G/B components using
    /// this display's channel masks.
    #[inline(always)]
    fn pack_bitmask(&self, r: u8, g: u8, b: u8) -> u32 {
        Self::pack_channel(r, self.red_mask)
            | Self::pack_channel(g, self.green_mask)
            | Self::pack_channel(b, self.blue_mask)
            | self.reserved_mask
    }

    /// Convert a logical 0x00RRGGBB color into the little-endian byte sequence
    /// to store for a single hardware pixel (up to 4 bytes wide).
    #[inline(always)]
    fn native_pixel_bytes(&self, color: u32) -> [u8; 4] {
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;

        match self.pixel_format {
            PixelFormat::Rgb => [r, g, b, 0],
            PixelFormat::Bgr => [b, g, r, 0],
            PixelFormat::Bitmask => self.pack_bitmask(r, g, b).to_le_bytes(),
            PixelFormat::BltOnly => [b, g, r, 0],
        }
    }

    /// Write a source row of 0x00RRGGBB pixels to the framebuffer in 32-bit
    /// RGB format.  The conversion is done in small stack chunks so a single
    /// row can be arbitrarily wide without heap allocation, and the whole
    /// chunk is copied with `copy_nonoverlapping` instead of one `ptr::write`
    /// per pixel.
    unsafe fn write_rgb_row(&self, src_row: &[u32], dst_offset: usize) {
        const CHUNK: usize = 256;
        let mut buf = [0u32; CHUNK];

        let mut written = 0usize;
        while written < src_row.len() {
            let end = core::cmp::min(written.saturating_add(CHUNK), src_row.len());
            let chunk = &src_row[written..end];
            for (i, &px) in chunk.iter().enumerate() {
                buf[i] = self.rgb_to_native_u32(px);
            }
            let bytes = chunk.len().saturating_mul(4);
            if bytes == 0 {
                break;
            }
            unsafe {
                ptr::copy_nonoverlapping(
                    buf.as_ptr().cast::<u8>(),
                    self.base
                        .add(dst_offset.saturating_add(written.saturating_mul(4))),
                    bytes,
                );
            }
            written += chunk.len();
        }
    }

    /// Fill the entire visible framebuffer with a single color.
    ///
    /// For 32-bit RGB/BGR displays this uses a wide slice fill (one `u32` write
    /// per pixel via `slice::fill`, which the compiler lowers to a fast memory
    /// set), taking a whole-buffer shortcut when the scanline stride matches the
    /// visible width. Other formats fall back to a per-pixel byte writer.
    pub fn clear_color(&mut self, color: u32) {
        if self.bytes_per_pixel == 4
            && matches!(self.pixel_format, PixelFormat::Rgb | PixelFormat::Bgr)
        {
            let packed = self.rgb_to_native_u32(color);
            let total_pixels = self.size_bytes / 4;
            if total_pixels == 0 {
                return;
            }

            // The framebuffer is normal (write-combining) RAM; treat it as a
            // slice of u32 and let `fill` emit a tight memory-set loop.
            let fb32 =
                unsafe { core::slice::from_raw_parts_mut(self.base.cast::<u32>(), total_pixels) };

            if self.stride == self.width {
                let count = core::cmp::min(self.width.saturating_mul(self.height), total_pixels);
                fb32[..count].fill(packed);
            } else {
                for y in 0..self.height {
                    let row = y.saturating_mul(self.stride);
                    if row >= total_pixels {
                        break;
                    }
                    let end = core::cmp::min(row + self.width, total_pixels);
                    fb32[row..end].fill(packed);
                }
            }
            return;
        }

        let packed = self.native_pixel_bytes(color);
        for y in 0..self.height {
            let row_base = y * self.stride * self.bytes_per_pixel;
            for x in 0..self.width {
                let offset = row_base + x * self.bytes_per_pixel;
                if offset + self.bytes_per_pixel > self.size_bytes {
                    break;
                }
                let dst = unsafe { self.base.add(offset) };
                unsafe {
                    let mut i = 0;
                    while i < self.bytes_per_pixel {
                        // OPTIMIZATION: Use ptr::write instead of write_volatile
                        ptr::write(dst.add(i), packed[i]);
                        i += 1;
                    }
                }
            }
        }
    }

    /// Copy a sub-rectangle of an off-screen image onto the framebuffer.
    ///
    /// `src_x`/`src_y`/`width`/`height` select the region (clipped to the
    /// visible area). The BGR 32-bit case is a straight per-row
    /// `copy_nonoverlapping` memcpy; RGB and exotic formats convert per pixel.
    pub fn flush_region(
        &mut self,
        pixels: &[u32],
        src_width: usize,
        src_x: usize,
        src_y: usize,
        width: usize,
        height: usize,
    ) {
        let x_end = core::cmp::min(src_x.saturating_add(width), self.width);
        let y_end = core::cmp::min(src_y.saturating_add(height), self.height);

        if self.bytes_per_pixel == 4 && matches!(self.pixel_format, PixelFormat::Bgr) {
            let copy_width = x_end.saturating_sub(src_x);
            if copy_width == 0 {
                return;
            }

            for y in src_y..y_end {
                let src_row_start = y * src_width + src_x;
                if src_row_start >= pixels.len() {
                    break;
                }

                let src_row_len = core::cmp::min(copy_width, pixels.len() - src_row_start);
                if src_row_len == 0 {
                    continue;
                }

                let dst_offset = (y * self.stride + src_x) * 4;
                if dst_offset >= self.size_bytes {
                    continue;
                }

                let max_pixels = (self.size_bytes - dst_offset) / 4;
                let copy_pixels = core::cmp::min(src_row_len, max_pixels);
                if copy_pixels == 0 {
                    continue;
                }

                unsafe {
                    ptr::copy_nonoverlapping(
                        pixels.as_ptr().add(src_row_start).cast::<u8>(),
                        self.base.add(dst_offset),
                        copy_pixels * 4,
                    );
                }
            }
            return;
        }

        if self.bytes_per_pixel == 4 && matches!(self.pixel_format, PixelFormat::Rgb) {
            let copy_width = x_end.saturating_sub(src_x);
            if copy_width == 0 {
                return;
            }

            for y in src_y..y_end {
                let src_row_start = y * src_width + src_x;
                if src_row_start >= pixels.len() {
                    break;
                }

                let src_row_len = core::cmp::min(copy_width, pixels.len() - src_row_start);
                if src_row_len == 0 {
                    continue;
                }

                let dst_offset = (y * self.stride + src_x) * 4;
                if dst_offset >= self.size_bytes {
                    continue;
                }

                let max_pixels = (self.size_bytes - dst_offset) / 4;
                let copy_pixels = core::cmp::min(src_row_len, max_pixels);
                if copy_pixels == 0 {
                    continue;
                }

                unsafe {
                    self.write_rgb_row(
                        &pixels[src_row_start..src_row_start + copy_pixels],
                        dst_offset,
                    );
                }
            }
            return;
        }

        for y in src_y..y_end {
            for x in src_x..x_end {
                let src = pixels[y * src_width + x];
                let r = ((src >> 16) & 0xFF) as u8;
                let g = ((src >> 8) & 0xFF) as u8;
                let b = (src & 0xFF) as u8;

                let offset = (y * self.stride + x) * self.bytes_per_pixel;
                if offset + self.bytes_per_pixel > self.size_bytes {
                    continue;
                }
                unsafe {
                    let p = self.base.add(offset);
                    match self.pixel_format {
                        PixelFormat::Rgb => {
                            Self::write_rgb_like(p, r, g, b, self.bytes_per_pixel, false)
                        }
                        PixelFormat::Bgr => {
                            Self::write_rgb_like(p, r, g, b, self.bytes_per_pixel, true)
                        }
                        PixelFormat::Bitmask => {
                            Self::write_packed(p, self.pack_bitmask(r, g, b), self.bytes_per_pixel)
                        }
                        // BltOnly framebuffers do not support direct pixel
                        // access; flushing is a no-op for this format.
                        PixelFormat::BltOnly => {}
                    }
                }
            }
        }
    }

    /// Write up to `bytes_per_pixel` little-endian bytes of an already-packed
    /// pixel value to `dst`.
    #[inline(always)]
    unsafe fn write_packed(dst: *mut u8, packed: u32, bytes_per_pixel: usize) {
        let bytes = packed.to_le_bytes();
        let count = core::cmp::min(bytes_per_pixel, 4);
        let mut i = 0;
        while i < count {
            unsafe {
                // OPTIMIZATION: Use ptr::write instead of write_volatile
                ptr::write(dst.add(i), bytes[i]);
            }
            i += 1;
        }
    }

    /// Write an R/G/B triple (plus optional zero alpha byte) to `dst` in either
    /// RGB or BGR channel order depending on `bgr`.
    #[inline(always)]
    unsafe fn write_rgb_like(dst: *mut u8, r: u8, g: u8, b: u8, bytes_per_pixel: usize, bgr: bool) {
        if bgr {
            unsafe {
                // OPTIMIZATION: Use ptr::write instead of write_volatile
                ptr::write(dst, b);
                ptr::write(dst.add(1), g);
                ptr::write(dst.add(2), r);
            }
        } else {
            unsafe {
                ptr::write(dst, r);
                ptr::write(dst.add(1), g);
                ptr::write(dst.add(2), b);
            }
        }
        if bytes_per_pixel >= 4 {
            unsafe {
                ptr::write(dst.add(3), 0);
            }
        }
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

    fn bytes_per_pixel(&self) -> usize {
        self.bytes_per_pixel
    }

    fn framebuffer_size(&self) -> usize {
        self.size_bytes
    }

    fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    fn pixel_masks(&self) -> (u32, u32, u32, u32) {
        (
            self.red_mask,
            self.green_mask,
            self.blue_mask,
            self.reserved_mask,
        )
    }

    fn framebuffer(&mut self) -> *mut u8 {
        self.base
    }

    fn flush(&mut self, pixels: &[u32], src_width: usize, src_height: usize) {
        let width = core::cmp::min(self.width, src_width);
        let height = core::cmp::min(self.height, src_height);

        if self.bytes_per_pixel == 4 && matches!(self.pixel_format, PixelFormat::Bgr) {
            for y in 0..height {
                let src_row_start = y * src_width;
                if src_row_start >= pixels.len() {
                    break;
                }

                let src_row_len = core::cmp::min(width, pixels.len() - src_row_start);
                if src_row_len == 0 {
                    continue;
                }

                let dst_offset = (y * self.stride) * 4;
                if dst_offset >= self.size_bytes {
                    continue;
                }

                let max_pixels = (self.size_bytes - dst_offset) / 4;
                let copy_pixels = core::cmp::min(src_row_len, max_pixels);
                if copy_pixels == 0 {
                    continue;
                }

                unsafe {
                    ptr::copy_nonoverlapping(
                        pixels.as_ptr().add(src_row_start).cast::<u8>(),
                        self.base.add(dst_offset),
                        copy_pixels * 4,
                    );
                }
            }
            return;
        }

        if self.bytes_per_pixel == 4 && matches!(self.pixel_format, PixelFormat::Rgb) {
            for y in 0..height {
                let src_row_start = y * src_width;
                if src_row_start >= pixels.len() {
                    break;
                }

                let src_row_len = core::cmp::min(width, pixels.len() - src_row_start);
                if src_row_len == 0 {
                    continue;
                }

                let dst_offset = (y * self.stride) * 4;
                if dst_offset >= self.size_bytes {
                    continue;
                }

                let max_pixels = (self.size_bytes - dst_offset) / 4;
                let copy_pixels = core::cmp::min(src_row_len, max_pixels);
                if copy_pixels == 0 {
                    continue;
                }

                unsafe {
                    self.write_rgb_row(
                        &pixels[src_row_start..src_row_start + copy_pixels],
                        dst_offset,
                    );
                }
            }
            return;
        }

        for y in 0..height {
            for x in 0..width {
                let src = pixels[y * src_width + x];
                let r = ((src >> 16) & 0xFF) as u8;
                let g = ((src >> 8) & 0xFF) as u8;
                let b = (src & 0xFF) as u8;

                let offset = (y * self.stride + x) * self.bytes_per_pixel;
                if offset + self.bytes_per_pixel > self.size_bytes {
                    continue;
                }
                unsafe {
                    let p = self.base.add(offset);
                    match self.pixel_format {
                        PixelFormat::Rgb => {
                            Self::write_rgb_like(p, r, g, b, self.bytes_per_pixel, false);
                        }
                        PixelFormat::Bgr => {
                            Self::write_rgb_like(p, r, g, b, self.bytes_per_pixel, true);
                        }
                        PixelFormat::Bitmask => {
                            Self::write_packed(p, self.pack_bitmask(r, g, b), self.bytes_per_pixel);
                        }
                        // BltOnly framebuffers do not support direct pixel
                        // access; flushing is a no-op for this format.
                        PixelFormat::BltOnly => {}
                    }
                }
            }
        }
    }
}
