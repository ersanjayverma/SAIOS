#![no_std]
#![no_main]

pub mod arch;
pub mod boot;
pub mod drivers;
pub mod fs;
pub mod graphics;
pub mod ipc;
pub mod memory;
pub mod net;
pub mod process;
pub mod scheduler;

use efi_main::SaiosBootInfo;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info:*const SaiosBootInfo) -> ! {
    let fb = unsafe { (*boot_info).framebuffer };
    draw_test_pattern(&fb);

    loop {
        core::hint::spin_loop();
    }
}

fn pack_bgr(color: (u8, u8, u8)) -> u32 {
    ((color.2 as u32) << 16) | ((color.1 as u32) << 8) | color.0 as u32
}

fn draw_test_pattern(fb: &efi_main::graphics::FramebufferInfo) {
    let base = fb.base as *mut u32;
    let width = fb.width;
    let height = fb.height;
    let stride = fb.stride;

    let bg = pack_bgr((18, 24, 48));
    let red = pack_bgr((255, 70, 70));
    let green = pack_bgr((70, 220, 120));
    let yellow = pack_bgr((245, 220, 70));
    let white = pack_bgr((255, 255, 255));

    unsafe {
        for y in 0..height {
            for x in 0..width {
                base.add(y * stride + x).write_volatile(bg);
            }
        }

        base.add(10 * stride + 10).write_volatile(white);

        let mut x0 = 24i32;
        let mut y0 = 32i32;
        let x1 = 240i32;
        let y1 = 132i32;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            if x0 >= 0 && y0 >= 0 && (x0 as usize) < width && (y0 as usize) < height {
                base.add(y0 as usize * stride + x0 as usize).write_volatile(red);
            }
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = err * 2;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }

        let rx = 270usize;
        let ry = 42usize;
        let rw = 200usize;
        let rh = 120usize;
        for x in rx..(rx + rw) {
            base.add(ry * stride + x).write_volatile(green);
            base.add((ry + rh - 1) * stride + x).write_volatile(green);
        }
        for y in ry..(ry + rh) {
            base.add(y * stride + rx).write_volatile(green);
            base.add(y * stride + (rx + rw - 1)).write_volatile(green);
        }

        let cx = 180i32;
        let cy = 260i32;
        let mut x = 72i32;
        let mut y = 0i32;
        let mut err = 1 - x;
        while x >= y {
            let pts = [
                (cx + x, cy + y),
                (cx + y, cy + x),
                (cx - y, cy + x),
                (cx - x, cy + y),
                (cx - x, cy - y),
                (cx - y, cy - x),
                (cx + y, cy - x),
                (cx + x, cy - y),
            ];
            for (px, py) in pts {
                if px >= 0 && py >= 0 && (px as usize) < width && (py as usize) < height {
                    base.add(py as usize * stride + px as usize)
                        .write_volatile(yellow);
                }
            }

            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
    }

}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // if let Some(loc) = location {
    //     let _ = println!("Panic occurred at {}:{}:{}", loc.file(), loc.line(), message);
    // } else {
    //     let _ = println!("Panic occurred: {}", message);
    // }
    loop {
        core::hint::spin_loop();
    }
}
