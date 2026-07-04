//! Interrupt Descriptor Table setup for x86_64.

use core::{
    arch::{asm, global_asm},
    mem::size_of,
};

use crate::arch::x86_64::sync::StaticCell;

const IDT_ENTRY_COUNT: usize = 256;
const IDT_INTERRUPT_GATE_ATTRIBUTES: u8 = 0x8E;
const ERROR_CODE_VECTORS: &[u8] = &[8, 10, 11, 12, 13, 14, 17, 21, 29, 30];

static IDT: StaticCell<InterruptDescriptorTable> = StaticCell::new(InterruptDescriptorTable::new());

global_asm!(
    ".global saios_default_interrupt_stub",
    "saios_default_interrupt_stub:",
    "iretq",
    ".global saios_default_interrupt_stub_with_error",
    "saios_default_interrupt_stub_with_error:",
    "add rsp, 8",
    "iretq",
    ".global saios_divide_error_stub",
    "saios_divide_error_stub:",
    "call divide_error",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_breakpoint_stub",
    "saios_breakpoint_stub:",
    "call breakpoint",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_invalid_opcode_stub",
    "saios_invalid_opcode_stub:",
    "call invalid_opcode",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_double_fault_stub",
    "saios_double_fault_stub:",
    "mov rdi, [rsp]",
    "call double_fault",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_general_protection_stub",
    "saios_general_protection_stub:",
    "mov rdi, [rsp]",
    "call general_protection",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_page_fault_stub",
    "saios_page_fault_stub:",
    "mov rdi, [rsp]",
    "call page_fault",
    "2:",
    "hlt",
    "jmp 2b",
);

unsafe extern "C" {
    fn saios_default_interrupt_stub();
    fn saios_default_interrupt_stub_with_error();
    fn saios_divide_error_stub();
    fn saios_breakpoint_stub();
    fn saios_invalid_opcode_stub();
    fn saios_double_fault_stub();
    fn saios_general_protection_stub();
    fn saios_page_fault_stub();
}

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
        self.set_handler_addr(addr);
    }

    pub fn set_handler_addr(&mut self, addr: usize) {
        self.offset_low = addr as u16;
        self.selector = crate::arch::x86_64::gdt::KERNEL_CODE.0;
        self.ist = 0;
        self.attributes = IDT_INTERRUPT_GATE_ATTRIBUTES;
        self.offset_mid = (addr >> 16) as u16;
        self.offset_high = (addr >> 32) as u32;
        self.reserved = 0;
    }
}

pub struct InterruptDescriptorTable {
    entries: [IdtEntry; IDT_ENTRY_COUNT],
}

impl InterruptDescriptorTable {
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::missing(); IDT_ENTRY_COUNT],
        }
    }

    fn pointer(&self) -> IdtPointer {
        IdtPointer {
            limit: (size_of::<Self>() - 1) as u16,
            base: self.entries.as_ptr() as u64,
        }
    }
}

impl Default for InterruptDescriptorTable {
    fn default() -> Self {
        Self::new()
    }
}
pub fn register(vector: u8, handler: extern "C" fn()) {
    unsafe {
        let idt = &mut *IDT.get();
        idt.entries[vector as usize].set_handler(handler);
    }
}

pub fn register_raw(vector: u8, handler_addr: usize) {
    unsafe {
        let idt = &mut *IDT.get();
        idt.entries[vector as usize].set_handler_addr(handler_addr);
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
    for vector in 0u8..=255 {
        register_raw(vector, saios_default_interrupt_stub as *const () as usize);
    }

    for &vector in ERROR_CODE_VECTORS {
        register_raw(
            vector,
            saios_default_interrupt_stub_with_error as *const () as usize,
        );
    }

    register_raw(0, saios_divide_error_stub as *const () as usize);
    register_raw(3, saios_breakpoint_stub as *const () as usize);
    register_raw(6, saios_invalid_opcode_stub as *const () as usize);
    register_raw(8, saios_double_fault_stub as *const () as usize);
    register_raw(13, saios_general_protection_stub as *const () as usize);
    register_raw(14, saios_page_fault_stub as *const () as usize);

    load();
}
#[unsafe(no_mangle)]
extern "C" fn divide_error() -> ! {
    panic!("Divide Error");
}

#[unsafe(no_mangle)]
extern "C" fn breakpoint() -> ! {
    panic!("Breakpoint");
}

#[unsafe(no_mangle)]
extern "C" fn invalid_opcode() -> ! {
    panic!("Invalid Opcode");
}

#[unsafe(no_mangle)]
extern "C" fn double_fault(error_code: usize) -> ! {
    panic!("Double Fault (error={:#x})", error_code);
}

#[unsafe(no_mangle)]
extern "C" fn general_protection(error_code: usize) -> ! {
    panic!("General Protection Fault (error={:#x})", error_code);
}

#[unsafe(no_mangle)]
extern "C" fn page_fault(error_code: usize) -> ! {
    panic!("Page Fault (error={:#x})", error_code);
}
