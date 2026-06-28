use crate::graphics::Size;

pub struct Font {
    pub family: &'static str,
    pub px: u16,
}

pub struct GlyphBitmap<'a> {
    pub size: Size,
    pub alpha: &'a [u8],
}
