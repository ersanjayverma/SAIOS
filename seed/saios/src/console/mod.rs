//! Kernel console subsystem.
//!
//! The console multiplexes output between a serial port and an optional
//! framebuffer renderer. It also handles keyboard/mouse input events and
//! provides the `println!` style output used by the rest of the kernel.

mod backend;
mod cursor;
#[allow(dead_code)]
mod framebuffer;
mod input;
mod keyboard;
mod mouse;
mod serial;
pub mod tests;
mod vga;
mod visual;

use alloc::string::String as AllocString;
use alloc::vec::Vec;
use core::fmt::{self, Write};

use crate::driver::usb;
use crate::kernel::device;
use crate::kernel::driver;
use backend::{ConsoleBackend, MirrorConsole};
use core::sync::atomic::{AtomicBool, Ordering};
use cursor::Cursor;
use efi_main::graphics::FramebufferInfo;
use hal::arch::x86_64::sync::StaticCell;
use heapless::String;
use input::InputBuffer;
use keyboard::KeyboardDriver;
use mouse::MouseDriver;
use serial::{SerialConsole, poll_input_event as poll_serial_input_event};
use static_assertions::const_assert;
use unicode_width::UnicodeWidthChar;
use visual::VisualConsole;

pub use keyboard::KeyEvent;
pub use mouse::{MouseButtons, MouseEvent};

const DEFAULT_WIDTH: usize = 80;
const DEFAULT_HEIGHT: usize = 25;
const TAB_WIDTH: usize = 4;
const MAX_WIDTH: usize = 160;
const MAX_HEIGHT: usize = 100;

const_assert!(MAX_WIDTH >= DEFAULT_WIDTH);
const_assert!(MAX_HEIGHT >= DEFAULT_HEIGHT);

struct Console<B: ConsoleBackend> {
    backend: B,
    cursor: Cursor,
    buffer: [[char; MAX_WIDTH]; MAX_HEIGHT],
}

struct OutputCapture {
    active: bool,
    suppress_console: bool,
    buffer: AllocString,
    stack: Vec<CaptureFrame>,
}

struct CaptureFrame {
    suppress_console: bool,
    buffer: AllocString,
}

impl OutputCapture {
    const fn new() -> Self {
        Self {
            active: false,
            suppress_console: false,
            buffer: AllocString::new(),
            stack: Vec::new(),
        }
    }
}

impl<B: ConsoleBackend> Console<B> {
    const fn new(backend: B, width: usize, height: usize) -> Self {
        Self {
            backend,
            cursor: Cursor::new(width, height),
            buffer: [[' '; MAX_WIDTH]; MAX_HEIGHT],
        }
    }

    fn init(&mut self) {
        self.clear();
    }

    fn resize(&mut self, width: usize, height: usize) {
        self.cursor.width = core::cmp::min(width, MAX_WIDTH).max(1);
        self.cursor.height = core::cmp::min(height, MAX_HEIGHT).max(1);
        self.clear();
    }

    fn sync_cursor(&mut self) {
        self.cursor.show();
        self.backend.set_cursor(self.cursor.x, self.cursor.y);
    }

    fn put_char_inner(&mut self, c: char, sync_cursor: bool, emit_backend: bool) {
        match c {
            '\n' => self.newline(),
            '\r' => {
                self.cursor.x = 0;
                if sync_cursor {
                    self.sync_cursor();
                }
            }
            '\t' => {
                let spaces = TAB_WIDTH - (self.cursor.x % TAB_WIDTH);
                for _ in 0..spaces {
                    self.put_char_inner(' ', sync_cursor, emit_backend);
                }
            }
            '\x08' => {
                if self.cursor.x > 0 {
                    self.cursor.x -= 1;
                    self.buffer[self.cursor.y][self.cursor.x] = ' ';
                    if emit_backend {
                        self.backend.put_char(' ');
                    }
                    if sync_cursor {
                        self.sync_cursor();
                    }
                }
            }
            ch => {
                self.buffer[self.cursor.y][self.cursor.x] = ch;
                if emit_backend {
                    self.backend.put_char(ch);
                }
                self.cursor.x += 1;
                if self.cursor.x >= self.cursor.width {
                    self.newline();
                } else if sync_cursor {
                    self.sync_cursor();
                }
            }
        }
    }

    fn put_char(&mut self, c: char) {
        if capture_char(c) && should_suppress_output() {
            return;
        }

        self.put_char_inner(c, true, true);
    }

    fn write_str(&mut self, s: &str) {
        if capture_str(s) && should_suppress_output() {
            return;
        }

        for c in s.chars() {
            // Render each character once while deferring cursor sync until the end.
            self.put_char_inner(c, false, true);
        }

        self.sync_cursor();
    }

    fn clear(&mut self) {
        for y in 0..self.cursor.height {
            for x in 0..self.cursor.width {
                self.buffer[y][x] = ' ';
            }
        }
        self.cursor.x = 0;
        self.cursor.y = 0;
        self.backend.clear();
        self.backend.set_cursor(0, 0);
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor.x = core::cmp::min(x, self.cursor.width.saturating_sub(1));
        self.cursor.y = core::cmp::min(y, self.cursor.height.saturating_sub(1));
        self.sync_cursor();
    }

    fn move_cursor_left(&mut self) {
        if self.cursor.x > 0 {
            self.cursor.x -= 1;
            self.sync_cursor();
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor.x + 1 < self.cursor.width {
            self.cursor.x += 1;
            self.sync_cursor();
        }
    }

    fn newline(&mut self) {
        // Backends like serial rely on explicit newline bytes; cursor moves alone are not visible.
        self.backend.put_char('\n');
        self.cursor.x = 0;
        self.cursor.y += 1;

        if self.cursor.y >= self.cursor.height {
            self.scroll();
            self.cursor.y = self.cursor.height - 1;
        }

        self.sync_cursor();
    }

    fn scroll(&mut self) {
        self.buffer[..self.cursor.height].copy_within(1..self.cursor.height, 0);

        let last = self.cursor.height - 1;
        self.buffer[last][..self.cursor.width].fill(' ');

        if self.backend.scroll_up(1) {
            // Cursor sync happens once in `newline` after scroll completes.
        } else {
            // Throughput-first mode: skip full redraw fallback. This avoids
            // O(screen_size) per-scroll work when a backend cannot accelerate.
        }
    }

    #[allow(dead_code)]
    fn redraw(&mut self) {
        self.backend.clear();
        for y in 0..self.cursor.height {
            self.backend.set_cursor(0, y);
            for x in 0..self.cursor.width {
                self.backend.put_char(self.buffer[y][x]);
            }
        }

        self.backend.set_cursor(self.cursor.x, self.cursor.y);
    }
}

impl<B: ConsoleBackend> Write for Console<B> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s);
        Ok(())
    }
}

type DefaultBackend = MirrorConsole<SerialConsole, VisualConsole>;

static CONSOLE: StaticCell<Console<DefaultBackend>> = StaticCell::new(Console::new(
    MirrorConsole::new(SerialConsole::new(), VisualConsole::new()),
    DEFAULT_WIDTH,
    DEFAULT_HEIGHT,
));

static CONSOLE_INITIALIZED: AtomicBool = AtomicBool::new(false);
static CONSOLE_LOCKED: AtomicBool = AtomicBool::new(false);
static CAPTURE_LOCKED: AtomicBool = AtomicBool::new(false);
static INPUT_BUFFER: StaticCell<InputBuffer> = StaticCell::new(InputBuffer::new());
static KEYBOARD: StaticCell<KeyboardDriver> = StaticCell::new(KeyboardDriver::new());
static MOUSE: StaticCell<MouseDriver> = StaticCell::new(MouseDriver::new());
static INPUT_PROMPT: StaticCell<String<64>> = StaticCell::new(String::new());
static OUTPUT_CAPTURE: StaticCell<OutputCapture> = StaticCell::new(OutputCapture::new());

fn capture_lock() {
    while CAPTURE_LOCKED
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn capture_unlock() {
    CAPTURE_LOCKED.store(false, Ordering::Release);
}

fn capture_char(c: char) -> bool {
    capture_lock();
    // SAFETY: guarded by capture lock.
    let capture = unsafe { &mut *OUTPUT_CAPTURE.get() };
    let active = capture.active;
    if active {
        capture.buffer.push(c);
    }
    capture_unlock();
    active
}

fn capture_str(s: &str) -> bool {
    capture_lock();
    // SAFETY: guarded by capture lock.
    let capture = unsafe { &mut *OUTPUT_CAPTURE.get() };
    let active = capture.active;
    if active {
        capture.buffer.push_str(s);
    }
    capture_unlock();
    active
}

fn should_suppress_output() -> bool {
    capture_lock();
    // SAFETY: guarded by capture lock.
    let suppress = unsafe {
        let capture = &*OUTPUT_CAPTURE.get();
        capture.active && capture.suppress_console
    };
    capture_unlock();
    suppress
}

fn with_console<R>(f: impl FnOnce(&mut Console<DefaultBackend>) -> R) -> R {
    // SAFETY: single-core early kernel context; mutable global console singleton.
    unsafe { f(&mut *CONSOLE.get()) }
}

fn try_with_console<R>(f: impl FnOnce(&mut Console<DefaultBackend>) -> R) -> Option<R> {
    if CONSOLE_LOCKED
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return None;
    }

    let out = with_console(f);
    CONSOLE_LOCKED.store(false, Ordering::Release);
    Some(out)
}

fn emergency_write_str(s: &str) {
    SerialConsole::emergency_write_str(s);
}

/// Advance the cursor blink state on every timer tick.  This is called from
/// the timer interrupt handler so the cursor blinks at a regular rate.
pub fn on_timer_tick() {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _changed = try_with_console(|console| {
        if console.cursor.blink_on() {
            console.backend.blink_cursor();
        }
    });
}

/// Initializes the serial port, input devices and console state, and only
/// starts USB input probing when the PS/2 keyboard path fails.
pub fn init() {
    SerialConsole::init();
    let keyboard_ready = unsafe { (*KEYBOARD.get()).init() };
    let mut usb_input_ready = false;
    unsafe {
        (*MOUSE.get()).init();
    }
    if !keyboard_ready {
        if crate::heap::identity_mode_enabled() {
            hal::arch::x86_64::console::_print(format_args!(
                "console: PS/2 keyboard unavailable; USB HID probe skipped in fallback mode\n"
            ));
        } else {
            usb::init();
            usb_input_ready = usb::hid_input_ready();
            if usb_input_ready {
                hal::arch::x86_64::console::_print(format_args!(
                    "console: PS/2 keyboard unavailable; USB HID input fallback active\n"
                ));
            } else {
                hal::arch::x86_64::console::_print(format_args!(
                    "console: PS/2 keyboard unavailable; USB HID input not ready\n"
                ));
            }
        }
    }
    let _ = driver::ensure_driver(
        "serial",
        "0.1.0",
        "SAIOS",
        &[],
        driver::DriverStatus::Running,
    );
    let _ = driver::ensure_driver(
        "input",
        "0.1.0",
        "SAIOS",
        &["serial"],
        driver::DriverStatus::Running,
    );
    let _ = driver::ensure_driver(
        "hid",
        "0.1.0",
        "SAIOS",
        &["input"],
        driver::DriverStatus::Running,
    );
    let _ = driver::ensure_driver(
        "hid-keyboard",
        "0.1.0",
        "SAIOS",
        &["hid"],
        if keyboard_ready || usb_input_ready {
            driver::DriverStatus::Running
        } else {
            driver::DriverStatus::Stopped
        },
    );
    let _ = driver::ensure_driver(
        "hid-mouse",
        "0.1.0",
        "SAIOS",
        &["hid"],
        driver::DriverStatus::Running,
    );
    // Keep legacy logical names for compatibility with existing scripts/tools.
    let _ = driver::ensure_driver(
        "mouse",
        "0.1.0",
        "SAIOS",
        &["hid-mouse"],
        driver::DriverStatus::Running,
    );
    let _ = device::ensure_device("COM1", "serial", "uart", device::DeviceStatus::Online);
    let _ = device::ensure_device(
        "keyboard0",
        "hid-keyboard",
        "hid-keyboard",
        if keyboard_ready || usb_input_ready {
            device::DeviceStatus::Online
        } else {
            device::DeviceStatus::Offline
        },
    );
    let _ = device::ensure_device(
        "mouse0",
        "hid-mouse",
        "hid-pointer",
        device::DeviceStatus::Online,
    );
    with_console(|console| console.init());
    unsafe {
        (*INPUT_BUFFER.get()).clear();
    }
    CONSOLE_INITIALIZED.store(true, Ordering::Release);
}

fn framebuffer_scrollback_up(lines: usize) {
    if lines == 0 {
        return;
    }
    let _ = try_with_console(|console| {
        let delta = core::cmp::min(lines, isize::MAX as usize) as isize;
        let _ = console.backend.right_mut().scroll_view_lines(delta);
    });
}

fn framebuffer_scrollback_down(lines: usize) {
    if lines == 0 {
        return;
    }
    let _ = try_with_console(|console| {
        let delta = core::cmp::min(lines, isize::MAX as usize) as isize;
        let _ = console.backend.right_mut().scroll_view_lines(-delta);
    });
}

fn framebuffer_scrollback_to_bottom() {
    let _ = try_with_console(|console| {
        let _ = console.backend.right_mut().scroll_to_bottom();
    });
}

fn move_cursor_left_cells(cells: usize) {
    if cells == 0 || !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| {
        console.cursor.x = console.cursor.x.saturating_sub(cells);
        console.sync_cursor();
    });
}

fn move_cursor_right_cells(cells: usize) {
    if cells == 0 || !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| {
        let max_x = console.cursor.width.saturating_sub(1);
        console.cursor.x = core::cmp::min(console.cursor.x.saturating_add(cells), max_x);
        console.sync_cursor();
    });
}

/// Attaches a framebuffer as an additional console output backend.
///
/// The framebuffer address is used as provided by the bootloader.
pub(crate) fn attach_framebuffer(info: FramebufferInfo) {
    let mapped_info = info;

    with_console(|console| {
        console.backend.right_mut().attach(mapped_info);
        if mapped_info.base != 0 && console.backend.right_mut().framebuffer_attached() {
            let _ = driver::ensure_driver(
                "framebuffer",
                "0.1.0",
                "SAIOS",
                &["serial"],
                driver::DriverStatus::Running,
            );
            let _ = device::ensure_device(
                "fb0",
                "framebuffer",
                "display",
                device::DeviceStatus::Online,
            );
        }
        if let (Some(columns), Some(rows)) = (
            console.backend.right_mut().text_columns(),
            console.backend.right_mut().text_rows(),
        ) {
            console.resize(columns, rows);
        }
    });
}

pub(crate) fn attach_framebuffer_direct(info: FramebufferInfo) {
    with_console(|console| {
        console.backend.right_mut().attach_direct(info);
        if info.base != 0 && console.backend.right_mut().framebuffer_attached() {
            let _ = driver::ensure_driver(
                "framebuffer",
                "0.1.0",
                "SAIOS",
                &["serial"],
                driver::DriverStatus::Running,
            );
            let _ = device::ensure_device(
                "fb0",
                "framebuffer",
                "display",
                device::DeviceStatus::Online,
            );
        }
        if let (Some(columns), Some(rows)) = (
            console.backend.right_mut().text_columns(),
            console.backend.right_mut().text_rows(),
        ) {
            console.resize(columns, rows);
        }
    });
}

/// Ensures the framebuffer renderer is ready and returns true on success.
pub fn promote_framebuffer_renderer() -> bool {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return false;
    }

    try_with_console(|console| {
        if !console.backend.right_mut().promote_framebuffer_renderer() {
            return false;
        }

        if let (Some(columns), Some(rows)) = (
            console.backend.right_mut().text_columns(),
            console.backend.right_mut().text_rows(),
        ) {
            console.cursor.width = core::cmp::min(columns, MAX_WIDTH).max(1);
            console.cursor.height = core::cmp::min(rows, MAX_HEIGHT).max(1);
            console.redraw();
        }

        true
    })
    .unwrap_or(false)
}

/// Returns the current console text grid size as `(columns, rows)`.
pub fn dimensions() -> (usize, usize) {
    try_with_console(|console| (console.cursor.width, console.cursor.height)).unwrap_or((0, 0))
}

/// Returns the current cursor position as `(x, y)` text cells.
pub fn cursor_position() -> (usize, usize) {
    try_with_console(|console| (console.cursor.x, console.cursor.y)).unwrap_or((0, 0))
}

/// Returns the number of lines stored in the framebuffer scrollback buffer.
pub fn scrollback_lines() -> usize {
    try_with_console(|console| console.backend.right_mut().scrollback_lines()).unwrap_or(0)
}

/// Returns the current scroll-back view offset in lines (0 means live bottom).
pub fn scrollback_offset() -> usize {
    try_with_console(|console| console.backend.right_mut().view_offset()).unwrap_or(0)
}

/// Returns true when the framebuffer backend is attached and ready.
pub fn framebuffer_attached() -> bool {
    try_with_console(|console| console.backend.right_mut().framebuffer_attached()).unwrap_or(false)
}

/// Returns the attached framebuffer properties, if any.
pub fn framebuffer_properties() -> Option<framebuffer::DisplayProperties> {
    try_with_console(|console| console.backend.right_mut().display_properties()).flatten()
}

/// Snapshot result for the `fbbench` command.
#[derive(Debug, Copy, Clone)]
pub struct FramebufferBenchResult {
    pub passes: usize,
    pub bytes_written: usize,
    pub elapsed_ticks: u64,
    pub elapsed_ms: u64,
    pub mib_per_sec: u64,
}

/// Benchmarks framebuffer full-screen clear throughput.
pub fn benchmark_framebuffer_clears(passes: usize) -> Option<FramebufferBenchResult> {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return None;
    }

    try_with_console(|console| console.backend.right_mut().benchmark_clears(passes)).flatten()
}

/// Enables or disables serial output logging.
pub fn set_serial_logging(enabled: bool) {
    SerialConsole::set_output_enabled(enabled);
}

/// Returns true if serial output logging is enabled.
pub fn serial_logging_enabled() -> bool {
    SerialConsole::output_enabled()
}

/// Writes a single character to the console.
pub fn put_char(c: char) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        if capture_char(c) && should_suppress_output() {
            return;
        }
        SerialConsole::emergency_put_char(c);
        return;
    }

    if try_with_console(|console| console.put_char(c)).is_none() {
        if capture_char(c) && should_suppress_output() {
            return;
        }
        SerialConsole::emergency_put_char(c);
    }
}

/// Writes a string to the console.
pub fn write_str(s: &str) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        if capture_str(s) && should_suppress_output() {
            return;
        }
        emergency_write_str(s);
        return;
    }

    if try_with_console(|console| console.write_str(s)).is_none() {
        if capture_str(s) && should_suppress_output() {
            return;
        }
        emergency_write_str(s);
    }
}

/// Clears the console screen.
pub fn clear() {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.clear());
}

/// Sets the prompt displayed before read-line input.
pub fn set_input_prompt(prompt: &str) {
    // SAFETY: single-core early kernel context.
    unsafe {
        let slot = &mut *INPUT_PROMPT.get();
        slot.clear();
        for ch in prompt.chars() {
            if slot.push(ch).is_err() {
                break;
            }
        }
    }
}

/// Begins capturing console output to an internal buffer.
pub fn begin_output_capture(suppress_console: bool) {
    capture_lock();
    // SAFETY: guarded by capture lock.
    unsafe {
        let capture = &mut *OUTPUT_CAPTURE.get();
        if capture.active {
            // Nested captures are used by some subsystems (for diagnostics).
            // Keep outer capture state so shell redirection remains intact.
            let previous = CaptureFrame {
                suppress_console: capture.suppress_console,
                buffer: core::mem::take(&mut capture.buffer),
            };
            capture.stack.push(previous);
            capture.suppress_console = suppress_console || capture.suppress_console;
            capture.buffer.clear();
            capture_unlock();
            return;
        }

        capture.active = true;
        capture.suppress_console = suppress_console;
        capture.buffer.clear();
    }
    capture_unlock();
}

/// Ends output capture and returns the captured text.
pub fn end_output_capture() -> AllocString {
    capture_lock();
    // SAFETY: guarded by capture lock.
    let out = unsafe {
        let capture = &mut *OUTPUT_CAPTURE.get();
        let current = core::mem::take(&mut capture.buffer);
        if let Some(mut previous) = capture.stack.pop() {
            // Nested capture output is still part of the outer command output.
            previous.buffer.push_str(current.as_str());
            capture.active = true;
            capture.suppress_console = previous.suppress_console;
            capture.buffer = previous.buffer;
        } else {
            capture.active = false;
            capture.suppress_console = false;
        }
        current
    };
    capture_unlock();
    out
}

/// Moves the text cursor to `(x, y)`.
pub fn set_cursor(x: usize, y: usize) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.set_cursor(x, y));
}

/// Moves the text cursor one cell to the left.
pub fn move_cursor_left() {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.move_cursor_left());
}

/// Moves the text cursor one cell to the right.
pub fn move_cursor_right() {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.move_cursor_right());
}

/// Writes a newline to the console.
pub fn newline() {
    put_char('\n');
}

/// Writes a string to the console.
pub fn print(s: &str) {
    write_str(s);
}

/// Writes a string to the stderr stream.
///
/// The current implementation keeps stderr visible on the same physical
/// console devices but marks output distinctly for stream separation.
pub fn stderr_write_str(s: &str) {
    write_str("[stderr] ");
    write_str(s);
}

/// Writes a line to the stderr stream.
pub fn stderr_println(s: &str) {
    stderr_write_str(s);
    newline();
}

/// Writes formatted arguments to the console.
pub fn print_fmt(args: fmt::Arguments) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        let rendered = alloc::format!("{}", args);
        if capture_str(rendered.as_str()) && should_suppress_output() {
            return;
        }
        hal::arch::x86_64::console::_print(format_args!("{}", rendered));
        return;
    }

    if CONSOLE_LOCKED
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
    {
        with_console(|console| {
            let _ = console.write_fmt(args);
        });
        CONSOLE_LOCKED.store(false, Ordering::Release);
        return;
    }

    let rendered = alloc::format!("{}", args);
    if capture_str(rendered.as_str()) && should_suppress_output() {
        return;
    }
    hal::arch::x86_64::console::_print(format_args!("{}", rendered));
}

/// Emergency string output used during panics.
pub fn panic_write_str(s: &str) {
    emergency_write_str(s);
}

/// Emergency line output used during panics.
pub fn panic_println(s: &str) {
    panic_write_str(s);
    panic_write_str("\n");
}

/// Polls for keyboard/mouse input and returns a processed input line if
/// available.
pub fn poll_input() -> Option<String<256>> {
    fn cell_width(ch: char) -> usize {
        UnicodeWidthChar::width(ch).unwrap_or(1).max(1)
    }

    fn line_cells(s: &str) -> usize {
        s.chars().map(cell_width).sum()
    }

    fn cursor_cells(s: &str, cursor_chars: usize) -> usize {
        s.chars().take(cursor_chars).map(cell_width).sum()
    }

    fn common_prefix_chars(a: &str, b: &str) -> usize {
        a.chars()
            .zip(b.chars())
            .take_while(|(left, right)| left == right)
            .count()
    }

    fn slice_from_char_index(s: &str, index: usize) -> &str {
        if index == 0 {
            return s;
        }

        for (char_index, (byte_index, _)) in s.char_indices().enumerate() {
            if char_index == index {
                return &s[byte_index..];
            }
        }

        ""
    }

    fn print_spaces(mut count: usize) {
        const SPACES_64: &str = "                                                                ";

        while count >= 64 {
            print(SPACES_64);
            count -= 64;
        }

        if count > 0 {
            print(&SPACES_64[..count]);
        }
    }

    fn redraw_line(prev_rendered: &str, prev_cursor_cells: usize) {
        // SAFETY: single-core early kernel context.
        let input = unsafe { &mut *INPUT_BUFFER.get() };

        let rendered = input.render();
        let new_len_cells = line_cells(rendered.as_str());
        let new_cursor_cells = cursor_cells(rendered.as_str(), input.cursor());
        let prefix_chars = common_prefix_chars(prev_rendered, rendered.as_str());
        let prefix_cells = cursor_cells(prev_rendered, prefix_chars);
        let old_suffix_cells = line_cells(slice_from_char_index(prev_rendered, prefix_chars));
        let new_suffix = slice_from_char_index(rendered.as_str(), prefix_chars);
        let new_suffix_cells = line_cells(new_suffix);

        // Move to line start once, then redraw only the changed suffix.
        if prev_cursor_cells > 0 {
            move_cursor_left_cells(prev_cursor_cells);
        }

        if prefix_cells > 0 {
            move_cursor_right_cells(prefix_cells);
        }

        if !new_suffix.is_empty() {
            print(new_suffix);
        }

        if old_suffix_cells > new_suffix_cells {
            let blank_cells = old_suffix_cells - new_suffix_cells;
            print_spaces(blank_cells);
            move_cursor_left_cells(blank_cells);
        }

        if new_len_cells > new_cursor_cells {
            move_cursor_left_cells(new_len_cells - new_cursor_cells);
        } else if new_cursor_cells > new_len_cells {
            move_cursor_right_cells(new_cursor_cells - new_len_cells);
        }
    }

    // SAFETY: single-core early kernel context.
    if let Some(mouse_event) = unsafe { (*MOUSE.get()).poll_event() }.or_else(usb::poll_mouse_event)
    {
        match mouse_event {
            MouseEvent::Wheel { delta, .. } => {
                if delta > 0 {
                    framebuffer_scrollback_up((delta as usize).saturating_mul(8));
                } else if delta < 0 {
                    framebuffer_scrollback_down(((-delta) as usize).saturating_mul(8));
                }
            }
            MouseEvent::Move { dy, buttons, .. } => {
                // Fallback gesture when wheel is unavailable: hold middle button and move.
                if buttons.middle {
                    if dy > 0 {
                        framebuffer_scrollback_up((dy as usize).max(2));
                    } else if dy < 0 {
                        framebuffer_scrollback_down(((-dy) as usize).max(2));
                    }
                }
            }
        }
    }

    // SAFETY: single-core early kernel context.
    let key_event = unsafe { (*KEYBOARD.get()).poll_event() }
        .or_else(usb::poll_key_event)
        .or_else(poll_serial_input_event)?;
    framebuffer_scrollback_to_bottom();

    // SAFETY: single-core early kernel context.
    let (
        prev_rendered,
        prev_len_cells,
        prev_cursor_cells,
        prev_cursor_char_width,
        prev_right_char_width,
    ) = unsafe {
        let input = &*INPUT_BUFFER.get();
        let rendered = input.render();
        let cursor_chars = input.cursor();
        let len_cells = line_cells(rendered.as_str());
        let cursor_cells = cursor_cells(rendered.as_str(), cursor_chars);
        (
            rendered,
            len_cells,
            cursor_cells,
            input.char_left_of_cursor().map(cell_width).unwrap_or(1),
            input.char_at_cursor().map(cell_width).unwrap_or(1),
        )
    };

    match key_event {
        KeyEvent::Character(ch) => {
            let inserted = unsafe { (*INPUT_BUFFER.get()).insert(ch) };
            if inserted {
                if prev_cursor_cells == prev_len_cells {
                    put_char(ch);
                } else {
                    redraw_line(prev_rendered.as_str(), prev_cursor_cells);
                }
            }
            None
        }
        KeyEvent::Backspace => {
            let erased = unsafe { (*INPUT_BUFFER.get()).backspace() };
            if erased {
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::Delete => {
            let erased = unsafe { (*INPUT_BUFFER.get()).delete() };
            if erased {
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::Insert => None,
        KeyEvent::Home => {
            if unsafe { (*INPUT_BUFFER.get()).move_home() } {
                move_cursor_left_cells(prev_cursor_cells);
            }
            None
        }
        KeyEvent::End => {
            if unsafe { (*INPUT_BUFFER.get()).move_end() } {
                move_cursor_right_cells(prev_len_cells.saturating_sub(prev_cursor_cells));
            }
            None
        }
        KeyEvent::PageUp => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_prev() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::PageDown => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_next() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::ArrowLeft => {
            if unsafe { (*INPUT_BUFFER.get()).move_left() } {
                move_cursor_left_cells(prev_cursor_char_width);
            }
            None
        }
        KeyEvent::ArrowRight => {
            if unsafe { (*INPUT_BUFFER.get()).move_right() } {
                move_cursor_right_cells(prev_right_char_width);
            }
            None
        }
        KeyEvent::ShiftArrowLeft => {
            if unsafe { (*INPUT_BUFFER.get()).move_left() } {
                move_cursor_left_cells(prev_cursor_char_width);
            }
            None
        }
        KeyEvent::ShiftArrowRight => {
            if unsafe { (*INPUT_BUFFER.get()).move_right() } {
                move_cursor_right_cells(prev_right_char_width);
            }
            None
        }
        KeyEvent::ShiftArrowUp => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_prev() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::ShiftArrowDown => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_next() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlArrowLeft | KeyEvent::CtrlShiftArrowLeft => {
            if unsafe { (*INPUT_BUFFER.get()).move_prev_word() } {
                let new_cursor_chars = unsafe { (*INPUT_BUFFER.get()).cursor() };
                let new_cursor_cells = cursor_cells(prev_rendered.as_str(), new_cursor_chars);
                if prev_cursor_cells > new_cursor_cells {
                    move_cursor_left_cells(prev_cursor_cells - new_cursor_cells);
                } else if new_cursor_cells > prev_cursor_cells {
                    move_cursor_right_cells(new_cursor_cells - prev_cursor_cells);
                }
            }
            None
        }
        KeyEvent::CtrlArrowRight | KeyEvent::CtrlShiftArrowRight => {
            if unsafe { (*INPUT_BUFFER.get()).move_next_word() } {
                let new_cursor_chars = unsafe { (*INPUT_BUFFER.get()).cursor() };
                let new_cursor_cells = cursor_cells(prev_rendered.as_str(), new_cursor_chars);
                if prev_cursor_cells > new_cursor_cells {
                    move_cursor_left_cells(prev_cursor_cells - new_cursor_cells);
                } else if new_cursor_cells > prev_cursor_cells {
                    move_cursor_right_cells(new_cursor_cells - prev_cursor_cells);
                }
            }
            None
        }
        KeyEvent::CtrlArrowUp | KeyEvent::CtrlShiftArrowUp => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_prev() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlArrowDown | KeyEvent::CtrlShiftArrowDown => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_next() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::FKey(key) => {
            let _ = key;
            None
        }
        KeyEvent::ArrowUp => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_prev() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::ArrowDown => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_next() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlC => {
            let _ = crate::kernel::process::signal_foreground_group(2);
            newline();
            unsafe { (*INPUT_BUFFER.get()).clear() };
            Some(String::new())
        }
        KeyEvent::CtrlA => {
            if unsafe { (*INPUT_BUFFER.get()).move_home() } {
                move_cursor_left_cells(prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlE => {
            if unsafe { (*INPUT_BUFFER.get()).move_end() } {
                move_cursor_right_cells(prev_len_cells.saturating_sub(prev_cursor_cells));
            }
            None
        }
        KeyEvent::CtrlD => {
            let erased = unsafe { (*INPUT_BUFFER.get()).delete() };
            if erased {
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlU => {
            let changed = unsafe { (*INPUT_BUFFER.get()).clear_to_start() };
            if changed {
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlK => {
            let changed = unsafe { (*INPUT_BUFFER.get()).clear_to_end() };
            if changed {
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlL => {
            clear();

            // SAFETY: single-core early kernel context.
            let prompt = unsafe { (*INPUT_PROMPT.get()).clone() };
            print(prompt.as_str());

            // SAFETY: single-core early kernel context.
            let input = unsafe { &mut *INPUT_BUFFER.get() };
            let rendered = input.render();
            print(rendered.as_str());

            let rendered_cells = line_cells(rendered.as_str());
            let cursor_cells = cursor_cells(rendered.as_str(), input.cursor());
            move_cursor_left_cells(rendered_cells.saturating_sub(cursor_cells));

            None
        }
        KeyEvent::CtrlW => {
            let changed = unsafe { (*INPUT_BUFFER.get()).delete_prev_word() };
            if changed {
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::Enter => {
            newline();
            let line = unsafe { (*INPUT_BUFFER.get()).submit() };
            Some(line)
        }
        KeyEvent::Tab => {
            let rendered = unsafe { (*INPUT_BUFFER.get()).render() };
            let cursor = unsafe { (*INPUT_BUFFER.get()).cursor() };
            let completed = crate::shell::complete_for_console(rendered.as_str(), cursor);
            if let Some(new_line) = completed {
                unsafe { (*INPUT_BUFFER.get()).set_line(new_line.as_str()) };
                redraw_line(prev_rendered.as_str(), prev_cursor_cells);
            }
            None
        }
        KeyEvent::Escape => None,
    }
}

/// Blocks until a complete input line is available and returns it.
pub fn read_line() -> String<256> {
    loop {
        if let Some(line) = poll_input() {
            return line;
        }
        hal::arch::x86_64::cpu::pause();
    }
}

pub fn is_initialized() -> bool {
    CONSOLE_INITIALIZED.load(Ordering::Acquire)
}

pub fn verify() -> crate::kernel::testing::report::VerifyReport {
    let mut checks = Vec::new();

    checks.push(if is_initialized() {
        crate::kernel::testing::report::VerifyCheck::pass("Console init", "console initialized")
    } else {
        crate::kernel::testing::report::VerifyCheck::fail("Console init", "console not initialized")
    });

    checks.push(if !CONSOLE_LOCKED.load(Ordering::Acquire) {
        crate::kernel::testing::report::VerifyCheck::pass("Console lock", "lock not stuck")
    } else {
        crate::kernel::testing::report::VerifyCheck::fail("Console lock", "lock currently held")
    });

    crate::kernel::testing::report::VerifyReport {
        target: "console",
        checks,
    }
}

#[macro_export]
macro_rules! console_println {
    () => {
        $crate::console::newline();
    };
    ($($arg:tt)*) => {{
        $crate::console::print_fmt(format_args!($($arg)*));
        $crate::console::newline();
    }};
}

pub use crate::console_println as println;
