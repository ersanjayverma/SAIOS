#[macro_use]
pub mod macros;
mod serial;
mod sink;

pub use serial::{_print, SerialConsole};
pub use sink::ConsoleSink;
