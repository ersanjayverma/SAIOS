pub struct InterruptDescriptorTable {}
pub fn init() {}
pub fn enable() {}

pub fn disable() {}

pub fn are_enabled() {}

pub fn without_interrupts<F>(f: F)
where
    F: FnOnce(),
{
}
pub fn int3() {}
