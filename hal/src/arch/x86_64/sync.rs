use core::cell::UnsafeCell;

pub struct StaticCell<T>(UnsafeCell<T>);

unsafe impl<T> Sync for StaticCell<T> {}

impl<T> StaticCell<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    pub fn get(&self) -> *mut T {
        self.0.get()
    }
}