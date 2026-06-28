use crate::graphics::Size;

pub struct Font {
    pub family: &'static str,
    pub px: u16,
}

pub struct GlyphBitmap<'a> {
    pub size: Size,
    pub alpha: &'a [u8],
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FontError {
    InvalidHeader,
    Unsupported,
    Truncated,
}

#[derive(Debug, Copy, Clone)]
pub struct PsfFont<'a> {
    glyph_count: u32,
    glyph_width: u32,
    glyph_height: u32,
    bytes_per_glyph: usize,
    glyph_data: &'a [u8],
}

impl<'a> PsfFont<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FontError> {
        if bytes.len() < 4 {
            return Err(FontError::InvalidHeader);
        }

        let psf1_magic = bytes[0] == 0x36 && bytes[1] == 0x04;
        let psf2_magic = bytes.len() >= 4
            && bytes[0] == 0x72
            && bytes[1] == 0xB5
            && bytes[2] == 0x4A
            && bytes[3] == 0x86;

        if psf1_magic {
            Self::parse_psf1(bytes)
        } else if psf2_magic {
            Self::parse_psf2(bytes)
        } else {
            Err(FontError::Unsupported)
        }
    }

    fn parse_psf1(bytes: &'a [u8]) -> Result<Self, FontError> {
        if bytes.len() < 4 {
            return Err(FontError::Truncated);
        }
        let mode = bytes[2];
        let charsize = bytes[3] as usize;
        if charsize == 0 {
            return Err(FontError::InvalidHeader);
        }

        let glyph_count = if (mode & 0x01) != 0 { 512 } else { 256 };
        let needed = 4 + glyph_count * charsize;
        if bytes.len() < needed {
            return Err(FontError::Truncated);
        }

        Ok(Self {
            glyph_count: glyph_count as u32,
            glyph_width: 8,
            glyph_height: charsize as u32,
            bytes_per_glyph: charsize,
            glyph_data: &bytes[4..4 + glyph_count * charsize],
        })
    }

    fn parse_psf2(bytes: &'a [u8]) -> Result<Self, FontError> {
        if bytes.len() < 32 {
            return Err(FontError::Truncated);
        }

        let header_size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let glyph_count = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let bytes_per_glyph =
            u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as usize;
        let glyph_height = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let glyph_width = u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);

        if header_size < 32 || bytes_per_glyph == 0 || glyph_width == 0 || glyph_height == 0 {
            return Err(FontError::InvalidHeader);
        }

        let glyph_bytes = glyph_count as usize * bytes_per_glyph;
        if bytes.len() < header_size + glyph_bytes {
            return Err(FontError::Truncated);
        }

        Ok(Self {
            glyph_count,
            glyph_width,
            glyph_height,
            bytes_per_glyph,
            glyph_data: &bytes[header_size..header_size + glyph_bytes],
        })
    }

    pub const fn glyph_width(&self) -> u32 {
        self.glyph_width
    }

    pub const fn glyph_height(&self) -> u32 {
        self.glyph_height
    }

    pub const fn glyph_count(&self) -> u32 {
        self.glyph_count
    }

    pub fn glyph(&self, index: u32) -> Option<&'a [u8]> {
        if index >= self.glyph_count {
            return None;
        }
        let start = index as usize * self.bytes_per_glyph;
        Some(&self.glyph_data[start..start + self.bytes_per_glyph])
    }

    pub fn glyph_for_char(&self, ch: char) -> Option<&'a [u8]> {
        let code = ch as u32;
        if code < self.glyph_count {
            self.glyph(code)
        } else {
            self.glyph('?' as u32)
        }
    }
}
