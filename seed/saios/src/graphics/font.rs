use font8x8::UnicodeFonts;

/// Width of a rendered glyph cell, in pixels.
pub const FONT_WIDTH: usize = 8;
/// Height of a rendered glyph cell, in pixels (the 8-row source font is scaled
/// to 16 scanlines — see [`glyph_row`]).
pub const FONT_HEIGHT: usize = 16;

/// Look up an 8x8 bitmap for `ch`, trying each font8x8 code-block table in turn.
/// Returns `None` if the character is not present in any table.
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

/// Generate a deterministic pseudo-random bit pattern for characters that have
/// no bitmap in any font table, so unknown glyphs render as a distinct,
/// bordered placeholder box instead of blank space.
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

fn glyph_rows(ch: char) -> [u8; 8] {
    // Resolve the glyph once so row readers can reuse the bitmap without
    // repeating font-table lookups or fallback generation.
    if let Some(rows) = lookup_font8x8(ch) {
        return rows;
    }

    let mut rows = [0u8; 8];
    let mut row8 = 0;
    while row8 < 8 {
        rows[row8] = fallback_row(ch, row8);
        row8 += 1;
    }
    rows
}

/// Return the 8 source rows for `ch` as a compact bitmap.
pub fn glyph_bitmap(ch: char) -> [u8; 8] {
    glyph_rows(ch)
}

/// Return one doubled scanline for the glyph for `ch`.
pub fn glyph_row(ch: char, row16: usize) -> u8 {
    if row16 >= FONT_HEIGHT {
        return 0;
    }

    let rows = glyph_rows(ch);
    rows[row16 / 2]
}
