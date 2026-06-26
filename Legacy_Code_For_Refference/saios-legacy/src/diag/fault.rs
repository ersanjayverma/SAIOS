//! Fault-time register / context dump.
//!
//! Used by the `#PF` / `#GP` / `#UD` / `#DF` exception handlers in
//! `src/interrupts.rs` to print a single coherent record of "where
//! were we when the fault fired".  The output goes to the serial
//! port *before* the panic/halt so it survives a kernel crash.
//!
//! Why this lives in `diag::fault` rather than in `interrupts.rs`:
//! the IDT frame only gives us RIP/RSP/RFLAGS/CS/SS/err.  To print
//! the rest of the GP registers (RAX-R15) we have to read them from
//! a small save area the IDT frame's caller pushed, OR we accept
//! that the caller's `rdi`/`rsi`/`rdx` etc are not visible.  For now
//! we only print the IDT-frame fields; expanding to a full register
//! dump is future work (would need a small wrapper at every
//! exception handler site to push the rest of the GP regs first).
//!
//! The IDT-frame dump is plenty for the most common cases: a #PF
//! with `cpl=3` and `cr2=0` is a NULL deref in user space; a #GP
//! at `cpl=3` with `opcode=0xEC` is the I/O-port trap we've now
//! routed around; a #DF means a kernel stack overflow and the
//! surviving context is the IDT frame.

use x86_64::structures::idt::InterruptStackFrame;

/// Print a single fault record: name, IDT frame fields, the CR3 the
/// CPU is using, the current process (if any), and a 32-byte window
/// of code around the faulting RIP.
///
/// Call this *before* the panic handler — once we are in `hlt_loop`,
/// no other output will be emitted.
///
/// `extra_err` is rendered as a hex u64 and is intended for the
/// `PageFaultErrorCode` bitfield for `#PF` or the raw error code for
/// the others.  The label is what comes after the name, e.g.
/// "PF" or "GP".
pub fn dump(label: &str, frame: &InterruptStackFrame, extra_err: u64) {
    let rip = frame.instruction_pointer.as_u64();
    let rsp = frame.stack_pointer.as_u64();
    let cs = frame.code_segment;
    let cpl = cs & 3;

    let cr3 = crate::memory::paging::active_pml4();
    let pid_name = if let Some(table) = crate::process::table::TABLE.try_lock() {
        match table.current_ref() {
            Some(p) => alloc::format!("pid={} name='{}'", p.pid, p.name),
            None => alloc::string::String::from("pid=<kernel>"),
        }
    } else {
        alloc::string::String::from("pid=<unknown> table_lock=held")
    };

    crate::serial_println!("\n[#{}]", label);
    crate::serial_println!("  rip={:#x} rsp={:#x}", rip, rsp);
    crate::serial_println!(
        "  cs={:#x} cpl={} ss={:#x} rflags={:#x} err={:#x}",
        cs,
        cpl,
        frame.stack_segment,
        frame.cpu_flags,
        extra_err
    );
    crate::serial_println!("  cr3={:#x}  {}", cr3, pid_name);
    crate::serial_println!("  bytes around RIP:");
    dump_code_around(rip, 32);
}

/// Print 32 bytes around `virt` as 4 lines of 8 hex bytes + ASCII
/// (non-printable bytes shown as '.').  Skips the dump gracefully if
/// the address is not mapped (unmapped kernel RIPs do happen on
/// faults-during-faults; in that case the dump just says "unmapped").
pub fn dump_code_around(virt: u64, window: usize) {
    let aligned = virt & !0xF;
    let start = aligned.saturating_sub((window / 2) as u64);
    for off in (0..window).step_by(16) {
        let addr = start + off as u64;
        let mut bytes = [0u8; 16];
        let mut got = 0usize;
        for (i, b) in bytes.iter_mut().enumerate() {
            match read_user_byte_safe(addr + i as u64) {
                Some(by) => {
                    *b = by;
                    got += 1;
                }
                None => break,
            }
        }
        if got == 0 {
            crate::serial_println!("    {:#x}: (unmapped)", addr);
            return;
        }
        let mut hex = alloc::string::String::new();
        let mut txt = alloc::string::String::new();
        for b in &bytes[..got] {
            hex.push_str(alloc::format!("{:02x} ", b).as_str());
            let c = if (0x20..0x7F).contains(b) {
                *b as char
            } else {
                '.'
            };
            txt.push(c);
        }
        crate::serial_println!("    {:#x}: {:<48}  {}", addr, hex, txt);
    }
}

/// Read one byte from the active address space; returns None if the
/// page is not present.  Used by the code dump so a faulted RIP
/// (e.g. inside a kernel NULL deref) doesn't take down the dump.
fn read_user_byte_safe(virt: u64) -> Option<u8> {
    use crate::memory::paging;
    let pml4 = paging::active_pml4();
    let (phys, flags) = paging::translate_entry_in(pml4, virt)?;
    if flags & paging::PTE_PRESENT == 0 {
        return None;
    }
    let phys = phys + (virt & 0xFFF);
    Some(unsafe { *(phys as *const u8) })
}
