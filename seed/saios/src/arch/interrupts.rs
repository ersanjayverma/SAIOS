#[repr(C, packed)]
#[derive(Copy, Clone)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    fn set_handler(&mut self, handler: u64, selector: u16) {
        self.offset_low = handler as u16;
        self.selector = selector;
        self.ist = 0;
        self.attributes = 0x8E;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.zero = 0;
    }
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

unsafe extern "C" {
    fn seed_isr_ud();
    fn seed_isr_gp();
    fn seed_isr_pf();
}

core::arch::global_asm!(
    r#"
    .global seed_isr_ud
seed_isr_ud:
    cli
    mov rdi, rsp
    mov esi, 6
    xor edx, edx
    call seed_exception_from_stack
1:
    hlt
    jmp 1b

    .global seed_isr_gp
seed_isr_gp:
    cli
    mov rdi, rsp
    mov esi, 13
    mov edx, 1
    call seed_exception_from_stack
2:
    hlt
    jmp 2b

    .global seed_isr_pf
seed_isr_pf:
    cli
    mov rdi, rsp
    mov esi, 14
    mov edx, 1
    call seed_exception_from_stack
3:
    hlt
    jmp 3b
"#
);

pub fn install_exception_handlers() {
    let mut cs: u16;
    unsafe {
        core::arch::asm!("mov {0:x}, cs", out(reg) cs, options(nomem, nostack, preserves_flags));
    }

    unsafe {
        IDT[6].set_handler(seed_isr_ud as *const () as usize as u64, cs);
        IDT[13].set_handler(seed_isr_gp as *const () as usize as u64, cs);
        IDT[14].set_handler(seed_isr_pf as *const () as usize as u64, cs);

        let idtr = Idtr {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: core::ptr::addr_of!(IDT) as u64,
        };

        core::arch::asm!("lidt [{}]", in(reg) &idtr, options(readonly, nostack, preserves_flags));
    }
}
