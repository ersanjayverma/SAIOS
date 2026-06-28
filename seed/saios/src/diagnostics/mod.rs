use core::sync::atomic::{AtomicBool, Ordering};

static LOGGER_READY: AtomicBool = AtomicBool::new(false);

pub fn init_serial() {
    crate::drivers::serial::init();
    crate::drivers::serial::write_str("[SEED] D0 serial init\n");
}

pub fn init_logger() {
    crate::log::logger::init();
    LOGGER_READY.store(true, Ordering::Relaxed);
    crate::drivers::serial::write_str("[SEED] D1 logger init\n");
    crate::log::logger::log_str(crate::log::level::LogLevel::Info, "Diagnostics pipeline online");
}

pub fn stage(name: &'static str) {
    crate::drivers::serial::write_str("[SEED] stage ");
    crate::drivers::serial::write_str(name);
    crate::drivers::serial::write_str("\n");

    if LOGGER_READY.load(Ordering::Relaxed) {
        crate::log::logger::log_str(crate::log::level::LogLevel::Info, name);
    }
}

pub fn exception_trap(vector: u32) {
    crate::drivers::serial::write_fmt(format_args!("[SEED] exception trap vector={}\n", vector));
}
