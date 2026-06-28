use crate::graphics::fonts::bitmap::BitmapFont;

use super::context::RRodContext;

const BLACK: (u8, u8, u8) = (0, 0, 0);
const DARK_RED: (u8, u8, u8) = (36, 0, 0);
const RED: (u8, u8, u8) = (255, 32, 32);
const RED_HI: (u8, u8, u8) = (255, 85, 85);
const WHITE: (u8, u8, u8) = (255, 255, 255);
const GRAY: (u8, u8, u8) = (170, 170, 170);

#[inline]
fn pack(color: (u8, u8, u8), bgr: bool) -> u32 {
    if bgr {
        ((color.2 as u32) << 16) | ((color.1 as u32) << 8) | color.0 as u32
    } else {
        ((color.0 as u32) << 16) | ((color.1 as u32) << 8) | color.2 as u32
    }
}

#[inline]
fn put_pixel(
    base: *mut u32,
    stride: usize,
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    color: u32,
) {
    if x < 0 || y < 0 || x as usize >= width || y as usize >= height {
        return;
    }
    unsafe {
        base.add(y as usize * stride + x as usize)
            .write_volatile(color);
    }
}

fn draw_char(
    base: *mut u32,
    stride: usize,
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    ch: char,
    color: u32,
) {
    let font = BitmapFont::new_5x7();
    let rows = font.glyph_rows(ch);
    for (row, bits) in rows.iter().enumerate() {
        for col in 0..font.width as usize {
            let bit = 4usize.saturating_sub(col);
            if ((bits >> bit) & 1) != 0 {
                put_pixel(
                    base,
                    stride,
                    width,
                    height,
                    x + col as i32,
                    y + row as i32,
                    color,
                );
            }
        }
    }
}

fn draw_text(
    base: *mut u32,
    stride: usize,
    width: usize,
    height: usize,
    mut x: i32,
    mut y: i32,
    text: &str,
    color: u32,
) {
    for ch in text.chars() {
        if ch == '\n' {
            y += 8;
            x = 0;
            continue;
        }
        draw_char(base, stride, width, height, x, y, ch, color);
        x += 6;
    }
}

fn draw_centered(
    base: *mut u32,
    stride: usize,
    width: usize,
    height: usize,
    y: i32,
    text: &str,
    color: u32,
) {
    let text_w = (text.len() as i32) * 6;
    let x = (width as i32 - text_w) / 2;
    draw_text(base, stride, width, height, x, y, text, color);
}

fn to_hex(value: u64, out: &mut [u8; 18]) -> &str {
    out[0] = b'0';
    out[1] = b'x';
    let mut i = 0;
    while i < 16 {
        let shift = (15 - i) * 4;
        let nib = ((value >> shift) & 0xF) as u8;
        out[2 + i] = if nib < 10 {
            b'0' + nib
        } else {
            b'A' + (nib - 10)
        };
        i += 1;
    }
    core::str::from_utf8(out).unwrap_or("0x0000000000000000")
}

pub fn render(boot_info: &efi_main::SaiosBootInfo, info: &RRodContext) {
    let fb = &boot_info.framebuffer;
    let base = fb.base as *mut u32;
    let width = fb.width;
    let height = fb.height;
    let stride = fb.stride;
    let bgr = matches!(fb.pixel_format, efi_main::graphics::PixelFormat::Bgr);

    let c_black = pack(BLACK, bgr);
    let c_dark_red = pack(DARK_RED, bgr);
    let c_red = pack(RED, bgr);
    let c_red_hi = pack(RED_HI, bgr);
    let c_white = pack(WHITE, bgr);
    let c_gray = pack(GRAY, bgr);

    unsafe {
        for y in 0..height {
            for x in 0..width {
                base.add(y * stride + x).write_volatile(c_black);
            }
        }
    }

    let edge_band = (height / 6).max(1);
    for y in 0..height as i32 {
        let from_edge = if (y as usize) < height / 2 {
            y as usize
        } else {
            (height - 1).saturating_sub(y as usize)
        };
        if from_edge < edge_band {
            for x in 0..width as i32 {
                put_pixel(base, stride, width, height, x, y, c_dark_red);
            }
        }
    }

    let cx = (width / 2) as i32;
    let cy = (height / 2) as i32;
    let outer = (height as i32 * 33) / 100;
    let inner = outer - 26;
    let glow = outer + 10;
    let inner_hi_outer = inner + 6;
    let inner_hi_inner = inner - 2;
    let outer2 = outer * outer;
    let inner2 = inner * inner;
    let glow2 = glow * glow;
    let inner_hi_outer2 = inner_hi_outer * inner_hi_outer;
    let inner_hi_inner2 = inner_hi_inner * inner_hi_inner;

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 <= glow2 && d2 > outer2 {
                put_pixel(base, stride, width, height, x, y, c_red_hi);
            } else if d2 <= outer2 && d2 >= inner2 {
                put_pixel(base, stride, width, height, x, y, c_red);
            } else if d2 < inner_hi_outer2 && d2 >= inner_hi_inner2 {
                put_pixel(base, stride, width, height, x, y, c_red_hi);
            }
        }
    }

    draw_centered(
        base,
        stride,
        width,
        height,
        cy - outer + 24,
        "SEED",
        c_white,
    );
    draw_centered(
        base,
        stride,
        width,
        height,
        cy - outer + 44,
        "SEED FATAL EXCEPTION",
        c_red_hi,
    );
    draw_centered(base, stride, width, height, cy - outer + 64, "/!\\", c_red);
    draw_centered(
        base,
        stride,
        width,
        height,
        cy - outer + 84,
        "RED RING OF DEATH",
        c_red,
    );

    draw_centered(
        base,
        stride,
        width,
        height,
        cy - 36,
        "The operating system has encountered",
        c_gray,
    );
    draw_centered(
        base,
        stride,
        width,
        height,
        cy - 26,
        "a fatal SEED error and cannot",
        c_gray,
    );
    draw_centered(
        base,
        stride,
        width,
        height,
        cy - 16,
        "continue execution safely.",
        c_gray,
    );

    let left = cx - 170;
    draw_text(
        base,
        stride,
        width,
        height,
        left,
        cy + 4,
        "ERROR SUMMARY",
        c_white,
    );
    draw_text(
        base,
        stride,
        width,
        height,
        left,
        cy + 18,
        "Reason:",
        c_gray,
    );
    draw_text(
        base,
        stride,
        width,
        height,
        left + 80,
        cy + 18,
        info.reason,
        c_white,
    );

    draw_text(
        base,
        stride,
        width,
        height,
        left,
        cy + 28,
        "Exception:",
        c_gray,
    );
    draw_text(
        base,
        stride,
        width,
        height,
        left + 80,
        cy + 28,
        info.exception.as_str(),
        c_white,
    );

    let mut hex = [0u8; 18];
    draw_text(base, stride, width, height, left, cy + 38, "RIP:", c_gray);
    draw_text(
        base,
        stride,
        width,
        height,
        left + 80,
        cy + 38,
        to_hex(info.rip, &mut hex),
        c_white,
    );

    draw_text(base, stride, width, height, left, cy + 48, "RSP:", c_gray);
    draw_text(
        base,
        stride,
        width,
        height,
        left + 80,
        cy + 48,
        to_hex(info.rsp, &mut hex),
        c_white,
    );

    draw_text(base, stride, width, height, left, cy + 58, "RBP:", c_gray);
    draw_text(
        base,
        stride,
        width,
        height,
        left + 80,
        cy + 58,
        to_hex(info.rbp, &mut hex),
        c_white,
    );

    draw_text(base, stride, width, height, left, cy + 68, "CR2:", c_gray);
    draw_text(
        base,
        stride,
        width,
        height,
        left + 80,
        cy + 68,
        to_hex(info.cr2, &mut hex),
        c_white,
    );

    draw_text(
        base,
        stride,
        width,
        height,
        left,
        cy + 78,
        "Error Code:",
        c_gray,
    );
    draw_text(
        base,
        stride,
        width,
        height,
        left + 80,
        cy + 78,
        to_hex(info.error_code, &mut hex),
        c_white,
    );

    draw_text(
        base,
        stride,
        width,
        height,
        left,
        cy + 88,
        "Location:",
        c_gray,
    );
    draw_text(
        base,
        stride,
        width,
        height,
        left + 80,
        cy + 88,
        info.file,
        c_white,
    );

    draw_centered(
        base,
        stride,
        width,
        height,
        height as i32 - 34,
        "SYSTEM HALTED",
        c_white,
    );
    draw_centered(
        base,
        stride,
        width,
        height,
        height as i32 - 22,
        "REBOOT REQUIRED",
        c_gray,
    );
}
