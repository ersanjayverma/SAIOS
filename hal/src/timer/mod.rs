pub mod hpet;
pub mod lapic;
pub mod pit;
pub mod tsc;
pub mod uefi;

pub trait ClockSource:HalDevice {
    fn counter(&self) -> u64;
    fn init(&mut self);
    fn frequency_hz(&self) -> u64;
}
pub trait HalDevice {
    fn name(&self) -> &'static str;
}
pub trait ClockEvent:HalDevice {
    fn enable(&mut self);

    fn disable(&mut self);

    fn set_periodic(&mut self, hz: u64);

    fn set_oneshot(&mut self, ns: u64);
}
