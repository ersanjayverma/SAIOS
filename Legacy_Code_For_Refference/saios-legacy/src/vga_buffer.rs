use core::fmt;
use spin::Mutex;
use volatile::Volatile;
use x86_64::instructions::port::Port;

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;
const VGA_BUFFER: usize = 0xb8000;
const SCROLLBACK: usize = 500;

// VGA CRTC (cathode-ray tube controller) port pair.
// We write the cursor position here so the hardware blinking cursor
// appears in the right place (after the last printed character).
const CRTC_INDEX: u16 = 0x3D4;
const CRTC_DATA: u16 = 0x3D5;
const CURSOR_HIGH: u8 = 0x0E;
const CURSOR_LOW: u8 = 0x0F;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    pub fn new(fg: Color, bg: Color) -> Self {
        ColorCode((bg as u8) << 4 | (fg as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii: u8,
    color: ColorCode,
}

#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

pub struct Writer {
    col: usize,
    pub color: ColorCode,
    buffer: *mut Buffer,
    // Scrollback ring buffer
    history: [[ScreenChar; BUFFER_WIDTH]; SCROLLBACK],
    hist_head: usize,  // index of oldest line
    hist_count: usize, // lines stored
    scroll_off: usize, // 0 = live view; N = scrolled back N lines
}

impl Writer {
    /// Write one byte to the VGA buffer.
    ///
    /// Control characters handled:
    ///   `\n` (0x0A)  — newline: scroll up, reset column to 0
    ///   `\r` (0x0D)  — carriage return: reset column to 0 without scrolling
    ///   `\x08` (0x08) — backspace: move column left by 1, erase the character
    ///   All other bytes are printed as glyphs from the CP437 font.
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            // Newline — scroll the screen up one line
            b'\n' => self.new_line(),

            // Carriage return — move to column 0 on the same line
            b'\r' => {
                self.col = 0;
            }

            // Backspace — erase the previous character and move left
            0x08 => {
                if self.col > 0 {
                    self.col -= 1;
                    let row = BUFFER_HEIGHT - 1;
                    let col = self.col;
                    unsafe {
                        (*self.buffer).chars[row][col].write(ScreenChar {
                            ascii: b' ',
                            color: self.color,
                        });
                    }
                }
            }

            // Printable ASCII + extended CP437 characters
            byte => {
                if self.col >= BUFFER_WIDTH {
                    self.new_line();
                }
                let row = BUFFER_HEIGHT - 1;
                let col = self.col;
                unsafe {
                    (*self.buffer).chars[row][col].write(ScreenChar {
                        ascii: byte,
                        color: self.color,
                    });
                }
                self.col += 1;
            }
        }
    }

    /// Write a string to the VGA buffer.
    ///
    /// Non-printable bytes other than `\n`, `\r`, and `\x08` are replaced
    /// with `░` (CP437 0xB0) so they are visible rather than invisible.
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // Pass through printable ASCII, newline, CR, and backspace
                0x20..=0x7E | b'\n' | b'\r' | 0x08 => self.write_byte(byte),
                // Replace other control chars with a visible placeholder
                _ => self.write_byte(0xB0),
            }
        }
    }

    /// Move the VGA hardware cursor (the blinking underscore/block) to the
    /// current text position.  Called after every `_print()` so the cursor
    /// always appears immediately after the last character written.
    pub fn update_hw_cursor(&self) {
        // The hardware cursor position is a flat index: row * 80 + col.
        // Row is always BUFFER_HEIGHT-1 (the bottom line) because we scroll.
        let pos = (BUFFER_HEIGHT - 1) * BUFFER_WIDTH + self.col;
        unsafe {
            Port::<u8>::new(CRTC_INDEX).write(CURSOR_HIGH);
            Port::<u8>::new(CRTC_DATA).write((pos >> 8) as u8);
            Port::<u8>::new(CRTC_INDEX).write(CURSOR_LOW);
            Port::<u8>::new(CRTC_DATA).write(pos as u8);
        }
    }

    pub fn new_line(&mut self) {
        // Save the top visible row into history before scrolling it away
        let mut saved = [BLANK_CHAR; BUFFER_WIDTH];
        for (col, slot) in saved.iter_mut().enumerate().take(BUFFER_WIDTH) {
            *slot = unsafe { (*self.buffer).chars[0][col].read() };
        }
        let idx = (self.hist_head + self.hist_count) % SCROLLBACK;
        self.history[idx] = saved;
        if self.hist_count < SCROLLBACK {
            self.hist_count += 1;
        } else {
            self.hist_head = (self.hist_head + 1) % SCROLLBACK;
        }

        // Scroll VGA buffer up one row
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                unsafe {
                    let ch = (*self.buffer).chars[row][col].read();
                    (*self.buffer).chars[row - 1][col].write(ch);
                }
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.col = 0;
    }

    /// Scroll display up by `lines` (shows older output).
    pub fn scroll_up(&mut self, lines: usize) {
        let max = self.hist_count;
        self.scroll_off = (self.scroll_off + lines).min(max);
        self.redraw_from_history();
    }

    /// Scroll display down by `lines` (towards live output).
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_off = self.scroll_off.saturating_sub(lines);
        self.redraw_from_history();
    }

    /// Jump back to live (bottom of scrollback).
    pub fn scroll_to_bottom(&mut self) {
        if self.scroll_off != 0 {
            self.scroll_off = 0;
            self.redraw_from_history();
        }
    }

    fn redraw_from_history(&mut self) {
        // Build a view: scroll_off=0 → current VGA (no change needed for live),
        // scroll_off=N → show N lines before the current screen.
        if self.scroll_off == 0 {
            return;
        } // live view unchanged

        let off = self.scroll_off;
        let count = self.hist_count;

        for screen_row in 0..BUFFER_HEIGHT {
            // Which history line maps to this screen row?
            // screen_row 0 = oldest visible line = history[count - off - (BUFFER_HEIGHT-1) + screen_row]
            let hist_idx_from_end = off + (BUFFER_HEIGHT - 1 - screen_row);
            let row_data = if hist_idx_from_end <= count {
                let abs = (self.hist_head + count - hist_idx_from_end) % SCROLLBACK;
                &self.history[abs]
            } else {
                &BLANK_ROW
            };
            for (col, ch) in row_data.iter().enumerate().take(BUFFER_WIDTH) {
                unsafe {
                    (*self.buffer).chars[screen_row][col].write(*ch);
                }
            }
        }
    }

    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii: b' ',
            color: self.color,
        };
        for col in 0..BUFFER_WIDTH {
            unsafe {
                (*self.buffer).chars[row][col].write(blank);
            }
        }
    }

    pub fn clear_screen(&mut self) {
        for row in 0..BUFFER_HEIGHT {
            self.clear_row(row);
        }
        self.col = 0;
        // After clearing, move the hardware cursor to the top-left
        self.update_hw_cursor();
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

// SAFETY: raw pointer to VGA MMIO — single-core kernel, access serialised by the Mutex.
unsafe impl Send for Writer {}

const BLANK_CHAR: ScreenChar = ScreenChar {
    ascii: b' ',
    color: ColorCode(0x07),
};
const BLANK_ROW: [ScreenChar; BUFFER_WIDTH] = [BLANK_CHAR; BUFFER_WIDTH];

pub static WRITER: Mutex<Writer> = Mutex::new(Writer {
    col: 0,
    color: ColorCode(0),
    buffer: VGA_BUFFER as *mut Buffer,
    history: [[BLANK_CHAR; BUFFER_WIDTH]; SCROLLBACK],
    hist_head: 0,
    hist_count: 0,
    scroll_off: 0,
});

/// Initialise the VGA writer: set default color and enable the hardware cursor.
///
/// The hardware cursor is configured to show as a full block (scan lines 0–15).
/// Without this call the cursor may be hidden or show at row 0 col 0.
pub fn init() {
    WRITER.lock().color = ColorCode::new(Color::LightGreen, Color::Black);
    enable_cursor();
}

/// Enable the VGA blinking text cursor and set it to a full-block shape.
fn enable_cursor() {
    unsafe {
        // Set cursor start scan line = 0 (top of character cell), enable bit = 0
        Port::<u8>::new(CRTC_INDEX).write(0x0A);
        Port::<u8>::new(CRTC_DATA).write(0x00);
        // Set cursor end scan line = 15 (bottom of cell) for a full-block cursor
        Port::<u8>::new(CRTC_INDEX).write(0x0B);
        Port::<u8>::new(CRTC_DATA).write(0x0F);
    }
}

pub fn clear() {
    // In graphics mode the visible surface is the framebuffer console, not the
    // 0xB8000 text grid — clear that instead (and keep the VGA grid reset too,
    // so a later mode switch starts clean).
    if GFX_CONSOLE.load(core::sync::atomic::Ordering::Relaxed) {
        crate::graphics::console::clear();
    }
    init();
    WRITER.lock().clear_screen();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Erase the previous character on the current line.
/// Atomically: move col back, write space, move col back again, update cursor.
/// Called by the shell on Backspace key — avoids three separate lock acquisitions.
pub fn backspace() {
    // In graphics mode the visible surface is the framebuffer console — erase
    // there (the 0xB8000 path below is invisible under a GRUB framebuffer).
    if GFX_CONSOLE.load(core::sync::atomic::Ordering::Relaxed) {
        crate::graphics::console::backspace();
        return;
    }
    let mut w = WRITER.lock();
    if w.col > 0 {
        w.col -= 1;
        let row = BUFFER_HEIGHT - 1;
        let col = w.col;
        let blank = ScreenChar {
            ascii: b' ',
            color: w.color,
        };
        unsafe {
            (*w.buffer).chars[row][col].write(blank);
        }
        w.update_hw_cursor();
    }
}

/// Position (and show) the text cursor at absolute (col, row) — used by full-
/// screen apps like nano to mark where editing happens.  Routes to the
/// framebuffer console in graphics mode, else moves the VGA hardware cursor.
pub fn move_cursor(col: usize, row: usize) {
    if GFX_CONSOLE.load(core::sync::atomic::Ordering::Relaxed) {
        crate::graphics::console::set_cursor(col, row);
        return;
    }
    let col = col.min(BUFFER_WIDTH - 1);
    let row = row.min(BUFFER_HEIGHT - 1);
    let pos = row * BUFFER_WIDTH + col;
    unsafe {
        Port::<u8>::new(CRTC_INDEX).write(CURSOR_HIGH);
        Port::<u8>::new(CRTC_DATA).write((pos >> 8) as u8);
        Port::<u8>::new(CRTC_INDEX).write(CURSOR_LOW);
        Port::<u8>::new(CRTC_DATA).write(pos as u8);
    }
}

/// When true, kernel text output is rendered to the framebuffer graphics
/// console instead of the 0xB8000 VGA text buffer.  Set when the kernel boots
/// with a GRUB framebuffer (graphics mode), where 0xB8000 is not displayed.
pub static GFX_CONSOLE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Route subsequent kernel text output to the graphics console.
pub fn use_gfx_console(on: bool) {
    GFX_CONSOLE.store(on, core::sync::atomic::Ordering::Relaxed);
}

/// Output capture for shell redirection / pipes.  When `Some`, `_print` appends
/// formatted text here instead of drawing to the console — the shell sets this
/// around a command whose stdout is going to a file or another command.
pub static OUTPUT_CAPTURE: spin::Mutex<Option<alloc::string::String>> = spin::Mutex::new(None);

/// Begin capturing stdout; any previous capture is returned.
pub fn capture_begin() -> Option<alloc::string::String> {
    (*OUTPUT_CAPTURE.lock()).replace(alloc::string::String::new())
}
/// Stop capturing and return what was captured, restoring a prior capture.
pub fn capture_end(prev: Option<alloc::string::String>) -> alloc::string::String {
    core::mem::replace(&mut *OUTPUT_CAPTURE.lock(), prev).unwrap_or_default()
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;

    // Redirected stdout: append to the capture buffer and skip the console.
    {
        let mut cap = OUTPUT_CAPTURE.lock();
        if let Some(buf) = cap.as_mut() {
            let _ = buf.write_fmt(args);
            return;
        }
    }

    let gfx = GFX_CONSOLE.load(core::sync::atomic::Ordering::Relaxed);
    let ready = crate::journal::ready();

    if gfx || ready {
        // Heap is available (graphics mode and the journal both come up after
        // init_heap), so format once into a String and reuse it.
        let mut buf = alloc::string::String::new();
        let _ = buf.write_fmt(args);
        if gfx {
            // The graphics CONSOLE lock is thread-context only (no IRQ handler
            // touches it), so it stays a plain spinlock with interrupts ENABLED:
            // a contending thread spins preemptibly and the holder is rescheduled
            // to release it.  Disabling interrupts here would deadlock instead —
            // see graphics::console::write_str.
            crate::graphics::console::write_str(&buf);
        } else {
            // WRITER is also thread-context only — keep it a plain spinlock.
            let mut w = WRITER.lock();
            let _ = w.write_str(&buf);
            w.update_hw_cursor();
        }
        // SERIAL is locked from IRQ handlers too (timer/keyboard logging), so the
        // hold MUST disable interrupts — otherwise an IRQ firing while a thread
        // holds it spins forever (IF=0 in the handler) → hard deadlock.
        crate::arch::without_interrupts(|| {
            crate::driver::serial::SERIAL.lock().write_str(&buf);
        });
        if ready {
            crate::journal::log(&buf);
        }
    } else {
        // Early boot, before the heap: write directly with no allocation.
        {
            let mut w = WRITER.lock();
            w.write_fmt(args).unwrap();
            w.update_hw_cursor();
        }
        crate::arch::without_interrupts(|| {
            crate::driver::serial::SERIAL.lock().write_fmt(args).ok();
        });
    }
}
