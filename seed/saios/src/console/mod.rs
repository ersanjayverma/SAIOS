use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use efi_main::graphics::FramebufferInfo;

mod ring;
pub mod sinks;

use ring::LogRing;
use sinks::framebuffer::FramebufferSink;
use sinks::serial::SerialSink;

static SERIAL_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ── Writer trait ──────────────────────────────────────────────────────

pub trait ConsoleWriter {
    fn write_str(&mut self, s: &str);
}

impl ConsoleWriter for SerialSink {
    fn write_str(&mut self, s: &str) {
        SerialSink::write_str(self, s);
    }
}

impl ConsoleWriter for FramebufferSink {
    fn write_str(&mut self, s: &str) {
        FramebufferSink::write_str(self, s);
    }
}

// ── Reader trait ──────────────────────────────────────────────────────

/// Trait for console input sources.
pub trait ConsoleReader {
    fn is_data_ready(&self) -> bool;
    fn read_byte(&mut self) -> Option<u8>;
    fn try_read_byte(&mut self) -> Option<u8>;
    fn read_bytes(&mut self, buf: &mut [u8]) -> usize;
}

impl ConsoleReader for SerialSink {
    fn is_data_ready(&self) -> bool {
        SerialSink::is_data_ready(self)
    }
    fn read_byte(&mut self) -> Option<u8> {
        SerialSink::read_byte(self)
    }
    fn try_read_byte(&mut self) -> Option<u8> {
        SerialSink::try_read_byte(self)
    }
    fn read_bytes(&mut self, buf: &mut [u8]) -> usize {
        SerialSink::read_bytes(self, buf)
    }
}

// ── Line buffer ───────────────────────────────────────────────────────

const LINE_BUF_SIZE: usize = 256;

pub struct LineBuffer {
    buf: [u8; LINE_BUF_SIZE],
    len: usize,
}

impl LineBuffer {
    pub const fn new() -> Self {
        Self {
            buf: [0; LINE_BUF_SIZE],
            len: 0,
        }
    }

    pub fn reset(&mut self) {
        self.len = 0;
    }

    /// Push a byte. Returns `true` if the line is now complete (newline).
    pub fn push(&mut self, byte: u8) -> bool {
        match byte {
            b'\n' | b'\r' => true,
            b'\x08' | b'\x7F' => {
                if self.len > 0 {
                    self.len -= 1;
                }
                false
            }
            _ => {
                if self.len < LINE_BUF_SIZE {
                    self.buf[self.len] = byte;
                    self.len += 1;
                }
                false
            }
        }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// ── Console state ─────────────────────────────────────────────────────

struct ConsoleState {
    serial: SerialSink,
    framebuffer: Option<FramebufferSink>,
    ring: LogRing,
    line_buf: LineBuffer,
    line_complete: bool,
    echo: bool,
}

impl ConsoleState {
    const fn new() -> Self {
        Self {
            serial: SerialSink::new(),
            framebuffer: None,
            ring: LogRing::new(),
            line_buf: LineBuffer::new(),
            line_complete: false,
            echo: true,
        }
    }

    fn write_str(&mut self, s: &str) {
        self.ring.append(s);
        self.serial.write_str(s);
        if let Some(fb) = self.framebuffer.as_mut() {
            fb.write_str(s);
        }
    }

    fn write_debug_str(&mut self, s: &str) {
        self.ring.append(s);
        self.serial.write_str(s);
    }

    fn write_byte(&mut self, b: u8) {
        // Append a UTF-8 representation to the ring
        let tmp = [b];
        if let Ok(s) = core::str::from_utf8(&tmp) {
            self.ring.append(s);
        }
        self.serial.write_byte(b);
        if let Some(fb) = self.framebuffer.as_mut() {
            if let Ok(s) = core::str::from_utf8(&tmp) {
                fb.write_str(s);
            }
        }
    }

    fn flush(&mut self) {
        self.serial.flush();
    }

    fn is_data_ready(&self) -> bool {
        self.serial.is_data_ready()
    }

    fn read_byte(&mut self) -> Option<u8> {
        self.serial.read_byte()
    }

    fn try_read_byte(&mut self) -> Option<u8> {
        self.serial.try_read_byte()
    }

    fn read_bytes(&mut self, buf: &mut [u8]) -> usize {
        self.serial.read_bytes(buf)
    }

    /// Process a single input byte for line editing.
    /// Returns `true` if a line was completed.
    fn process_input_byte(&mut self, byte: u8) -> bool {
        match byte {
            b'\n' | b'\r' => {
                if self.echo {
                    self.write_str("\n");
                }
                self.line_complete = true;
                true
            }
            b'\x08' | b'\x7F' => {
                if self.line_buf.len() > 0 {
                    self.line_buf.push(byte); // handles deletion
                    if self.echo {
                        self.write_str("\x08 \x08");
                    }
                }
                false
            }
            _ => {
                self.line_buf.push(byte);
                if self.echo {
                    self.write_byte(byte);
                }
                false
            }
        }
    }

    /// Poll for input. Returns `true` if a complete line is ready.
    fn poll_input(&mut self) -> bool {
        while let Some(byte) = self.serial.try_read_byte() {
            if self.process_input_byte(byte) {
                return true;
            }
        }
        false
    }

    fn set_echo(&mut self, echo: bool) {
        self.echo = echo;
    }
}

// ── Global singleton ──────────────────────────────────────────────────

struct GlobalCell<T>(UnsafeCell<T>);

impl<T> GlobalCell<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    fn get(&self) -> *mut T {
        self.0.get()
    }
}

unsafe impl<T> Sync for GlobalCell<T> {}

static CONSOLE: GlobalCell<ConsoleState> = GlobalCell::new(ConsoleState::new());

#[inline]
fn state() -> &'static mut ConsoleState {
    unsafe { &mut *CONSOLE.get() }
}

/// Obtain a shared reference to the console state.
///
/// # Safety
/// Must not be called while a mutable reference is active.
#[inline]
fn state_ref() -> &'static ConsoleState {
    unsafe { &*CONSOLE.get() }
}

// ── Public API ────────────────────────────────────────────────────────

/// Initialize the serial port and probe for hardware.
pub fn init_serial() {
    if SERIAL_INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }
    state().serial.init();
}

/// Attach a framebuffer for on-screen console output.
pub fn attach_framebuffer(fb: FramebufferInfo) {
    state().framebuffer = Some(FramebufferSink::new(fb));
}

// ── Write API ─────────────────────────────────────────────────────────

/// Write a string to all active console sinks (serial + framebuffer + ring).
pub fn write_str(s: &str) {
    state().write_str(s);
}

/// Write a debug string to serial and ring only (not framebuffer).
pub fn write_debug_str(s: &str) {
    state().write_debug_str(s);
}

/// Write a single byte to all active console sinks.
pub fn write_byte(b: u8) {
    state().write_byte(b);
}

/// Write formatted output to all active console sinks.
pub fn write_fmt(args: fmt::Arguments<'_>) {
    struct ConsoleFmt;
    impl fmt::Write for ConsoleFmt {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            crate::console::write_str(s);
            Ok(())
        }
    }
    use fmt::Write;
    let _ = ConsoleFmt.write_fmt(args);
}

/// Write formatted debug output to serial and ring only.
pub fn write_debug_fmt(args: fmt::Arguments<'_>) {
    struct DebugFmt;
    impl fmt::Write for DebugFmt {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            crate::console::write_debug_str(s);
            Ok(())
        }
    }
    use fmt::Write;
    let _ = DebugFmt.write_fmt(args);
}

/// Flush pending serial output.
pub fn flush() {
    state().flush();
}

// ── Read API ──────────────────────────────────────────────────────────

/// Check if a byte is available to read from console input.
#[inline]
pub fn is_data_ready() -> bool {
    state().is_data_ready()
}

/// Read a single byte from console input with timeout.
#[inline]
pub fn read_byte() -> Option<u8> {
    state().read_byte()
}

/// Try to read a byte without blocking.
#[inline]
pub fn try_read_byte() -> Option<u8> {
    state().try_read_byte()
}

/// Read available bytes into the provided buffer.
/// Returns the number of bytes actually read.
#[inline]
pub fn read_bytes(buf: &mut [u8]) -> usize {
    state().read_bytes(buf)
}

// ── Line editing API ──────────────────────────────────────────────────

/// Poll for console input with line editing and echo.
///
/// Call this regularly (e.g., in an event loop). When it returns
/// `true`, a complete line is available. Use [`line_str()`] or
/// [`copy_line()`] to retrieve it, then call [`reset_line()`] to
/// prepare for the next line.
///
/// Handles backspace (0x08 / 0x7F) and echoes input to all active
/// console sinks.
pub fn poll_line() -> bool {
    state().poll_input()
}

/// Check whether a complete line has been received.
#[inline]
pub fn line_ready() -> bool {
    state_ref().line_complete
}

/// Get the current line buffer contents as a `&str`.
///
/// The returned reference is valid until the next call to
/// [`reset_line()`] or [`poll_line()`].
pub fn line_str() -> &'static str {
    let s = state_ref();
    s.line_buf.as_str()
}

/// Get the current line buffer contents as bytes.
pub fn line_bytes() -> &'static [u8] {
    let s = state_ref();
    s.line_buf.as_bytes()
}

/// Copy the current line buffer into the provided slice.
/// Returns the number of bytes copied.
pub fn copy_line_into(buf: &mut [u8]) -> usize {
    let s = state_ref();
    let src = s.line_buf.as_bytes();
    let len = src.len().min(buf.len());
    buf[..len].copy_from_slice(&src[..len]);
    len
}

/// Reset the line buffer for the next line of input.
pub fn reset_line() {
    let s = state();
    s.line_buf.reset();
    s.line_complete = false;
}

/// Enable or disable input echo.
pub fn set_echo(echo: bool) {
    state().set_echo(echo);
}

// ── Status API ────────────────────────────────────────────────────────

/// Check if the serial port is present and functional.
pub fn serial_present() -> bool {
    state_ref().serial.is_present()
}

/// Check if the serial port is ready to accept a byte for transmission.
pub fn serial_ready() -> bool {
    state_ref().serial.is_ready()
}

// ── Panic / crash path ────────────────────────────────────────────────

/// Print panic information to all console sinks.
///
/// Called by the panic handler before triggering RRoD. This ensures
/// the panic message appears on all console sinks and in the ring
/// buffer before the system halts.
pub fn panic_prelude(info: &core::panic::PanicInfo<'_>) {
    write_str("\n=== KERNEL PANIC ===\n");

    if let Some(location) = info.location() {
        write_fmt(format_args!(
            "location: {}:{}\n",
            location.file(),
            location.line()
        ));
    } else {
        write_str("location: <unknown>\n");
    }

    if let Some(message) = info.message().as_str() {
        write_str("message: ");
        write_str(message);
        write_str("\n");
    } else {
        write_str("message: <formatted panic>\n");
    }

    write_str("--- recent log ring ---\n");
    replay_ring_to_serial();
    write_str("\n--- end log ring ---\n");
}

/// Replay the log ring to the serial port.
pub fn replay_ring_to_serial() {
    let s = state();
    s.ring.replay(|b| s.serial.write_byte(b));
}

/// Replay the log ring through a caller-provided callback.
pub fn replay_ring<F: FnMut(u8)>(emit: F) {
    state().ring.replay(emit);
}
