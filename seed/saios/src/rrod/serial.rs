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

pub fn flush() {}

pub fn dump_report(ctx: &RRodContext) {
    crate::drivers::serial::write_str("\n========== RROD DIAGNOSTIC ==========" );
    crate::drivers::serial::write_str("\n" );
    crate::drivers::serial::write_fmt(format_args!("Reason     : {}\n", ctx.reason));
    crate::drivers::serial::write_fmt(format_args!("Exception  : {} (#{})\n", ctx.exception.as_str(), exception_code(ctx.exception)));
    crate::drivers::serial::write_fmt(format_args!("CPU        : {}\n", ctx.cpu));
    crate::drivers::serial::write_fmt(format_args!("RIP        : {:#018x}\n", ctx.rip));
    crate::drivers::serial::write_fmt(format_args!("RSP        : {:#018x}\n", ctx.rsp));
    crate::drivers::serial::write_fmt(format_args!("RBP        : {:#018x}\n", ctx.rbp));
    crate::drivers::serial::write_fmt(format_args!("CR2        : {:#018x}\n", ctx.cr2));
    crate::drivers::serial::write_fmt(format_args!("Error Code : {:#018x}\n", ctx.error_code));
    crate::drivers::serial::write_fmt(format_args!("Location   : {}:{}\n", ctx.file, ctx.line));
    if let Some(pid) = ctx.process {
        crate::drivers::serial::write_fmt(format_args!("Process    : {}\n", pid));
    } else {
        crate::drivers::serial::write_str("Process    : <none>\n");
    }
    if let Some(tid) = ctx.thread {
        crate::drivers::serial::write_fmt(format_args!("Thread     : {}\n", tid));
    } else {
        crate::drivers::serial::write_str("Thread     : <none>\n");
    }
    crate::drivers::serial::write_str("=====================================\n");
}
