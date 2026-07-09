//! Interrupt Descriptor Table setup for x86_64.

use crate::arch::x86_64::constants::{IDT_ENTRY_COUNT, IDT_INTERRUPT_GATE_ATTRS};
use core::{
    arch::{asm, global_asm},
    mem::size_of,
};

use crate::arch::x86_64::sync::StaticCell;

const ERROR_CODE_VECTORS: &[u8] = &[8, 10, 11, 12, 13, 14, 17, 21, 29, 30];
const IST_STACK_SIZE: usize = 16 * 1024;

#[repr(align(16))]
struct IstStack([u8; IST_STACK_SIZE]);

static IDT: StaticCell<InterruptDescriptorTable> = StaticCell::new(InterruptDescriptorTable::new());
static DF_IST_STACK: StaticCell<IstStack> = StaticCell::new(IstStack([0; IST_STACK_SIZE]));
static GP_IST_STACK: StaticCell<IstStack> = StaticCell::new(IstStack([0; IST_STACK_SIZE]));
/// Dedicated stack for hardware IRQs that can interrupt ring 3 (currently
/// just the PIT timer, vector 32). Without an IST, a ring3->ring0 interrupt
/// falls back to TSS.RSP0 for the stack switch; that path raises #SS the
/// instant the timer fires while user code is running, which cascades to a
/// double fault whose own delivery also hits #SS, forcing a triple fault.
/// Confirmed via QEMU's `-d int` trace: v=20 (timer) at cpl=3 -> #SS -> #DF
/// -> #SS again -> triple fault, reproducible on the very first ring3 entry.
static IRQ_IST_STACK: StaticCell<IstStack> = StaticCell::new(IstStack([0; IST_STACK_SIZE]));
type InvalidOpcodeHandler = fn(stack_ptr: usize) -> bool;
type GeneralProtectionHandler = fn(error_code: usize, stack_ptr: usize) -> bool;
type PageFaultHandler = fn(fault_addr: usize, error_code: usize, stack_ptr: usize) -> bool;
type UserFaultAbortHandler = extern "C" fn() -> !;
static INVALID_OPCODE_HANDLER: StaticCell<Option<InvalidOpcodeHandler>> = StaticCell::new(None);
static GENERAL_PROTECTION_HANDLER: StaticCell<Option<GeneralProtectionHandler>> = StaticCell::new(None);
static PAGE_FAULT_HANDLER: StaticCell<Option<PageFaultHandler>> = StaticCell::new(None);
static USER_FAULT_ABORT_HANDLER: StaticCell<Option<UserFaultAbortHandler>> = StaticCell::new(None);
const PAGE_FAULT_WALK_TRACE: bool = false;

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
    "push rax",
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
    "push rax",
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
    "push rax",
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
    "push rax",
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
    "push rax",
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

pub fn set_ist(vector: u8, ist: u8) {
    unsafe {
        let idt = &mut *IDT.get();
        idt.entries[vector as usize].ist = ist;
    }
}

/// Debug readback: (ist field, offset, selector, attributes) for a vector.
pub fn debug_entry(vector: u8) -> (u8, u64, u16, u8) {
    unsafe {
        let idt = &*IDT.get();
        let e = &idt.entries[vector as usize];
        let offset = (e.offset_low as u64)
            | ((e.offset_mid as u64) << 16)
            | ((e.offset_high as u64) << 32);
        (e.ist, offset, e.selector, e.attributes)
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
    // Provide dedicated fault stacks so exception delivery during CPL
    // transitions can still run handlers even if the current stack is broken.
    let df_top = unsafe { (*DF_IST_STACK.get()).0.as_ptr().add(IST_STACK_SIZE) as u64 };
    let gp_top = unsafe { (*GP_IST_STACK.get()).0.as_ptr().add(IST_STACK_SIZE) as u64 };
    let irq_top = unsafe { (*IRQ_IST_STACK.get()).0.as_ptr().add(IST_STACK_SIZE) as u64 };
    crate::arch::x86_64::tss::set_ist(0, df_top);
    crate::arch::x86_64::tss::set_ist(1, gp_top);
    crate::arch::x86_64::tss::set_ist(2, irq_top);

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
    set_ist(8, 1);
    // Task-switch, segment-not-present, stack-segment, and general-
    // protection faults commonly surface during CPL3 entry via iretq and
    // use the same error-code frame shape, so route them through the same
    // logging/recovery path. They are NOT given a dedicated IST, matching
    // #PF below: an IST forces a stack switch unconditionally, even when
    // the fault occurs in ring0 already (no privilege change, so no switch
    // is actually needed) -- and that forced switch was confirmed, for
    // #PF, to fail outright under VirtualBox's NEM backend specifically
    // (silently cascading to a triple fault before a single instruction of
    // our own handler ran), while working correctly under QEMU. Falling
    // back to TSS.RSP0-based switching (a real CPL3->CPL0 transition) or no
    // switch at all (already CPL0) avoids that mechanism. #DF (above) is
    // deliberately kept on a dedicated IST regardless -- it exists
    // specifically to run on a known-good stack when the current one may
    // itself be corrupt, which is architecturally different from these.
    register_raw(10, saios_invalid_tss_stub as *const () as usize);
    register_raw(11, saios_segment_not_present_stub as *const () as usize);
    register_raw(12, saios_stack_segment_stub as *const () as usize);
    register_raw(13, saios_general_protection_stub as *const () as usize);
    register_raw(14, saios_page_fault_stub as *const () as usize);

    // The PIT timer IRQ (vector 32) also needs IST slot 3 (irq_top, tss
    // index 2) — see IRQ_IST_STACK above for why. It can't be wired up here:
    // timer::init() calls register_raw() for vector 32 *after* this function
    // runs, and register_raw()/set_handler_addr() unconditionally clears the
    // entry's IST field. timer::init() re-applies set_ist(32, 3) itself,
    // immediately after its register_raw() call.

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

    // Keep deep page-table walk logging opt-in; default logs stay concise.
    if PAGE_FAULT_WALK_TRACE {
        const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;
        let cr3_phys = cr3 & ADDR_MASK;

        let walk = |label: &str, va: u64| {
            let l4 = ((va >> 39) & 0x1ff) as usize;
            let l3 = ((va >> 30) & 0x1ff) as usize;
            let l2 = ((va >> 21) & 0x1ff) as usize;
            let l1 = ((va >> 12) & 0x1ff) as usize;

            let pml4e = unsafe { *((cr3_phys as *const u64).add(l4)) };
            crate::arch::x86_64::console::_print_force(format_args!(
                "[pf-walk:{}] va={:#x} l4={} pml4e={:#018x}\n",
                label,
                va,
                l4,
                pml4e
            ));

            if pml4e & 0x1 == 0 {
                return;
            }

            let pdpt = (pml4e & ADDR_MASK) as *const u64;
            let pdpte = unsafe { *pdpt.add(l3) };
            crate::arch::x86_64::console::_print_force(format_args!(
                "[pf-walk:{}] l3={} pdpte={:#018x}\n",
                label,
                l3,
                pdpte
            ));

            if pdpte & 0x1 == 0 || pdpte & 0x80 != 0 {
                return;
            }

            let pd = (pdpte & ADDR_MASK) as *const u64;
            let pde = unsafe { *pd.add(l2) };
            crate::arch::x86_64::console::_print_force(format_args!(
                "[pf-walk:{}] l2={} pde={:#018x} huge={}\n",
                label,
                l2,
                pde,
                (pde & 0x80 != 0) as u8
            ));

            if pde & 0x1 == 0 || pde & 0x80 != 0 {
                return;
            }

            let pt = (pde & ADDR_MASK) as *const u64;
            let pte = unsafe { *pt.add(l1) };
            crate::arch::x86_64::console::_print_force(format_args!(
                "[pf-walk:{}] l1={} pte={:#018x}\n",
                label,
                l1,
                pte
            ));
        };

        walk("cr2", fault_addr as u64);
        walk("rip", saved_rip as u64);
        walk("rsp", saved_rsp as u64);
    }
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
