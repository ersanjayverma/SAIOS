//! x86_64 implementation for the architecture wrapper surface and moved core subsystems.

pub mod cpu;
pub mod interrupt;
pub mod ioapic;
pub mod memory;
pub mod process;
pub mod smp;
pub mod syscall;

use ::x86_64::instructions::port::Port;
use ::x86_64::registers::control::Cr2;
use ::x86_64::registers::model_specific::Msr;

const PIT_CONTROL_PORT: u16 = 0x43;
const PIT_CHANNEL2_PORT: u16 = 0x42;
const PIT_SPEAKER_GATE_PORT: u16 = 0x61;
const CMOS_ADDRESS_PORT: u16 = 0x70;
const CMOS_DATA_PORT: u16 = 0x71;

#[inline]
pub fn halt() {
    ::x86_64::instructions::hlt();
}

#[inline]
pub fn nop() {
    ::x86_64::instructions::nop();
}

#[inline]
pub fn read_tsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[inline]
pub fn enable_interrupts() {
    ::x86_64::instructions::interrupts::enable();
}

#[inline]
pub fn disable_interrupts() {
    ::x86_64::instructions::interrupts::disable();
}

#[inline]
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    ::x86_64::instructions::interrupts::without_interrupts(f)
}

#[inline]
pub fn current_page_table() -> u64 {
    memory::paging::active_pml4()
}

#[inline]
pub fn fault_address() -> u64 {
    Cr2::read().as_u64()
}

#[inline]
pub fn flush_tlb(addr: u64) {
    memory::paging::flush_tlb(addr)
}

#[inline]
pub unsafe fn write_msr(msr: u32, value: u64) {
    unsafe {
        Msr::new(msr).write(value);
    }
}

#[inline]
pub unsafe fn read_msr(msr: u32) -> u64 {
    unsafe { Msr::new(msr).read() }
}

#[inline]
pub unsafe fn port_read_u8(port: u16) -> u8 {
    unsafe { Port::<u8>::new(port).read() }
}

#[inline]
pub unsafe fn port_write_u8(port: u16, value: u8) {
    unsafe {
        Port::<u8>::new(port).write(value);
    }
}

#[inline]
pub unsafe fn port_read_u16(port: u16) -> u16 {
    unsafe { Port::<u16>::new(port).read() }
}

#[inline]
pub unsafe fn port_write_u16(port: u16, value: u16) {
    unsafe {
        Port::<u16>::new(port).write(value);
    }
}

#[inline]
pub unsafe fn port_read_u32(port: u16) -> u32 {
    unsafe { Port::<u32>::new(port).read() }
}

#[inline]
pub unsafe fn port_write_u32(port: u16, value: u32) {
    unsafe {
        Port::<u32>::new(port).write(value);
    }
}

#[inline]
pub unsafe fn prepare_pit_channel2_oneshot(count: u16) {
    unsafe {
        let gate = (port_read_u8(PIT_SPEAKER_GATE_PORT) & 0xFC) | 0x01;
        port_write_u8(PIT_SPEAKER_GATE_PORT, gate);
        port_write_u8(PIT_CONTROL_PORT, 0b1011_0000);
        port_write_u8(PIT_CHANNEL2_PORT, (count & 0xFF) as u8);
        port_write_u8(PIT_CHANNEL2_PORT, (count >> 8) as u8);
    }
}

#[inline]
pub unsafe fn pit_channel2_terminal_count() -> bool {
    unsafe { port_read_u8(PIT_SPEAKER_GATE_PORT) & 0x20 != 0 }
}

#[inline]
pub unsafe fn read_cmos_register(reg: u8) -> u8 {
    unsafe {
        port_write_u8(CMOS_ADDRESS_PORT, reg);
        port_read_u8(CMOS_DATA_PORT)
    }
}
