use core::fmt::Arguments;
use core::sync::atomic::{AtomicU8, Ordering};

use crate::log::level::LogLevel;
use crate::log::serial_backend;

static MIN_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

pub fn init() {
    serial_backend::init();
    #[cfg(debug_assertions)]
    set_min_level(LogLevel::Debug);
    #[cfg(not(debug_assertions))]
    set_min_level(LogLevel::Info);
}

pub fn set_min_level(level: LogLevel) {
    MIN_LEVEL.store(level as u8, Ordering::Relaxed);
}

#[inline]
pub fn enabled(level: LogLevel) -> bool {
    (level as u8) >= MIN_LEVEL.load(Ordering::Relaxed)
}

pub fn log(level: LogLevel, args: Arguments<'_>) {
    if !enabled(level) {
        return;
    }

    serial_backend::write_str("[");
    serial_backend::write_str(level.tag());
    serial_backend::write_str("] ");

    serial_backend::write_fmt(args);

    serial_backend::write_str("\n");
}

pub fn log_str(level: LogLevel, msg: &str) {
    if !enabled(level) {
        return;
    }

    serial_backend::write_str("[");
    serial_backend::write_str(level.tag());
    serial_backend::write_str("] ");
    serial_backend::write_str(msg);
    serial_backend::write_str("\n");
}
