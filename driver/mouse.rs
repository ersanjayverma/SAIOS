//! PS/2 mouse driver — supports standard 3-byte protocol + IntelliMouse scroll wheel.
//!
//! Initialisation sequence:
//!   1. Enable PS/2 port 2 (send 0xA8 to controller)
//!   2. Enable IRQ12 in controller config
//!   3. Detect IntelliMouse scroll wheel (sample-rate magic sequence)
//!   4. Enable mouse reporting (0xF4)
//!
//! Packet format (IntelliMouse, 4 bytes):
//!   Byte 0: [Y-overflow][X-overflow][Y-sign][X-sign][1][Middle][Right][Left]
//!   Byte 1: X movement
//!   Byte 2: Y movement
//!   Byte 3: scroll wheel delta (signed nibble) + button 4/5

use spin::Mutex;

// PS/2 ports
const PS2_DATA: u16 = 0x60;
const PS2_CMD: u16 = 0x64; // write = command, read = status

// Controller commands
const CMD_READ_CONFIG: u8 = 0x20;
const CMD_WRITE_CONFIG: u8 = 0x60;
const CMD_ENABLE_PORT2: u8 = 0xA8;
const CMD_WRITE_TO_MOUSE: u8 = 0xD4; // next byte goes to mouse

// Mouse commands
const MOUSE_RESET: u8 = 0xFF;
const MOUSE_ENABLE: u8 = 0xF4;
const MOUSE_SET_DEFAULTS: u8 = 0xF6;
const MOUSE_SET_SAMPLE: u8 = 0xF3;
const MOUSE_GET_ID: u8 = 0xF2;
const MOUSE_ACK: u8 = 0xFA;

// -- Global mouse state -----------------------------------------------------

#[derive(Default, Clone, Copy)]
pub struct MouseState {
    pub x: i32,      // absolute column (0 = left, text-grid resolution)
    pub y: i32,      // absolute row    (0 = top,  text-grid resolution)
    pub gx: i32,     // pixel-resolution X (for the GUI toolkit, Phase 9)
    pub gy: i32,     // pixel-resolution Y
    pub scroll: i32, // cumulative scroll delta (positive = down)
    pub left: bool,
    pub right: bool,
    pub middle: bool,
    pub scroll_delta: i8, // last scroll tick (+1 down, -1 up)
}

pub static STATE: Mutex<MouseState> = Mutex::new(MouseState {
    x: 40,
    y: 12, // start at screen centre
    gx: 512,
    gy: 384,
    scroll: 0,
    left: false,
    right: false,
    middle: false,
    scroll_delta: 0,
});

// Pixel bounds for the GUI cursor (framebuffer size), set by the UI toolkit.
use core::sync::atomic::{AtomicI32, Ordering};
static GFX_W: AtomicI32 = AtomicI32::new(1024);
static GFX_H: AtomicI32 = AtomicI32::new(768);

/// Set the pixel bounds the GUI mouse cursor is clamped to (framebuffer size).
pub fn set_gfx_bounds(w: i32, h: i32) {
    GFX_W.store(w.max(1), Ordering::Relaxed);
    GFX_H.store(h.max(1), Ordering::Relaxed);
    crate::arch::without_interrupts(|| {
        let mut s = STATE.lock();
        s.gx = s.gx.clamp(0, w - 1);
        s.gy = s.gy.clamp(0, h - 1);
    });
}

/// Pixel-resolution cursor position and whether the left button is down.
pub fn gfx_state() -> (i32, i32, bool) {
    crate::arch::without_interrupts(|| {
        let s = STATE.lock();
        (s.gx, s.gy, s.left)
    })
}

// Packet assembly state
static PACKET: Mutex<PacketState> = Mutex::new(PacketState::new());
static HAS_SCROLL: Mutex<bool> = Mutex::new(false);

struct PacketState {
    buf: [u8; 4],
    idx: usize,
    bytes: usize, // 3 or 4
}

impl PacketState {
    const fn new() -> Self {
        Self {
            buf: [0u8; 4],
            idx: 0,
            bytes: 3,
        }
    }
}

// Screen bounds (VGA text mode)
const COLS: i32 = 80;
const ROWS: i32 = 25;

// -- Public API -------------------------------------------------------------

/// Initialise the PS/2 mouse. Call after keyboard init.
pub fn init() {
    unsafe {
        init_inner();
    }
    let has_scroll = crate::arch::without_interrupts(|| *HAS_SCROLL.lock());
    crate::println!(
        "[mouse] PS/2 mouse ready ({})",
        if has_scroll {
            "IntelliMouse scroll wheel"
        } else {
            "3-button"
        }
    );
}

/// Whether the cursor display needs refreshing (set by IRQ, cleared by poll).
/// Using an atomic avoids taking any lock in interrupt context.
static CURSOR_DIRTY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Called from IRQ12 handler — feeds one byte into the packet assembler.
/// Never touches VGA memory directly — that happens in `apply_cursor_update()`
/// called from the main loop where the WRITER lock can be safely taken.
pub fn handle_byte(byte: u8) {
    let mut pkt = PACKET.lock();

    // First byte sanity check: bit 3 must be set, overflow bits clear
    if pkt.idx == 0 && (byte & 0x08 == 0 || byte & 0xC0 != 0) {
        return; // desync — drop
    }

    let idx = pkt.idx;
    pkt.buf[idx] = byte;
    pkt.idx += 1;

    if pkt.idx >= pkt.bytes {
        let buf = pkt.buf;
        let bytes = pkt.bytes;
        pkt.idx = 0;
        drop(pkt);
        process_packet(&buf, bytes);
        // Signal the main loop that the cursor position changed
        CURSOR_DIRTY.store(true, core::sync::atomic::Ordering::Relaxed);
    }
}

/// Apply any pending cursor position update to the VGA display.
/// Must be called from the main loop (NOT interrupt context) so the
/// WRITER lock can be safely acquired.
pub fn apply_cursor_update() {
    // IRQ12 locks STATE/PREV_CURSOR via process_packet; disable interrupts while
    // this mainline code holds them so the handler can never spin on a lock we
    // own (which would freeze keyboard + mouse + timer permanently).
    crate::arch::without_interrupts(|| {
        if CURSOR_DIRTY.swap(false, core::sync::atomic::Ordering::Relaxed) {
            update_cursor_display();
        }
    });
}

/// Returns the current scroll delta since last call, then resets it.
pub fn take_scroll_delta() -> i8 {
    crate::arch::without_interrupts(|| {
        let mut s = STATE.lock();
        let d = s.scroll_delta;
        s.scroll_delta = 0;
        d
    })
}

// -- Packet processing ------------------------------------------------------

fn process_packet(buf: &[u8; 4], bytes: usize) {
    let flags = buf[0];

    // Decode X / Y deltas (9-bit signed: sign bit in flags byte)
    let dx = {
        let raw = buf[1] as i16;
        if flags & 0x10 != 0 { raw - 256 } else { raw }
    };
    let dy = {
        let raw = buf[2] as i16;
        if flags & 0x20 != 0 { raw - 256 } else { raw }
    };

    // Scroll wheel (byte 3, lower nibble, signed)
    let scroll: i8 = if bytes == 4 {
        let raw = (buf[3] & 0x0F) as i8;
        // Sign-extend from 4 bits
        if raw & 0x08 != 0 { raw | -16i8 } else { raw }
    } else {
        0
    };

    let mut s = STATE.lock();

    // Update position (Y is inverted: PS/2 reports up as positive)
    s.x = (s.x + dx as i32).clamp(0, COLS - 1);
    s.y = (s.y - dy as i32).clamp(0, ROWS - 1);
    // Pixel-resolution position for the GUI toolkit (3x accel for usable speed).
    let gw = GFX_W.load(Ordering::Relaxed);
    let gh = GFX_H.load(Ordering::Relaxed);
    s.gx = (s.gx + dx as i32 * 3).clamp(0, gw - 1);
    s.gy = (s.gy - dy as i32 * 3).clamp(0, gh - 1);

    // Buttons
    s.left = flags & 0x01 != 0;
    s.right = flags & 0x02 != 0;
    s.middle = flags & 0x04 != 0;

    // Scroll
    if scroll != 0 {
        s.scroll_delta = scroll;
        s.scroll = s.scroll.wrapping_add(scroll as i32);
    }

    drop(s);
    update_cursor_display();
}

// -- VGA text-mode cursor ---------------------------------------------------

// u16::MAX = sentinel meaning "cursor not yet drawn, nothing to restore"
static PREV_CURSOR: Mutex<(i32, i32, u16)> = Mutex::new((-1, -1, u16::MAX));

fn update_cursor_display() {
    // In graphics mode the 0xB8000 text buffer is not displayed, so drawing the
    // text-mode cursor there is invisible (and pointless work in the IRQ path).
    // The framebuffer desktop draws its own cursor via graphics::draw_cursor.
    if crate::vga_buffer::GFX_CONSOLE.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let s = STATE.lock();
    let (cx, cy) = (s.x, s.y);
    drop(s);

    let mut prev = PREV_CURSOR.lock();
    let (px, py, saved_char) = *prev;

    const VGA: u64 = 0xB8000;

    unsafe {
        // Restore the cell we last drew the cursor on
        if px >= 0 && py >= 0 && saved_char != u16::MAX && (px != cx || py != cy) {
            let old_addr = (VGA + (py as u64 * 80 + px as u64) * 2) as *mut u16;
            core::ptr::write_volatile(old_addr, saved_char);
        }

        // Read the NEW cell's original contents, then draw an inverted cursor
        let new_addr = (VGA + (cy as u64 * 80 + cx as u64) * 2) as *mut u16;
        let orig = core::ptr::read_volatile(new_addr);

        let ch = orig & 0x00FF;
        let attr = (orig >> 8) as u8;
        // Swap fg ↔ bg nibbles for a solid block cursor
        let inv = ((attr & 0x0F) << 4) | ((attr >> 4) & 0x0F);
        // If the cell is blank, show a full-block cursor (█ = 0xDB)
        let cursor_ch = if ch == b' ' as u16 { 0xDB } else { ch };
        let cursor_cell = cursor_ch | ((inv as u16) << 8);

        *prev = (cx, cy, orig); // remember original to restore later
        core::ptr::write_volatile(new_addr, cursor_cell);
    }
}

// -- Initialisation ---------------------------------------------------------

unsafe fn init_inner() {
    unsafe {
        // Enable PS/2 port 2 (mouse)
        crate::arch::port_write_u8(PS2_CMD, CMD_ENABLE_PORT2);
        wait_write();

        // Read controller config, enable IRQ12 and mouse clock
        crate::arch::port_write_u8(PS2_CMD, CMD_READ_CONFIG);
        wait_read();
        let mut config = crate::arch::port_read_u8(PS2_DATA);
        config |= 0x02; // enable IRQ12
        config &= !0x20; // enable mouse clock (clear bit 5)
        crate::arch::port_write_u8(PS2_CMD, CMD_WRITE_CONFIG);
        wait_write();
        crate::arch::port_write_u8(PS2_DATA, config);
        wait_write();

        // Reset mouse
        mouse_cmd(MOUSE_RESET);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA); // ACK
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA); // 0xAA (self-test passed)
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA); // 0x00 (device ID)

        // Try to enable IntelliMouse scroll wheel
        // Magic sequence: sample 200, 100, 80 → device ID 0x03 = scroll wheel
        mouse_cmd(MOUSE_SET_SAMPLE);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA);
        mouse_write(200);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA);
        mouse_cmd(MOUSE_SET_SAMPLE);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA);
        mouse_write(100);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA);
        mouse_cmd(MOUSE_SET_SAMPLE);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA);
        mouse_write(80);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA);

        mouse_cmd(MOUSE_GET_ID);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA); // ACK
        wait_read();
        let id = crate::arch::port_read_u8(PS2_DATA);
        if id == 0x03 {
            crate::arch::without_interrupts(|| {
                *HAS_SCROLL.lock() = true;
                PACKET.lock().bytes = 4;
            });
        }

        // Set default sample rate (100 reports/sec)
        mouse_cmd(MOUSE_SET_DEFAULTS);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA);

        // Enable reporting
        mouse_cmd(MOUSE_ENABLE);
        wait_read();
        let _ = crate::arch::port_read_u8(PS2_DATA);
    }
}

unsafe fn mouse_cmd(cmd: u8) {
    unsafe {
        wait_write();
        crate::arch::port_write_u8(PS2_CMD, CMD_WRITE_TO_MOUSE);
        wait_write();
        crate::arch::port_write_u8(PS2_DATA, cmd);
    }
}

unsafe fn mouse_write(byte: u8) {
    unsafe {
        wait_write();
        crate::arch::port_write_u8(PS2_CMD, CMD_WRITE_TO_MOUSE);
        wait_write();
        crate::arch::port_write_u8(PS2_DATA, byte);
    }
}

unsafe fn wait_write() {
    unsafe {
        for _ in 0..100_000u32 {
            if crate::arch::port_read_u8(PS2_CMD) & 0x02 == 0 {
                return;
            }
            crate::arch::nop();
        }
    }
}

unsafe fn wait_read() {
    unsafe {
        for _ in 0..100_000u32 {
            if crate::arch::port_read_u8(PS2_CMD) & 0x01 != 0 {
                return;
            }
            crate::arch::nop();
        }
    }
}
