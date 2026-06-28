use crate::graphics::image::Image;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum BmpError {
    TooSmall,
    InvalidSignature,
    Unsupported,
    Truncated,
}

pub struct DecodedBmp<'a> {
    pub width: u32,
    pub height: u32,
    pub bpp: u16,
    pub row_stride: usize,
    pixels: &'a [u8],
    bottom_up: bool,
}

impl<'a> DecodedBmp<'a> {
    pub fn as_image<'b>(&self, out: &'b mut [u8]) -> Result<Image<'b>, BmpError> {
        let dst_bpp = 4usize;
        let width = self.width as usize;
        let height = self.height as usize;
        let needed = width * height * dst_bpp;
        if out.len() < needed {
            return Err(BmpError::Truncated);
        }

        let src_bpp = (self.bpp as usize) / 8;
        for y in 0..height {
            let src_y = if self.bottom_up { height - 1 - y } else { y };
            let src_row = src_y * self.row_stride;
            let dst_row = y * width * dst_bpp;

            for x in 0..width {
                let s = src_row + x * src_bpp;
                let d = dst_row + x * dst_bpp;
                if s + src_bpp > self.pixels.len() || d + dst_bpp > out.len() {
                    return Err(BmpError::Truncated);
                }

                let (r, g, b, a) = if src_bpp == 3 {
                    (self.pixels[s + 2], self.pixels[s + 1], self.pixels[s], 255)
                } else {
                    (
                        self.pixels[s + 2],
                        self.pixels[s + 1],
                        self.pixels[s],
                        self.pixels[s + 3],
                    )
                };

                out[d] = r;
                out[d + 1] = g;
                out[d + 2] = b;
                out[d + 3] = a;
            }
        }

        Ok(Image {
            width: self.width,
            height: self.height,
            bpp: 32,
            pixels: &out[..needed],
        })
    }
}

pub fn decode(data: &[u8]) -> Result<DecodedBmp<'_>, BmpError> {
    if data.len() < 54 {
        return Err(BmpError::TooSmall);
    }
    if &data[0..2] != b"BM" {
        return Err(BmpError::InvalidSignature);
    }

    let pixel_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    let dib_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);
    if dib_size < 40 {
        return Err(BmpError::Unsupported);
    }

    let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
    let height_signed = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
    if width <= 0 || height_signed == 0 {
        return Err(BmpError::Unsupported);
    }

    let planes = u16::from_le_bytes([data[26], data[27]]);
    let bpp = u16::from_le_bytes([data[28], data[29]]);
    let compression = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);

    if planes != 1 || compression != 0 || (bpp != 24 && bpp != 32) {
        return Err(BmpError::Unsupported);
    }

    let height = height_signed.unsigned_abs();
    let width_u = width as u32;
    let src_bpp = (bpp as usize) / 8;
    let row_stride = (width_u as usize * src_bpp + 3) & !3;
    let total = row_stride
        .checked_mul(height as usize)
        .ok_or(BmpError::Truncated)?;

    if pixel_offset + total > data.len() {
        return Err(BmpError::Truncated);
    }

    Ok(DecodedBmp {
        width: width_u,
        height,
        bpp,
        row_stride,
        pixels: &data[pixel_offset..pixel_offset + total],
        bottom_up: height_signed > 0,
    })
}
