#[macro_use]
pub mod console;
pub mod memory;
/// Re-export the HAL's console module (which provides println support).
pub use hal::arch::x86_64::console as hal_console;
pub use hal::arch::x86_64::console::{_print, COM1, COM2, COM3, COM4, init_serial};
