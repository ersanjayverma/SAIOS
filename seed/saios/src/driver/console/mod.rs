/// Re-export the HAL's console module (which provides println support).
pub use hal::arch::x86_64::console::*;

/// One-time console initialisation.  Must be called before the first
/// `println!`.
pub fn init() {
    hal::arch::x86_64::console::init_serial();
}
