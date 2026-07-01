use super::font::{glyph_row, FONT_HEIGHT, FONT_WIDTH};
use super::framebuffer::{Color, Framebuffer};

pub fn draw_glyph(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    ch: char,
    fg: Color,
    bg: Color,
) {
    for row_idx in 0..FONT_HEIGHT {
        let row_bits = glyph_row(ch, row_idx);
        for bit in 0..FONT_WIDTH {
            let mask = 1u8 << (7 - bit);
            let color = if (row_bits & mask) != 0 { fg } else { bg };
            fb.put_pixel(x + bit, y + row_idx, color);
        }
    }
}
