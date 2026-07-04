use super::font::{glyph_bitmap, FONT_HEIGHT, FONT_WIDTH};
use super::framebuffer::{Color, Framebuffer};

pub fn draw_glyph(
    fb: &mut Framebuffer,
    x: usize,
    y: usize,
    ch: char,
    fg: Color,
    bg: Color,
) {
    let glyph = glyph_bitmap(ch);
    for row_idx in 0..FONT_HEIGHT {
        let row_bits = glyph[row_idx / 2];
        for bit in 0..FONT_WIDTH {
            // font8x8 stores glyph rows in LSB-first order.
            let mask = 1u8 << bit;
            let color = if (row_bits & mask) != 0 { fg } else { bg };
            fb.put_pixel(x + bit, y + row_idx, color);
        }
    }
}
