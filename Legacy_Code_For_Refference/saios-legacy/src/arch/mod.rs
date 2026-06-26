//! Phase 1 architecture-neutral forwarding wrappers.
//!
//! This module intentionally stays thin for now: it exposes a small stable API
//! while forwarding to the existing x86_64 implementation.

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("SAIOS Phase 1 arch wrappers currently support only x86_64");

pub mod cpu {
    pub use super::x86_64::cpu::*;
    pub use super::x86_64::cpu::{gdt, tables};
}

pub mod interrupt {
    pub use super::x86_64::interrupt::*;
}

pub mod memory {
    pub use super::x86_64::memory::paging;
    pub use super::x86_64::memory::*;
}

pub mod process {
    pub use super::x86_64::process::*;
}

pub mod smp {
    pub use super::x86_64::smp::*;
}

pub mod syscall {
    pub use super::x86_64::syscall::*;
}

#[inline]
pub fn halt() {
    x86_64::halt()
}

#[inline]
pub fn nop() {
    x86_64::nop()
}

#[inline]
pub fn read_tsc() -> u64 {
    x86_64::read_tsc()
}

#[inline]
pub fn enable_interrupts() {
    x86_64::enable_interrupts()
}

#[inline]
pub fn disable_interrupts() {
    x86_64::disable_interrupts()
}

#[inline]
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    x86_64::without_interrupts(f)
}

#[inline]
pub fn current_page_table() -> u64 {
    x86_64::current_page_table()
}

#[inline]
pub fn fault_address() -> u64 {
    x86_64::fault_address()
}

#[inline]
pub fn flush_tlb(addr: u64) {
    x86_64::flush_tlb(addr)
}

#[inline]
pub unsafe fn write_msr(msr: u32, value: u64) {
    unsafe { x86_64::write_msr(msr, value) }
}

#[inline]
pub unsafe fn port_read_u8(port: u16) -> u8 {
    unsafe { x86_64::port_read_u8(port) }
}

#[inline]
pub unsafe fn port_write_u8(port: u16, value: u8) {
    unsafe { x86_64::port_write_u8(port, value) }
}

#[inline]
pub unsafe fn port_read_u16(port: u16) -> u16 {
    unsafe { x86_64::port_read_u16(port) }
}

#[inline]
pub unsafe fn port_write_u16(port: u16, value: u16) {
    unsafe { x86_64::port_write_u16(port, value) }
}

#[inline]
pub unsafe fn port_read_u32(port: u16) -> u32 {
    unsafe { x86_64::port_read_u32(port) }
}

#[inline]
pub unsafe fn port_write_u32(port: u16, value: u32) {
    unsafe { x86_64::port_write_u32(port, value) }
}

#[inline]
pub unsafe fn prepare_pit_channel2_oneshot(count: u16) {
    unsafe { x86_64::prepare_pit_channel2_oneshot(count) }
}

#[inline]
pub unsafe fn pit_channel2_terminal_count() -> bool {
    unsafe { x86_64::pit_channel2_terminal_count() }
}

#[inline]
pub unsafe fn read_cmos_register(reg: u8) -> u8 {
    unsafe { x86_64::read_cmos_register(reg) }
}
