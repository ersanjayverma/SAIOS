use core::cell::UnsafeCell;
use core::ptr;

pub struct Volatile<T: Copy> {
    value: UnsafeCell<T>,
}

impl<T: Copy> Volatile<T> {
    pub const fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
        }
    }

    pub fn read(&self) -> T {
        unsafe { ptr::read_volatile(self.value.get()) }
    }

    pub fn write(&self, value: T) {
        unsafe {
            ptr::write_volatile(self.value.get(), value);
        }
    }
}
