use font8x8::UnicodeFonts;

pub const FONT_WIDTH: usize = 8;
pub const FONT_HEIGHT: usize = 16;

fn lookup_font8x8(ch: char) -> Option<[u8; 8]> {
    UnicodeFonts::get(&font8x8::BASIC_FONTS, ch)
        .or_else(|| UnicodeFonts::get(&font8x8::LATIN_FONTS, ch))
        .or_else(|| UnicodeFonts::get(&font8x8::GREEK_FONTS, ch))
        .or_else(|| UnicodeFonts::get(&font8x8::BOX_FONTS, ch))
        .or_else(|| UnicodeFonts::get(&font8x8::BLOCK_FONTS, ch))
        .or_else(|| UnicodeFonts::get(&font8x8::HIRAGANA_FONTS, ch))
        .or_else(|| UnicodeFonts::get(&font8x8::MISC_FONTS, ch))
        .or_else(|| UnicodeFonts::get(&font8x8::SGA_FONTS, ch))
}

fn fallback_row(ch: char, row8: usize) -> u8 {
    let mut state = (ch as u32) ^ ((row8 as u32) * 0x9E37_79B9);
    state ^= state >> 16;
    state = state.wrapping_mul(0x7FEB_352D);
    state ^= state >> 15;

    let mut bits = (state & 0xFF) as u8;
    bits |= 0b1000_0001;
    if row8 == 0 || row8 == 7 {
        bits = 0xFF;
    }
    bits
}

pub fn glyph_row(ch: char, row16: usize) -> u8 {
    let row8 = core::cmp::min(row16 / 2, 7);

    if let Some(rows) = lookup_font8x8(ch) {
        return rows[row8];
    }

    fallback_row(ch, row8)
}
