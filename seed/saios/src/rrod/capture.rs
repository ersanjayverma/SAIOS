use super::context::{Exception, RRodContext};

#[inline]
fn read_cr2() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, cr2",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

#[inline]
fn read_rsp() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rsp",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

#[inline]
fn read_rbp() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rbp",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

#[inline]
fn read_rip() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!(
            "lea {}, [rip]",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// # Safety
///
/// `stack` must point to a valid exception stack frame laid out according to
/// the current interrupt/exception entry stub.
pub unsafe fn from_exception_stack(
    stack: *const u64,
    vector: u32,
    has_error_code: bool,
) -> RRodContext {
    let (rip, error_code) = unsafe {
        if has_error_code {
            (*stack.add(1), *stack)
        } else {
            (*stack, 0)
        }
    };

    let exception = match vector {
        6 => Exception::InvalidOpcode,
        13 => Exception::GeneralProtection,
        14 => Exception::PageFault,
        _ => Exception::Unknown(vector),
    };

    RRodContext {
        reason: exception.as_str(),
        exception,
        cpu: 0,
        rip,
        rsp: stack as u64,
        rbp: 0,
        cr2: read_cr2(),
        error_code,
        file: "<exception>",
        line: 0,
        process: None,
        thread: None,
    }
}

pub fn from_panic(info: &core::panic::PanicInfo<'_>) -> RRodContext {
    let line = if let Some(loc) = info.location() {
        loc.line()
    } else {
        0
    };

    RRodContext {
        reason: "SEED panic",
        exception: Exception::Panic,
        cpu: 0,
        rip: read_rip(),
        rsp: read_rsp(),
        rbp: read_rbp(),
        cr2: read_cr2(),
        error_code: 0,
        file: "<panic>",
        line,
        process: None,
        thread: None,
    }
}

pub fn from_fatal(reason: &'static str) -> RRodContext {
    RRodContext {
        reason,
        exception: Exception::Fatal,
        cpu: 0,
        rip: read_rip(),
        rsp: read_rsp(),
        rbp: read_rbp(),
        cr2: read_cr2(),
        error_code: 0,
        file: "<runtime>",
        line: 0,
        process: None,
        thread: None,
    }
}
