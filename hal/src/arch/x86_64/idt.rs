use core::{arch::asm, mem::size_of};

use crate::arch::x86_64::sync::StaticCell;

static IDT: StaticCell<InterruptDescriptorTable> = StaticCell::new(InterruptDescriptorTable::new());

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    pub const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }
    pub fn set_handler(&mut self, handler: extern "C" fn()) {
        let addr = handler as usize;

        self.offset_low = addr as u16;
        self.selector = crate::arch::x86_64::gdt::KERNEL_CODE.0;
        self.ist = 0;
        self.attributes = 0x8E;
        self.offset_mid = (addr >> 16) as u16;
        self.offset_high = (addr >> 32) as u32;
        self.reserved = 0;
    }
}

pub struct InterruptDescriptorTable {
    entries: [IdtEntry; 256],
}

impl InterruptDescriptorTable {
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::missing(); 256],
        }
    }

    fn pointer(&self) -> IdtPointer {
        IdtPointer {
            limit: (size_of::<Self>() - 1) as u16,
            base: self.entries.as_ptr() as u64,
        }
    }
}
pub fn register(vector: u8, handler: extern "C" fn()) {
    unsafe {
        let idt = &mut *IDT.get();
        idt.entries[vector as usize].set_handler(handler);
    }
}
pub fn load() {
    unsafe {
        let idt = &*IDT.get();

        let ptr = idt.pointer();

        asm!(
            "lidt [{}]",
            in(reg) &ptr,
            options(readonly, nostack, preserves_flags),
        );
    }
}
pub fn init() {
    register(0, divide_error);
    register(3, breakpoint);
    register(6, invalid_opcode);
    register(8, double_fault);
    register(13, general_protection);
    register(14, page_fault);

    load();
}
extern "C" fn divide_error() {
    panic!("Divide Error");
}

extern "C" fn breakpoint() {
    panic!("Breakpoint");
}

extern "C" fn invalid_opcode() {
    panic!("Invalid Opcode");
}

extern "C" fn double_fault() {
    panic!("Double Fault");
}

extern "C" fn general_protection() {
    panic!("General Protection Fault");
}

extern "C" fn page_fault() {
    panic!("Page Fault");
}
