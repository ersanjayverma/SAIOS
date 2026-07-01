mod backend;
mod cursor;
mod framebuffer;
mod font;
mod glyph;
mod input;
mod keyboard;
mod serial;

use core::fmt::{self, Write};

use backend::{ConsoleBackend, MirrorConsole};
use cursor::Cursor;
use framebuffer::FramebufferConsole;
use hal::arch::x86_64::sync::StaticCell;
use input::InputBuffer;
use keyboard::{KeyEvent, KeyboardDriver};
use serial::SerialConsole;
use core::sync::atomic::{AtomicBool, Ordering};
use efi_main::graphics::FramebufferInfo;
use heapless::String;

const DEFAULT_WIDTH: usize = 80;
const DEFAULT_HEIGHT: usize = 25;
const TAB_WIDTH: usize = 4;
const MAX_WIDTH: usize = 160;
const MAX_HEIGHT: usize = 100;

struct Console<B: ConsoleBackend> {
    backend: B,
    cursor: Cursor,
    buffer: [[char; MAX_WIDTH]; MAX_HEIGHT],
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

    fn newline(&mut self) {
        self.cursor.x = 0;
        self.cursor.y += 1;
        self.backend.put_char('\n');

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

        self.redraw();
    }

    fn redraw(&mut self) {
        self.backend.clear();
        self.backend.set_cursor(0, 0);

        for y in 0..self.cursor.height {
            for x in 0..self.cursor.width {
                self.backend.put_char(self.buffer[y][x]);
            }
            if y + 1 < self.cursor.height {
                self.backend.put_char('\n');
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
static INPUT_BUFFER: StaticCell<InputBuffer> = StaticCell::new(InputBuffer::new());
static KEYBOARD: StaticCell<KeyboardDriver> = StaticCell::new(KeyboardDriver::new());

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
    with_console(|console| console.init());
    unsafe {
        (*INPUT_BUFFER.get()).clear();
    }
    CONSOLE_INITIALIZED.store(true, Ordering::Release);
}

pub(crate) fn attach_framebuffer(info: FramebufferInfo) {
    with_console(|console| {
        console.backend.right_mut().attach(info);
        if let (Some(columns), Some(rows)) = (
            console.backend.right_mut().text_columns(),
            console.backend.right_mut().text_rows(),
        ) {
            console.resize(columns, rows);
        }
    });
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

pub fn set_cursor(x: usize, y: usize) {
    if !CONSOLE_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let _ = try_with_console(|console| console.set_cursor(x, y));
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

pub fn prompt() {
    print("> ");
}

pub fn poll_input() -> Option<String<256>> {
    // SAFETY: single-core early kernel context.
    let key_event = unsafe { (*KEYBOARD.get()).poll_event() }?;

    match key_event {
        KeyEvent::Character(ch) => {
            unsafe { (*INPUT_BUFFER.get()).push(ch) };
            put_char(ch);
            None
        }
        KeyEvent::Backspace => {
            let erased = unsafe { (*INPUT_BUFFER.get()).backspace() };
            if erased {
                put_char('\x08');
            }
            None
        }
        KeyEvent::Enter => {
            newline();
            let line = unsafe { (*INPUT_BUFFER.get()).take() };
            unsafe { (*INPUT_BUFFER.get()).clear() };
            prompt();
            Some(line)
        }
        KeyEvent::Tab => {
            put_char('\t');
            None
        }
        KeyEvent::Escape => None,
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
