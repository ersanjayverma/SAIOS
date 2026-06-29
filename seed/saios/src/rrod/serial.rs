use super::context::{Exception, RRodContext};

#[inline]
fn exception_code(exception: Exception) -> u32 {
    match exception {
        Exception::InvalidOpcode => 6,
        Exception::GeneralProtection => 13,
        Exception::PageFault => 14,
        Exception::Panic => 0xFFFF_FF01,
        Exception::Fatal => 0xFFFF_FF02,
        Exception::Unknown(v) => v,
    }
}

pub fn flush() {
    crate::drivers::serial::flush();
}

/// Dump the RRoD diagnostic report directly to the serial port.
///
/// Uses `drivers::serial` directly (not `console`) for maximum
/// reliability during a crash. The console panic_prelude has already
/// written the panic header to all sinks; this dump adds the detailed
/// register/context information that only needs to reach the serial
/// log.
pub fn dump_report(ctx: &RRodContext) {
    use crate::drivers::serial;

    serial::write_str("\n========== RROD DIAGNOSTIC ==========\n");
    serial::write_fmt(format_args!("Reason     : {}\n", ctx.reason));
    serial::write_fmt(format_args!(
        "Exception  : {} (#{})\n",
        ctx.exception.as_str(),
        exception_code(ctx.exception)
    ));
    serial::write_fmt(format_args!("CPU        : {}\n", ctx.cpu));
    serial::write_fmt(format_args!("RIP        : {:#018x}\n", ctx.rip));
    serial::write_fmt(format_args!("RSP        : {:#018x}\n", ctx.rsp));
    serial::write_fmt(format_args!("RBP        : {:#018x}\n", ctx.rbp));
    serial::write_fmt(format_args!("CR2        : {:#018x}\n", ctx.cr2));
    serial::write_fmt(format_args!("Error Code : {:#018x}\n", ctx.error_code));
    serial::write_fmt(format_args!("Location   : {}:{}\n", ctx.file, ctx.line));
    if let Some(pid) = ctx.process {
        serial::write_fmt(format_args!("Process    : {}\n", pid));
    } else {
        serial::write_str("Process    : <none>\n");
    }
    if let Some(tid) = ctx.thread {
        serial::write_fmt(format_args!("Thread     : {}\n", tid));
    } else {
        serial::write_str("Thread     : <none>\n");
    }
    serial::write_str("--- recent log ring ---\n");
    crate::console::replay_ring(|b| serial::write_byte(b));
    serial::write_str("\n--- end log ring ---\n");
    serial::write_str("=====================================\n");
    serial::flush();
}
