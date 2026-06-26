//! Debug-exception ownership authority.
//!
//! User-mode #DB is only valid when a debugger has deliberately armed single
//! stepping or hardware breakpoints. During the current migration there is no
//! debugger owner, so leaked TF/DR state is treated as kernel-owned diagnostic
//! state that must be cleared at the boundary.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugTrapContext {
    pub pid: u32,
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
}

pub struct DebugContract;

impl DebugContract {
    const TRAP_FLAG: u64 = 1 << 8;
    const RESUME_FLAG: u64 = 1 << 16;

    pub fn sanitize_user_rflags(rflags: u64) -> u64 {
        rflags & !(Self::TRAP_FLAG | Self::RESUME_FLAG)
    }

    pub fn validate_user_debug_trap(context: DebugTrapContext) -> Result<(), &'static str> {
        if context.pid == 0 {
            return Err("debug: user #DB has no current pid");
        }
        if context.rflags & Self::TRAP_FLAG != 0 {
            return Err("debug: user Trap Flag leaked without debugger owner");
        }
        Ok(())
    }

    pub fn dump_user_debug_trap(context: DebugTrapContext, reason: &'static str) {
        crate::observability_contract::ObservabilityContract::contract_violation(
            crate::observability_contract::ContractOwner::Debug,
            "user_debug_trap",
            reason,
            crate::observability_contract::ResourceClass::Process,
            crate::observability_contract::ResourceOwner::Pid(context.pid),
            [context.rip, context.rsp, context.rflags, 0],
        );
        crate::serial_println!(
            "[debug-contract] user #DB reason={} pid={} rip={:#x} rsp={:#x} rflags={:#x}",
            reason,
            context.pid,
            context.rip,
            context.rsp,
            context.rflags
        );
    }
}
