use core::sync::atomic::{AtomicBool, Ordering};

static SERIAL_READY: AtomicBool = AtomicBool::new(false);
static LOGGER_READY: AtomicBool = AtomicBool::new(false);

pub fn init_serial() {
    if SERIAL_READY.swap(true, Ordering::Relaxed) {
        return;
    }
    crate::console::init_serial();
    crate::console::write_debug_str("[SEED] serial ready\n");
}

pub fn init_logger() {
    if LOGGER_READY.swap(true, Ordering::Relaxed) {
        return;
    }
    crate::log::logger::init();
    crate::console::write_debug_str("[SEED] logger ready\n");
    crate::log::info!("Diagnostics pipeline online");
}

pub fn stage(name: &'static str) {
    crate::console::write_debug_str("[SEED] stage: ");
    crate::console::write_debug_str(name);
    crate::console::write_debug_str("\n");

    if LOGGER_READY.load(Ordering::Relaxed) {
        crate::log::logger::log_str(crate::log::level::LogLevel::Info, "stage: ");
        crate::log::logger::log_str(crate::log::level::LogLevel::Info, name);
    }
}

pub fn stage_ok(name: &'static str) {
    crate::console::write_debug_str("[SEED] ");
    crate::console::write_debug_str(name);
    crate::console::write_debug_str(" OK\n");

    if LOGGER_READY.load(Ordering::Relaxed) {
        crate::log::logger::log_str(crate::log::level::LogLevel::Info, name);
        crate::log::logger::log_str(crate::log::level::LogLevel::Info, " OK");
    }
}

pub fn exception_trap(vector: u32) {
    crate::console::write_debug_fmt(format_args!("[SEED] exception trap vector={}\n", vector));
    if LOGGER_READY.load(Ordering::Relaxed) {
        crate::log::error!("Exception trap: vector {}", vector);
    }
}

pub fn serial_ready() -> bool {
    SERIAL_READY.load(Ordering::Relaxed)
}

pub fn logger_ready() -> bool {
    LOGGER_READY.load(Ordering::Relaxed)
}
