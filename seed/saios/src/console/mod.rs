use core::cell::UnsafeCell;
use core::fmt;

use efi_main::graphics::FramebufferInfo;

mod ring;
pub mod sinks;

use ring::LogRing;
use sinks::framebuffer::FramebufferSink;
use sinks::serial::SerialSink;

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

struct ConsoleState {
    serial: SerialSink,
    framebuffer: Option<FramebufferSink>,
    ring: LogRing,
}

impl ConsoleState {
    const fn new() -> Self {
        Self {
            serial: SerialSink::new(),
            framebuffer: None,
            ring: LogRing::new(),
        }
    }

    fn write_str(&mut self, s: &str) {
        self.ring.append(s);
        self.serial.write_str(s);
        if let Some(fb) = self.framebuffer.as_mut() {
            fb.write_str(s);
        }
    }
}

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

pub fn init_serial() {
    state().serial.init();
}

pub fn attach_framebuffer(fb: FramebufferInfo) {
    state().framebuffer = Some(FramebufferSink::new(fb));
}

pub fn write_str(s: &str) {
    state().write_str(s);
}

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

pub fn replay_ring_to_serial() {
    let s = state();
    s.ring.replay(|b| s.serial.write_byte(b));
}

pub fn replay_ring<F: FnMut(u8)>(emit: F) {
    state().ring.replay(emit);
}
