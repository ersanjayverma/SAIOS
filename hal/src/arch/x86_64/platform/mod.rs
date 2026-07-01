pub mod cpu;
pub mod interrupt;
pub mod memory;
pub mod timer;

pub fn init() {
    cpu::init();
    interrupt::init();
    memory::init();
    timer::init();
}
