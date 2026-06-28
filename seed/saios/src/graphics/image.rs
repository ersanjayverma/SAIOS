pub struct Image<'a> {
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
    pub pixels: &'a [u8],
}
