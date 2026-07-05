use core::ptr;

use efi_main::graphics::FramebufferInfo;

use super::FramebufferBenchResult;
use super::backend::ConsoleBackend;
use super::framebuffer::DisplayProperties;
use crate::vmm;

const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;
const VGA_TAB_WIDTH: usize = 4;
const VGA_ATTR: u8 = 0x0F;
const VGA_PHYS_BASE: u64 = 0xB8000;
const VGA_SCROLLBACK_LINES: usize = 0;
const VGA_CRTC_INDEX: u16 = 0x3D4;
const VGA_CRTC_DATA: u16 = 0x3D5;
const VGA_CURSOR_HIGH: u8 = 0x0E;
const VGA_CURSOR_LOW: u8 = 0x0F;

/// Text-mode VGA console backend.
pub struct VgaConsole {
    base: Option<*mut u16>,
    cursor_x: usize,
    cursor_y: usize,
}

impl VgaConsole {
    /// Creates an unattached VGA backend.
    pub const fn new() -> Self {
        Self {
            base: None,
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    /// Returns the VGA text width.
    pub fn text_columns(&self) -> Option<usize> {
        Some(VGA_WIDTH)
    }

    /// Returns the VGA text height.
    pub fn text_rows(&self) -> Option<usize> {
        Some(VGA_HEIGHT)
    }

    /// Returns zero because VGA text mode does not maintain scrollback.
    pub fn scrollback_lines(&self) -> usize {
        VGA_SCROLLBACK_LINES
    }

    /// Returns zero because the VGA backend does not expose viewport scrolling.
    pub fn view_offset(&self) -> usize {
        0
    }

    /// Reports the VGA backend as ready once it has been attached or mapped.
    #[allow(dead_code)]
    pub fn ensure_renderer_ready(&mut self) -> bool {
        self.ensure_mapped().is_some()
    }

    /// Accepts framebuffer metadata and clears the VGA screen for a fresh boot console.
    pub fn attach(&mut self, _info: FramebufferInfo) {
        let _ = self.ensure_mapped();
        self.clear();
    }

    /// Accepts direct-attach requests for API compatibility.
    pub fn attach_direct(&mut self, _info: FramebufferInfo) {
        let _ = self.ensure_mapped();
        self.clear();
    }

    /// Returns the VGA text-mode properties for compatibility with console queries.
    pub fn display_properties(&self) -> Option<DisplayProperties> {
        None
    }

    /// VGA text mode does not support framebuffer clear benchmarking.
    pub fn benchmark_clears(&mut self, _passes: usize) -> Option<FramebufferBenchResult> {
        None
    }

    /// Attempts to show a scrollback viewport change, which VGA text mode does not support.
    pub fn scroll_view_lines(&mut self, _lines: isize) -> bool {
        false
    }

    /// Jumps back to the live bottom, which is a no-op for the VGA backend.
    pub fn scroll_to_bottom(&mut self) -> bool {
        false
    }

    /// Advances the visible cursor to the next row and scrolls when necessary.
    fn newline(&mut self) {
        self.cursor_x = 0;
        if self.cursor_y + 1 >= VGA_HEIGHT {
            let _ = self.scroll_up(1);
            self.cursor_y = VGA_HEIGHT - 1;
        } else {
            self.cursor_y += 1;
        }
        self.update_cursor();
    }

    /// Writes one cell into the VGA text buffer.
    fn write_cell(&mut self, x: usize, y: usize, c: char) {
        let Some(base) = self.ensure_mapped() else {
            return;
        };

        let ch = if c.is_ascii() { c as u8 } else { b'?' };
        let idx = y * VGA_WIDTH + x;
        let value = u16::from_le_bytes([ch, VGA_ATTR]);
        unsafe {
            ptr::write_volatile(base.add(idx), value);
        }
    }

    /// Moves the hardware cursor to the current text position.
    fn update_cursor(&mut self) {
        if self.ensure_mapped().is_none() {
            return;
        }

        let pos = (self.cursor_y * VGA_WIDTH + self.cursor_x) as u16;
        hal::arch::x86_64::io::outb(VGA_CRTC_INDEX, VGA_CURSOR_HIGH);
        hal::arch::x86_64::io::outb(VGA_CRTC_DATA, (pos >> 8) as u8);
        hal::arch::x86_64::io::outb(VGA_CRTC_INDEX, VGA_CURSOR_LOW);
        hal::arch::x86_64::io::outb(VGA_CRTC_DATA, pos as u8);
    }

    /// Scrolls the VGA text buffer up by `rows` rows.
    fn scroll_up_rows(&mut self, rows: usize) {
        let Some(base) = self.ensure_mapped() else {
            return;
        };

        let rows = core::cmp::min(rows.max(1), VGA_HEIGHT);
        let visible = (VGA_HEIGHT - rows) * VGA_WIDTH;
        unsafe {
            ptr::copy(base.add(rows * VGA_WIDTH), base, visible);
        }

        for y in VGA_HEIGHT - rows..VGA_HEIGHT {
            for x in 0..VGA_WIDTH {
                self.write_cell(x, y, ' ');
            }
        }
    }

    /// Ensures the VGA text buffer is mapped and returns a raw pointer to it.
    fn ensure_mapped(&mut self) -> Option<*mut u16> {
        if let Some(base) = self.base {
            return Some(base);
        }

        let virt = vmm::map_physical_anywhere(
            VGA_PHYS_BASE,
            1,
            vmm::FLAG_READ | vmm::FLAG_WRITE | vmm::FLAG_DEVICE,
            "vga-text",
        )
        .ok()?;

        let base = virt as *mut u16;
        self.base = Some(base);
        Some(base)
    }
}

impl ConsoleBackend for VgaConsole {
    /// Writes a single character into VGA text mode.
    fn put_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => {
                self.cursor_x = 0;
                self.update_cursor();
            }
            '\t' => {
                let spaces = VGA_TAB_WIDTH - (self.cursor_x % VGA_TAB_WIDTH);
                for _ in 0..spaces {
                    self.put_char(' ');
                }
            }
            '\x08' => {
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                    self.write_cell(self.cursor_x, self.cursor_y, ' ');
                    self.update_cursor();
                }
            }
            ch => {
                let x = self.cursor_x.min(VGA_WIDTH.saturating_sub(1));
                let y = self.cursor_y.min(VGA_HEIGHT.saturating_sub(1));
                self.write_cell(x, y, ch);
                self.cursor_x = self
                    .cursor_x
                    .saturating_add(1)
                    .min(VGA_WIDTH.saturating_sub(1));
                if self.cursor_x >= VGA_WIDTH {
                    self.newline();
                } else {
                    self.update_cursor();
                }
            }
        }
    }

    /// Writes a whole string and updates the visible cursor once at the end.
    fn put_str(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '\n' => self.newline(),
                '\r' => {
                    self.cursor_x = 0;
                }
                '\t' => {
                    let spaces = VGA_TAB_WIDTH - (self.cursor_x % VGA_TAB_WIDTH);
                    for _ in 0..spaces {
                        self.write_cell(self.cursor_x, self.cursor_y, ' ');
                        self.cursor_x = self
                            .cursor_x
                            .saturating_add(1)
                            .min(VGA_WIDTH.saturating_sub(1));
                        if self.cursor_x >= VGA_WIDTH {
                            self.newline();
                        }
                    }
                }
                '\x08' => {
                    if self.cursor_x > 0 {
                        self.cursor_x -= 1;
                        self.write_cell(self.cursor_x, self.cursor_y, ' ');
                    }
                }
                ch => {
                    let x = self.cursor_x.min(VGA_WIDTH.saturating_sub(1));
                    let y = self.cursor_y.min(VGA_HEIGHT.saturating_sub(1));
                    self.write_cell(x, y, ch);
                    self.cursor_x = self
                        .cursor_x
                        .saturating_add(1)
                        .min(VGA_WIDTH.saturating_sub(1));
                    if self.cursor_x >= VGA_WIDTH {
                        self.newline();
                    }
                }
            }
        }
        self.update_cursor();
    }

    /// Clears the VGA text buffer.
    fn clear(&mut self) {
        let Some(base) = self.ensure_mapped() else {
            return;
        };

        let blank = u16::from_le_bytes([b' ', VGA_ATTR]);
        for i in 0..(VGA_WIDTH * VGA_HEIGHT) {
            unsafe {
                ptr::write_volatile(base.add(i), blank);
            }
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.update_cursor();
    }

    /// Sets the hardware cursor to a specific text cell.
    fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor_x = core::cmp::min(x, VGA_WIDTH.saturating_sub(1));
        self.cursor_y = core::cmp::min(y, VGA_HEIGHT.saturating_sub(1));
        self.update_cursor();
    }

    /// Scrolls the visible text by whole rows.
    fn scroll_up(&mut self, rows: usize) -> bool {
        self.scroll_up_rows(rows);
        true
    }

    /// VGA text mode does not expose a blinking overlay cursor here.
    fn blink_cursor(&mut self) {}
}
