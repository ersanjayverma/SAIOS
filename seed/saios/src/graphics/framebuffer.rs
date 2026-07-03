/// A 24-bit RGB color with 8 bits per channel.
#[derive(Copy, Clone)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// Solid black.
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
    /// Solid white.
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
    };

    /// Pack the color into a 0x00RRGGBB word (the format used everywhere in the
    /// graphics stack for logical pixels).
    #[inline]
    pub const fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }
}
