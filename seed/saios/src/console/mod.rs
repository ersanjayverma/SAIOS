mod backend;
mod cursor;
mod framebuffer;
mod input;
mod keyboard;
mod serial;
pub mod tests;

use core::fmt::{self, Write};
use alloc::string::String as AllocString;
use alloc::vec::Vec;

use backend::{ConsoleBackend, MirrorConsole};
use cursor::Cursor;
use framebuffer::FramebufferConsole;
use hal::arch::x86_64::sync::StaticCell;
use input::InputBuffer;
use crate::kernel::device;
use crate::kernel::driver;
use keyboard::{KeyEvent, KeyboardDriver};
use serial::{poll_input_event as poll_serial_input_event, SerialConsole};
use core::sync::atomic::{AtomicBool, Ordering};
use efi_main::graphics::FramebufferInfo;
use heapless::String;
use static_assertions::const_assert;
use unicode_width::UnicodeWidthChar;

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
}

impl OutputCapture {
    const fn new() -> Self {
        Self {
            active: false,
            suppress_console: false,
            buffer: AllocString::new(),
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

    fn put_char(&mut self, c: char) {
        if capture_char(c) {
            if should_suppress_output() {
                return;
            }
        }

        match c {
            '\n' => self.newline(),
            '\r' => {
                self.cursor.x = 0;
                self.backend.set_cursor(self.cursor.x, self.cursor.y);
            }
            '\t' => {
                let spaces = TAB_WIDTH - (self.cursor.x % TAB_WIDTH);
                for _ in 0..spaces {
                    self.put_char(' ');
                }
            }
            '\x08' => {
                if self.cursor.x > 0 {
                    self.cursor.x -= 1;
                    self.buffer[self.cursor.y][self.cursor.x] = ' ';
                    self.backend.set_cursor(self.cursor.x, self.cursor.y);
                    self.backend.put_char(' ');
                    self.backend.set_cursor(self.cursor.x, self.cursor.y);
                }
            }
            ch => {
                self.buffer[self.cursor.y][self.cursor.x] = ch;
                self.backend.put_char(ch);
                self.cursor.x += 1;
                if self.cursor.x >= self.cursor.width {
                    self.newline();
                }
            }
        }
    }

    fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c);
        }
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
        self.backend.set_cursor(self.cursor.x, self.cursor.y);
    }

    fn move_cursor_left(&mut self) {
        if self.cursor.x > 0 {
            self.cursor.x -= 1;
            self.backend.set_cursor(self.cursor.x, self.cursor.y);
        }
    }

    fn move_cursor_right(&mut self) {
        if self.cursor.x + 1 < self.cursor.width {
            self.cursor.x += 1;
            self.backend.set_cursor(self.cursor.x, self.cursor.y);
        }
    }

    fn newline(&mut self) {
        self.cursor.x = 0;
        self.cursor.y += 1;

        if self.cursor.y >= self.cursor.height {
            self.scroll();
            self.cursor.y = self.cursor.height - 1;
        }

        self.backend.set_cursor(self.cursor.x, self.cursor.y);
    }

    fn scroll(&mut self) {
        for y in 1..self.cursor.height {
            for x in 0..self.cursor.width {
                self.buffer[y - 1][x] = self.buffer[y][x];
            }
        }

        let last = self.cursor.height - 1;
        for x in 0..self.cursor.width {
            self.buffer[last][x] = ' ';
        }

        if self.backend.scroll_up(1) {
            self.backend.set_cursor(0, last);
            for x in 0..self.cursor.width {
                self.backend.put_char(self.buffer[last][x]);
            }
            self.backend.set_cursor(self.cursor.x, self.cursor.y);
        } else {
            self.redraw();
        }
    }

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

type DefaultBackend = MirrorConsole<SerialConsole, FramebufferConsole>;

static CONSOLE: StaticCell<Console<DefaultBackend>> =
    StaticCell::new(Console::new(
        MirrorConsole::new(SerialConsole::new(), FramebufferConsole::new()),
        DEFAULT_WIDTH,
        DEFAULT_HEIGHT,
    ));

static CONSOLE_INITIALIZED: AtomicBool = AtomicBool::new(false);
static CONSOLE_LOCKED: AtomicBool = AtomicBool::new(false);
static CAPTURE_LOCKED: AtomicBool = AtomicBool::new(false);
static INPUT_BUFFER: StaticCell<InputBuffer> = StaticCell::new(InputBuffer::new());
static KEYBOARD: StaticCell<KeyboardDriver> = StaticCell::new(KeyboardDriver::new());
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

pub fn init() {
    SerialConsole::init();
    let _ = driver::ensure_driver("serial", "0.1.0", "SAIOS", &[], driver::DriverStatus::Running);
    let _ = driver::ensure_driver("input", "0.1.0", "SAIOS", &["serial"], driver::DriverStatus::Running);
    let _ = device::ensure_device("COM1", "serial", "uart", device::DeviceStatus::Online);
    let _ = device::ensure_device("keyboard0", "input", "keyboard", device::DeviceStatus::Online);
    with_console(|console| console.init());
    unsafe {
        (*INPUT_BUFFER.get()).clear();
    }
    CONSOLE_INITIALIZED.store(true, Ordering::Release);
}

pub(crate) fn attach_framebuffer(info: FramebufferInfo) {
    with_console(|console| {
        console.backend.right_mut().attach(info);
        let _ = driver::ensure_driver("framebuffer", "0.1.0", "SAIOS", &["serial"], driver::DriverStatus::Running);
        let _ = device::ensure_device("fb0", "framebuffer", "display", device::DeviceStatus::Online);
        if let (Some(columns), Some(rows)) = (
            console.backend.right_mut().text_columns(),
            console.backend.right_mut().text_rows(),
        ) {
            console.resize(columns, rows);
        }
    });
}

pub fn promote_framebuffer_renderer() -> bool {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return false;
    }

    try_with_console(|console| console.backend.right_mut().ensure_renderer_ready()).unwrap_or(false)
}

pub fn put_char(c: char) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        SerialConsole::emergency_put_char(c);
        return;
    }

    if try_with_console(|console| console.put_char(c)).is_none() {
        SerialConsole::emergency_put_char(c);
    }
}

pub fn write_str(s: &str) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        emergency_write_str(s);
        return;
    }

    if try_with_console(|console| console.write_str(s)).is_none() {
        emergency_write_str(s);
    }
}

pub fn clear() {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.clear());
}

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

pub fn begin_output_capture(suppress_console: bool) {
    capture_lock();
    // SAFETY: guarded by capture lock.
    unsafe {
        let capture = &mut *OUTPUT_CAPTURE.get();
        capture.active = true;
        capture.suppress_console = suppress_console;
        capture.buffer.clear();
    }
    capture_unlock();
}

pub fn end_output_capture() -> AllocString {
    capture_lock();
    // SAFETY: guarded by capture lock.
    let out = unsafe {
        let capture = &mut *OUTPUT_CAPTURE.get();
        capture.active = false;
        capture.suppress_console = false;
        core::mem::take(&mut capture.buffer)
    };
    capture_unlock();
    out
}

pub fn set_cursor(x: usize, y: usize) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.set_cursor(x, y));
}

pub fn move_cursor_left() {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.move_cursor_left());
}

pub fn move_cursor_right() {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.move_cursor_right());
}

pub fn newline() {
    put_char('\n');
}

pub fn print(s: &str) {
    write_str(s);
}

pub fn print_fmt(args: fmt::Arguments) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        let _ = hal::arch::x86_64::console::_print(args);
        return;
    }

    if try_with_console(|console| {
        let _ = console.write_fmt(args);
    })
    .is_none()
    {
        let _ = hal::arch::x86_64::console::_print(args);
    }
}

pub fn panic_write_str(s: &str) {
    emergency_write_str(s);
}

pub fn panic_println(s: &str) {
    panic_write_str(s);
    panic_write_str("\n");
}

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

    fn redraw_line(prev_len_cells: usize, prev_cursor_cells: usize) {
        // SAFETY: single-core early kernel context.
        let input = unsafe { &mut *INPUT_BUFFER.get() };

        for _ in prev_cursor_cells..prev_len_cells {
            move_cursor_right();
        }

        for _ in 0..prev_len_cells {
            put_char('\x08');
        }

        let rendered = input.render();
        for ch in rendered.chars() {
            put_char(ch);
        }

        let new_len_cells = line_cells(rendered.as_str());
        let new_cursor_cells = cursor_cells(rendered.as_str(), input.cursor());
        for _ in new_cursor_cells..new_len_cells {
            move_cursor_left();
        }
    }

    // SAFETY: single-core early kernel context.
    let key_event = unsafe { (*KEYBOARD.get()).poll_event() }.or_else(poll_serial_input_event)?;

    // SAFETY: single-core early kernel context.
    let (prev_len_cells, prev_cursor_cells, prev_cursor_char_width, prev_right_char_width) = unsafe {
        let input = &*INPUT_BUFFER.get();
        let rendered = input.render();
        let len_cells = line_cells(rendered.as_str());
        let cursor_cells = cursor_cells(rendered.as_str(), input.cursor());
        (
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
                    redraw_line(prev_len_cells, prev_cursor_cells);
                }
            }
            None
        }
        KeyEvent::Backspace => {
            let erased = unsafe { (*INPUT_BUFFER.get()).backspace() };
            if erased {
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::Delete => {
            let erased = unsafe { (*INPUT_BUFFER.get()).delete() };
            if erased {
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::Insert => None,
        KeyEvent::Home => {
            if unsafe { (*INPUT_BUFFER.get()).move_home() } {
                for _ in 0..prev_cursor_cells {
                    move_cursor_left();
                }
            }
            None
        }
        KeyEvent::End => {
            if unsafe { (*INPUT_BUFFER.get()).move_end() } {
                for _ in prev_cursor_cells..prev_len_cells {
                    move_cursor_right();
                }
            }
            None
        }
        KeyEvent::PageUp => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_prev() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::PageDown => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_next() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::ArrowLeft => {
            if unsafe { (*INPUT_BUFFER.get()).move_left() } {
                for _ in 0..prev_cursor_char_width {
                    move_cursor_left();
                }
            }
            None
        }
        KeyEvent::ArrowRight => {
            if unsafe { (*INPUT_BUFFER.get()).move_right() } {
                for _ in 0..prev_right_char_width {
                    move_cursor_right();
                }
            }
            None
        }
        KeyEvent::ShiftArrowLeft => {
            if unsafe { (*INPUT_BUFFER.get()).move_left() } {
                for _ in 0..prev_cursor_char_width {
                    move_cursor_left();
                }
            }
            None
        }
        KeyEvent::ShiftArrowRight => {
            if unsafe { (*INPUT_BUFFER.get()).move_right() } {
                for _ in 0..prev_right_char_width {
                    move_cursor_right();
                }
            }
            None
        }
        KeyEvent::ShiftArrowUp => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_prev() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::ShiftArrowDown => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_next() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlArrowLeft | KeyEvent::CtrlShiftArrowLeft => {
            if unsafe { (*INPUT_BUFFER.get()).move_prev_word() } {
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlArrowRight | KeyEvent::CtrlShiftArrowRight => {
            if unsafe { (*INPUT_BUFFER.get()).move_next_word() } {
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlArrowUp | KeyEvent::CtrlShiftArrowUp => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_prev() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlArrowDown | KeyEvent::CtrlShiftArrowDown => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_next() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_len_cells, prev_cursor_cells);
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
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::ArrowDown => {
            let line = unsafe { (*INPUT_BUFFER.get()).history_next() };
            if let Some(line) = line {
                unsafe { (*INPUT_BUFFER.get()).set_line(line.as_str()) };
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlC => {
            newline();
            unsafe { (*INPUT_BUFFER.get()).clear() };
            Some(String::new())
        }
        KeyEvent::CtrlA => {
            if unsafe { (*INPUT_BUFFER.get()).move_home() } {
                for _ in 0..prev_cursor_cells {
                    move_cursor_left();
                }
            }
            None
        }
        KeyEvent::CtrlE => {
            if unsafe { (*INPUT_BUFFER.get()).move_end() } {
                for _ in prev_cursor_cells..prev_len_cells {
                    move_cursor_right();
                }
            }
            None
        }
        KeyEvent::CtrlD => {
            let erased = unsafe { (*INPUT_BUFFER.get()).delete() };
            if erased {
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlU => {
            let changed = unsafe { (*INPUT_BUFFER.get()).clear_to_start() };
            if changed {
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::CtrlK => {
            let changed = unsafe { (*INPUT_BUFFER.get()).clear_to_end() };
            if changed {
                redraw_line(prev_len_cells, prev_cursor_cells);
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
            for ch in rendered.chars() {
                put_char(ch);
            }

            let rendered_cells = line_cells(rendered.as_str());
            let cursor_cells = cursor_cells(rendered.as_str(), input.cursor());
            for _ in cursor_cells..rendered_cells {
                move_cursor_left();
            }

            None
        }
        KeyEvent::CtrlW => {
            let changed = unsafe { (*INPUT_BUFFER.get()).delete_prev_word() };
            if changed {
                redraw_line(prev_len_cells, prev_cursor_cells);
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
                redraw_line(prev_len_cells, prev_cursor_cells);
            }
            None
        }
        KeyEvent::Escape => None,
    }
}

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
