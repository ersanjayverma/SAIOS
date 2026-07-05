//! x86_64 hardware abstraction layer.
//!
//! Provides low-level drivers and data structures for the x86_64 architecture:
//! GDT, IDT, APIC, PCI, serial, console and platform helpers.

pub mod apic;
pub mod console;
pub mod cpu;
pub mod cpuid;
pub mod gdt;
pub mod idt;
pub mod interrupt;
pub mod io;
pub mod ioapic;
pub mod lapic;
pub mod mmio;
pub mod msr;
pub mod paging;
pub mod pci;
pub mod pit;
pub mod platform;
pub mod rtc;
pub mod seed_support;
pub mod serial;
pub mod syscall;
pub mod sync;
pub mod tss;
pub mod volatile;
