#[macro_use]
pub mod console;

/// Re-export the HAL's console module (which provides println support).
pub use hal::arch::x86_64::console as hal_console;
pub use hal::arch::x86_64::console::{_print, init_serial, COM1, COM2, COM3, COM4};
