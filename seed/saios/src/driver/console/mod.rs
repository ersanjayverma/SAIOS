#[macro_use]
pub mod macros;
mod serial;
mod sink;

pub use serial::SerialConsole;
pub use sink::ConsoleSink;

/// Re-export the singleton-backed `_print` so macros resolve to it.
pub use crate::driver::serial::_print;

/// One-time console initialisation.  Must be called before the first
/// `println!`.
pub fn init() {
    crate::driver::serial::init_serial();
}
