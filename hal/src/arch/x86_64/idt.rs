//! Interrupt Descriptor Table setup for x86_64.

use crate::arch::x86_64::constants::{IDT_ENTRY_COUNT, IDT_INTERRUPT_GATE_ATTRS};
use core::{
    arch::{asm, global_asm},
    mem::size_of,
};

use crate::arch::x86_64::sync::StaticCell;

const ERROR_CODE_VECTORS: &[u8] = &[8, 10, 11, 12, 13, 14, 17, 21, 29, 30];

static IDT: StaticCell<InterruptDescriptorTable> = StaticCell::new(InterruptDescriptorTable::new());
type InvalidOpcodeHandler = fn(stack_ptr: usize) -> bool;
type GeneralProtectionHandler = fn(error_code: usize, stack_ptr: usize) -> bool;
type PageFaultHandler = fn(fault_addr: usize, error_code: usize, stack_ptr: usize) -> bool;
type UserFaultAbortHandler = extern "C" fn() -> !;
static INVALID_OPCODE_HANDLER: StaticCell<Option<InvalidOpcodeHandler>> = StaticCell::new(None);
static GENERAL_PROTECTION_HANDLER: StaticCell<Option<GeneralProtectionHandler>> = StaticCell::new(None);
static PAGE_FAULT_HANDLER: StaticCell<Option<PageFaultHandler>> = StaticCell::new(None);
static USER_FAULT_ABORT_HANDLER: StaticCell<Option<UserFaultAbortHandler>> = StaticCell::new(None);

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
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "lea rdi, [rsp + 64]",
    "call invalid_opcode",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "test rax, rax",
    "jnz 3f",
    "2:",
    "hlt",
    "jmp 2b",
    "3:",
    "call saios_user_fault_abort",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_double_fault_stub",
    "saios_double_fault_stub:",
    "push rax",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "mov rdi, [rsp + 72]",
    "call double_fault",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rax",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_invalid_tss_stub",
    "saios_invalid_tss_stub:",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "mov edi, 10",
    "mov rsi, [rsp + 64]",
    "lea rdx, [rsp + 64]",
    "call selector_fault",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "test rax, rax",
    "jnz 3f",
    "2:",
    "hlt",
    "jmp 2b",
    "3:",
    "add rsp, 8",
    "call saios_user_fault_abort",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_segment_not_present_stub",
    "saios_segment_not_present_stub:",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "mov edi, 11",
    "mov rsi, [rsp + 64]",
    "lea rdx, [rsp + 64]",
    "call selector_fault",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "test rax, rax",
    "jnz 3f",
    "2:",
    "hlt",
    "jmp 2b",
    "3:",
    "add rsp, 8",
    "call saios_user_fault_abort",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_stack_segment_stub",
    "saios_stack_segment_stub:",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "mov edi, 12",
    "mov rsi, [rsp + 64]",
    "lea rdx, [rsp + 64]",
    "call selector_fault",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "test rax, rax",
    "jnz 3f",
    "2:",
    "hlt",
    "jmp 2b",
    "3:",
    "add rsp, 8",
    "call saios_user_fault_abort",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_general_protection_stub",
    "saios_general_protection_stub:",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "mov rdi, [rsp + 64]",
    "lea rsi, [rsp + 64]",
    "call general_protection",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "test rax, rax",
    "jnz 3f",
    "2:",
    "hlt",
    "jmp 2b",
    "3:",
    "add rsp, 8",
    "call saios_user_fault_abort",
    "2:",
    "hlt",
    "jmp 2b",
    ".global saios_page_fault_stub",
    "saios_page_fault_stub:",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "mov rdi, [rsp + 64]",
    "lea rsi, [rsp + 64]",
    "call page_fault",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "test rax, rax",
    "jnz 3f",
    "2:",
    "hlt",
    "jmp 2b",
    "3:",
    "add rsp, 8",
    "call saios_user_fault_abort",
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
    fn saios_invalid_tss_stub();
    fn saios_segment_not_present_stub();
    fn saios_stack_segment_stub();
    fn saios_general_protection_stub();
    fn saios_page_fault_stub();
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
        self.attributes = IDT_INTERRUPT_GATE_ATTRS;
        self.offset_mid = (addr >> 16) as u16;
        self.offset_high = (addr >> 32) as u32;
        self.reserved = 0;
    }
}
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct InterruptDescriptorTable {
    entries: [IdtEntry; IDT_ENTRY_COUNT],
}

impl InterruptDescriptorTable {
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::missing(); IDT_ENTRY_COUNT],
        }
    }

    fn pointer_bytes(&self) -> [u8; 10] {
        let limit = (size_of::<Self>() - 1) as u16;
        let base = self.entries.as_ptr() as u64;
        let mut raw = [0u8; 10];
        raw[0..2].copy_from_slice(&limit.to_le_bytes());
        raw[2..10].copy_from_slice(&base.to_le_bytes());
        raw
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

#[inline(always)]
pub fn load_null_idt() {
    let raw: [u8; 10] = [0; 10];
    unsafe {
        asm!("lidt [{}]", in(reg) raw.as_ptr(), options(readonly, nostack, preserves_flags));
    }
}
pub fn load() {
    unsafe {
        let idt = &*IDT.get();

        let ptr = idt.pointer_bytes();

        asm!(
            "lidt [{}]",
            in(reg) ptr.as_ptr(),
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
    // Task-switch, segment-not-present, and stack-segment faults commonly
    // surface during CPL3 entry via iretq and use the same error-code frame
    // shape as #GP, so route them through the same logging/recovery path.
    register_raw(10, saios_invalid_tss_stub as *const () as usize);
    register_raw(11, saios_segment_not_present_stub as *const () as usize);
    register_raw(12, saios_stack_segment_stub as *const () as usize);
    register_raw(13, saios_general_protection_stub as *const () as usize);
    register_raw(14, saios_page_fault_stub as *const () as usize);

    load();
}

pub fn set_page_fault_handler(handler: PageFaultHandler) {
    unsafe {
        *PAGE_FAULT_HANDLER.get() = Some(handler);
    }
}

pub fn set_invalid_opcode_handler(handler: InvalidOpcodeHandler) {
    unsafe {
        *INVALID_OPCODE_HANDLER.get() = Some(handler);
    }
}

pub fn set_general_protection_handler(handler: GeneralProtectionHandler) {
    unsafe {
        *GENERAL_PROTECTION_HANDLER.get() = Some(handler);
    }
}

pub fn set_user_fault_abort_handler(handler: UserFaultAbortHandler) {
    unsafe {
        *USER_FAULT_ABORT_HANDLER.get() = Some(handler);
    }
}

#[unsafe(no_mangle)]
extern "C" fn saios_user_fault_abort() -> ! {
    let handler = unsafe { *USER_FAULT_ABORT_HANDLER.get() };
    if let Some(h) = handler {
        h();
    }

    loop {
        crate::arch::x86_64::cpu::hlt();
    }
}
#[unsafe(no_mangle)]
extern "C" fn divide_error() -> ! {
    crate::arch::x86_64::console::_print_force(format_args!("[fault] #DE\n"));
    panic!("Divide Error");
}

#[unsafe(no_mangle)]
extern "C" fn breakpoint() -> ! {
    crate::arch::x86_64::console::_print_force(format_args!("[fault] #BP\n"));
    panic!("Breakpoint");
}

#[unsafe(no_mangle)]
extern "C" fn invalid_opcode(stack_ptr: usize) -> usize {
    let saved_rip = unsafe { *(stack_ptr as *const usize) };
    let cr3 = crate::arch::x86_64::paging::read_cr3();
    crate::arch::x86_64::console::_print_force(format_args!("[fault] #UD sp={:#x}\n", stack_ptr));
    crate::arch::x86_64::console::_print_force(format_args!(
        "[fault] #UD rip={:#x} cr3={:#x}\n",
        saved_rip,
        cr3
    ));
    let handled = unsafe {
        (*INVALID_OPCODE_HANDLER.get())
            .map(|h| h(stack_ptr))
            .unwrap_or(false)
    };
    if handled {
        return 1;
    }

    panic!("Invalid Opcode");
}

#[unsafe(no_mangle)]
extern "C" fn double_fault(error_code: usize) -> ! {
    crate::arch::x86_64::console::_print_force(format_args!(
        "[fault] #DF err={:#x}\n",
        error_code
    ));
    panic!(
        "Double Fault (error={:#x}, reason={})",
        error_code,
        if error_code == 0 {
            "task-state/stack/descriptor escalation"
        } else {
            "unexpected non-zero error code"
        }
    );
}

#[unsafe(no_mangle)]
extern "C" fn general_protection(error_code: usize, stack_ptr: usize) -> usize {
    let saved_rip = unsafe { *((stack_ptr as *const usize).add(1)) };
    let saved_rsp = unsafe { *((stack_ptr as *const usize).add(4)) };
    let cr3 = crate::arch::x86_64::paging::read_cr3();
    crate::arch::x86_64::console::_print_force(format_args!(
        "[fault] #GP err={:#x} sp={:#x}\n",
        error_code,
        stack_ptr
    ));
    crate::arch::x86_64::console::_print_force(format_args!(
        "[fault] #GP rip={:#x} rsp={:#x} cr3={:#x}\n",
        saved_rip,
        saved_rsp,
        cr3
    ));
    let handled = unsafe {
        (*GENERAL_PROTECTION_HANDLER.get())
            .map(|h| h(error_code, stack_ptr))
            .unwrap_or(false)
    };
    if handled {
        return 1;
    }

    let ext = (error_code & 0x1) != 0;
    let idt = (error_code & 0x2) != 0;
    let ti = (error_code & 0x4) != 0;
    let selector_index = (error_code >> 3) & 0x1FFF;
    panic!(
        "General Protection Fault (error={:#x}, ext={}, idt={}, table={}, selector_index={})",
        error_code,
        ext,
        idt,
        if ti { "ldt" } else { "gdt" },
        selector_index
    );
}

#[unsafe(no_mangle)]
extern "C" fn selector_fault(vector: usize, error_code: usize, stack_ptr: usize) -> usize {
    let saved_rip = unsafe { *((stack_ptr as *const usize).add(1)) };
    let saved_rsp = unsafe { *((stack_ptr as *const usize).add(4)) };
    let cr3 = crate::arch::x86_64::paging::read_cr3();
    let name = match vector {
        10 => "#TS",
        11 => "#NP",
        12 => "#SS",
        _ => "#SEL",
    };
    crate::arch::x86_64::console::_print_force(format_args!(
        "[fault] {} err={:#x} sp={:#x}\n",
        name,
        error_code,
        stack_ptr
    ));
    crate::arch::x86_64::console::_print_force(format_args!(
        "[fault] {} rip={:#x} rsp={:#x} cr3={:#x}\n",
        name,
        saved_rip,
        saved_rsp,
        cr3
    ));

    let handled = unsafe {
        (*GENERAL_PROTECTION_HANDLER.get())
            .map(|h| h(error_code, stack_ptr))
            .unwrap_or(false)
    };
    if handled {
        return 1;
    }

    let ext = (error_code & 0x1) != 0;
    let idt = (error_code & 0x2) != 0;
    let ti = (error_code & 0x4) != 0;
    let selector_index = (error_code >> 3) & 0x1FFF;
    panic!(
        "{} (error={:#x}, ext={}, idt={}, table={}, selector_index={})",
        name,
        error_code,
        ext,
        idt,
        if ti { "ldt" } else { "gdt" },
        selector_index
    );
}

#[unsafe(no_mangle)]
extern "C" fn page_fault(error_code: usize, stack_ptr: usize) -> usize {
    let fault_addr = crate::arch::x86_64::cpu::read_cr2();
    let saved_rip = unsafe { *((stack_ptr as *const usize).add(1)) };
    let saved_rsp = unsafe { *((stack_ptr as *const usize).add(4)) };
    let cr3 = crate::arch::x86_64::paging::read_cr3();
    crate::arch::x86_64::console::_print_force(format_args!(
        "[fault] #PF err={:#x} cr2={:#x} sp={:#x}\n",
        error_code,
        fault_addr,
        stack_ptr
    ));
    crate::arch::x86_64::console::_print_force(format_args!(
        "[fault] #PF rip={:#x} rsp={:#x} cr3={:#x}\n",
        saved_rip,
        saved_rsp,
        cr3
    ));
    let present = (error_code & (1 << 0)) != 0;
    let write = (error_code & (1 << 1)) != 0;
    let user = (error_code & (1 << 2)) != 0;
    let reserved_bit_violation = (error_code & (1 << 3)) != 0;
    let instruction_fetch = (error_code & (1 << 4)) != 0;
    let protection_key = (error_code & (1 << 5)) != 0;
    let shadow_stack = (error_code & (1 << 6)) != 0;
    let sgx = (error_code & (1 << 15)) != 0;

    let handled = unsafe {
        (*PAGE_FAULT_HANDLER.get())
            .map(|h| h(fault_addr as usize, error_code, stack_ptr))
            .unwrap_or(false)
    };
    if handled {
        return 1;
    }

    panic!(
        "Page Fault (error={:#x}, cr2={:#x}, present={}, write={}, user={}, rsvd={}, ifetch={}, pkey={}, sstk={}, sgx={})",
        error_code,
        fault_addr,
        present,
        write,
        user,
        reserved_bit_violation,
        instruction_fetch,
        protection_key,
        shadow_stack,
        sgx
    );
}
