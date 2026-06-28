pub mod hpet;
pub mod lapic;
pub mod pit;
pub mod tsc;
pub mod uefi;

pub trait TimerHal {
    fn name(&self) -> &'static str;
    fn frequency_hz(&self) -> u64;
    fn counter(&self) -> u64;
    fn set_periodic(&mut self, hz: u64);
    fn set_oneshot(&mut self, ns: u64);
    fn enable(&mut self);
    fn disable(&mut self);
    fn acknowledge(&mut self);
}
