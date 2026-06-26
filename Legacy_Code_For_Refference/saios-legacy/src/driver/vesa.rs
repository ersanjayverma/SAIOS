//! VESA/VBE linear framebuffer driver.
//!
//! When GRUB loads the kernel with a multiboot2 framebuffer tag, the
//! framebuffer address, dimensions, and bits-per-pixel are passed via
//! the MBI. This driver maps that framebuffer and provides basic 2D
//! drawing primitives.
//!
//! VirtualBox and QEMU both support:
//!   - VBE 3.0 via VESA BIOS Extensions (legacy)
//!   - VirtIO-GPU (preferred — see driver/virtio_gpu.rs planned)
//!
//! To enable in GRUB: add to grub.cfg:
//!   set gfxmode=1024x768x32
//!   set gfxpayload=keep
//!
//! # Phase 7 note
//! This driver is the foundation for the graphics subsystem.
//! The DRM/KMS layer (Phase 7) will sit on top of it.

use spin::Mutex;

/// Framebuffer descriptor filled by `init()` from the Multiboot2 tag.
#[derive(Default, Clone, Copy)]
pub struct Framebuffer {
    /// Physical (and virtual — identity-mapped) address of the framebuffer.
    pub addr: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per scan line (may be > width * bpp/8 due to padding).
    pub pitch: u32,
    /// Bits per pixel (16, 24, or 32).
    pub bpp: u8,
    /// True once initialised.
    pub active: bool,
}

pub static FB: Mutex<Framebuffer> = Mutex::new(Framebuffer {
    addr: 0,
    width: 0,
    height: 0,
    pitch: 0,
    bpp: 0,
    active: false,
});

/// Initialise from a Multiboot2 framebuffer tag (tag type 8).
///
/// # Arguments
/// * `addr`   — physical address of the linear framebuffer
/// * `width`  — horizontal resolution in pixels
/// * `height` — vertical resolution in pixels
/// * `pitch`  — bytes per row
/// * `bpp`    — colour depth (16, 24, or 32)
pub fn init(addr: u64, width: u32, height: u32, pitch: u32, bpp: u8) {
    if addr == 0 || width == 0 || height == 0 {
        return;
    }
    {
        let mut fb = FB.lock();
        *fb = Framebuffer {
            addr,
            width,
            height,
            pitch,
            bpp,
            active: true,
        };
    }
    crate::println!(
        "[vesa] framebuffer {}x{}@{}bpp at {:#x} pitch={}",
        width,
        height,
        bpp,
        addr,
        pitch
    );
}

/// Write one 32-bit ARGB pixel at (x, y).
#[inline]
pub fn put_pixel(x: u32, y: u32, colour: u32) {
    let fb = FB.lock();
    if !fb.active || x >= fb.width || y >= fb.height {
        return;
    }
    let off = (y * fb.pitch + x * (fb.bpp as u32 / 8)) as u64;
    unsafe {
        match fb.bpp {
            32 => core::ptr::write_volatile((fb.addr + off) as *mut u32, colour),
            24 => {
                let p = (fb.addr + off) as *mut u8;
                core::ptr::write_volatile(p, (colour & 0xFF) as u8);
                core::ptr::write_volatile(p.add(1), ((colour >> 8) & 0xFF) as u8);
                core::ptr::write_volatile(p.add(2), ((colour >> 16) & 0xFF) as u8);
            }
            16 => {
                // Pack RGB888 → RGB565
                let r = (colour >> 16) & 0xFF;
                let g = (colour >> 8) & 0xFF;
                let b = colour & 0xFF;
                let px16 = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
                core::ptr::write_volatile((fb.addr + off) as *mut u16, px16 as u16);
            }
            _ => {}
        }
    }
}

/// Read one pixel at (x, y) as 0x00RRGGBB, or 0 if out of range.
#[inline]
pub fn get_pixel(x: u32, y: u32) -> u32 {
    let fb = FB.lock();
    if !fb.active || x >= fb.width || y >= fb.height {
        return 0;
    }
    let off = (y * fb.pitch + x * (fb.bpp as u32 / 8)) as u64;
    unsafe {
        match fb.bpp {
            32 => core::ptr::read_volatile((fb.addr + off) as *const u32) & 0x00FF_FFFF,
            24 => {
                let p = (fb.addr + off) as *const u8;
                let b = core::ptr::read_volatile(p) as u32;
                let g = core::ptr::read_volatile(p.add(1)) as u32;
                let r = core::ptr::read_volatile(p.add(2)) as u32;
                (r << 16) | (g << 8) | b
            }
            16 => {
                let px = core::ptr::read_volatile((fb.addr + off) as *const u16) as u32;
                let r = ((px >> 11) & 0x1F) << 3;
                let g = ((px >> 5) & 0x3F) << 2;
                let b = (px & 0x1F) << 3;
                (r << 16) | (g << 8) | b
            }
            _ => 0,
        }
    }
}

/// Fill a rectangle with a solid colour.
pub fn fill_rect(x: u32, y: u32, w: u32, h: u32, colour: u32) {
    for row in y..y.saturating_add(h) {
        for col in x..x.saturating_add(w) {
            put_pixel(col, row, colour);
        }
    }
}

/// Clear the entire framebuffer to `colour` (fast memset path for 32-bit).
pub fn clear(colour: u32) {
    let fb = FB.lock();
    if !fb.active {
        return;
    }
    let total = (fb.height * fb.pitch) as usize / 4;
    unsafe {
        let p = fb.addr as *mut u32;
        for i in 0..total {
            core::ptr::write_volatile(p.add(i), colour);
        }
    }
}

/// Scroll the framebuffer up by `row_px` pixels using a single memmove, then
/// clear the vacated bottom strip.  Far faster than redrawing every cell.
pub fn scroll_up_px(row_px: usize, clear_colour: u32) {
    let fb = FB.lock();
    if !fb.active {
        return;
    }
    let pitch = fb.pitch as usize;
    let total_h = fb.height as usize;
    if row_px >= total_h {
        return;
    }
    let copy_bytes = pitch * (total_h - row_px);
    let base = fb.addr as usize;
    unsafe {
        core::ptr::copy(
            (base + pitch * row_px) as *const u8,
            base as *mut u8,
            copy_bytes,
        );
        // Clear the last row_px scanlines.
        let clear_base = (base + copy_bytes) as *mut u32;
        let clear_words = pitch * row_px / 4;
        for i in 0..clear_words {
            core::ptr::write_volatile(clear_base.add(i), clear_colour);
        }
    }
}

/// Returns true if the framebuffer is active (GRUB provided one).
pub fn active() -> bool {
    FB.lock().active
}
