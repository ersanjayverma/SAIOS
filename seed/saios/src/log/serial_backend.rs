use core::sync::atomic::{AtomicBool, Ordering};

static BACKEND_READY: AtomicBool = AtomicBool::new(false);

pub fn init() {
    if BACKEND_READY.swap(true, Ordering::Relaxed) {
        return;
    }
    crate::console::init_serial();
}

/// Write a string through the console subsystem.
///
/// Uses `write_str` (not `write_debug_str`) so log output reaches
/// the framebuffer in addition to serial and the ring buffer.
pub fn write_str(s: &str) {
    crate::console::write_str(s);
}

pub fn write_fmt(args: core::fmt::Arguments<'_>) {
    crate::console::write_fmt(args);
}

pub fn ready() -> bool {
    BACKEND_READY.load(Ordering::Relaxed)
}
