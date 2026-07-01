pub struct Mmio<T: Copy> {
    address: *mut T,
}
impl<T: Copy> Mmio<T> {
    pub const fn new(address: *mut T) -> Self {
        Self { address }
    }

    pub fn read(&self) -> T {
        unsafe { core::ptr::read_volatile(self.address) }
    }

    pub fn write(&self, value: T) {
        unsafe {
            core::ptr::write_volatile(self.address, value);
        }
    }
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(T) -> T,
    {
        let value = self.read();
        let new_value = f(value);
        self.write(new_value);
    }
}
