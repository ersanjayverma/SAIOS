use core::sync::atomic::{AtomicBool, Ordering};

static LOGGER_READY: AtomicBool = AtomicBool::new(false);

pub fn init_serial() {
    crate::console::init_serial();
    crate::console::write_str("[SEED] serial init\n");
}

pub fn init_logger() {
    crate::log::logger::init();
    LOGGER_READY.store(true, Ordering::Relaxed);
    crate::console::write_str("[SEED] logger init\n");
    crate::log::logger::log_str(
        crate::log::level::LogLevel::Info,
        "Diagnostics pipeline online",
    );
}

pub fn stage(name: &'static str) {
    crate::console::write_str("[SEED] stage ");
    crate::console::write_str(name);
    crate::console::write_str("\n");

    if LOGGER_READY.load(Ordering::Relaxed) {
        crate::log::logger::log_str(crate::log::level::LogLevel::Info, name);
    }
}

pub fn stage_ok(name: &'static str) {
    crate::console::write_str("[SEED] ");
    crate::console::write_str(name);
    crate::console::write_str(" OK\n");

    if LOGGER_READY.load(Ordering::Relaxed) {
        crate::log::logger::log_str(crate::log::level::LogLevel::Info, name);
    }
}

pub fn exception_trap(vector: u32) {
    crate::console::write_fmt(format_args!("[SEED] exception trap vector={}\n", vector));
}
