//! Signal delivery - pushes a Linux-compatible sigframe onto the user stack
//! so the handler can run and rt_sigreturn can restore register state.

use crate::memory::paging;
use alloc::collections::BTreeMap;

// Signal numbers (same as Linux)
pub const SIGHUP: u32 = 1;
pub const SIGINT: u32 = 2;
pub const SIGQUIT: u32 = 3;
pub const SIGILL: u32 = 4;
pub const SIGTRAP: u32 = 5;
pub const SIGABRT: u32 = 6;
pub const SIGBUS: u32 = 7;
pub const SIGFPE: u32 = 8;
pub const SIGKILL: u32 = 9;
pub const SIGUSR1: u32 = 10;
pub const SIGSEGV: u32 = 11;
pub const SIGUSR2: u32 = 12;
pub const SIGPIPE: u32 = 13;
pub const SIGALRM: u32 = 14;
pub const SIGTERM: u32 = 15;
pub const SIGCHLD: u32 = 17;
pub const SIGCONT: u32 = 18;
pub const SIGSTOP: u32 = 19;
pub const SIGWINCH: u32 = 28;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SigAction {
    Default,
    Ignore,
    Handler {
        func: u64,
        flags: u64,
        mask: u64,
        restorer: u64,
    },
}

/// Per-process signal state stored inside `Process`.
#[derive(Clone)]
pub struct SigState {
    pub handlers: BTreeMap<u32, SigAction>,
    pub pending: u64,
    pub blocked: u64,
}

impl SigState {
    pub fn new() -> Self {
        Self {
            handlers: BTreeMap::new(),
            pending: 0,
            blocked: 0,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.next_actionable().is_some()
    }

    pub fn raise(&mut self, sig: u32) {
        self.pending |= 1u64 << sig;
    }

    pub fn next_actionable(&self) -> Option<u32> {
        let mut actionable = self.pending & !self.blocked;
        while actionable != 0 {
            let sig = actionable.trailing_zeros();
            match self.action(sig) {
                SigAction::Ignore => {}
                SigAction::Default if default_action_is_ignored(sig) => {}
                _ => return Some(sig),
            }
            actionable &= !(1u64 << sig);
        }
        None
    }

    pub fn action(&self, sig: u32) -> SigAction {
        self.handlers
            .get(&sig)
            .copied()
            .unwrap_or(SigAction::Default)
    }

    pub fn set_action(&mut self, sig: u32, action: SigAction) {
        if sig == SIGKILL || sig == SIGSTOP {
            return;
        }
        self.handlers.insert(sig, action);
    }
}

fn default_action_is_ignored(sig: u32) -> bool {
    matches!(sig, SIGCHLD | SIGWINCH | SIGCONT)
}

#[repr(C)]
pub struct RtSigFrame {
    pub pretcode: u64,
    pub sig: u32,
    _pad: u32,
    pub siginfo: [u8; 128],
    pub uc_flags: u64,
    pub uc_link: u64,
    pub uc_stack: [u64; 3],
    pub regs: SigRegs,
    pub fpstate: u64,
    pub _pad2: [u8; 64],
}

#[repr(C)]
pub struct SigRegs {
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rsp: u64,
    pub rip: u64,
    pub eflags: u64,
    pub cs: u16,
    pub gs: u16,
    pub fs: u16,
    pub _pad: u16,
    pub err: u64,
    pub trapno: u64,
    pub oldmask: u64,
    pub cr2: u64,
}

pub fn deliver(
    sig: u32,
    handler: u64,
    restorer: u64,
    oldmask: u64,
    cur_rip: u64,
    cur_rsp: u64,
    cur_rflags: u64,
) -> (u64, u64) {
    if restorer == 0 {
        return (cur_rip, cur_rsp);
    }
    let frame_size = core::mem::size_of::<RtSigFrame>();
    let new_rsp = (cur_rsp - frame_size as u64 - 128) & !15u64;

    let frame = RtSigFrame {
        pretcode: restorer,
        sig,
        _pad: 0,
        siginfo: [0u8; 128],
        uc_flags: 0,
        uc_link: 0,
        uc_stack: [0; 3],
        regs: SigRegs {
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rdi: sig as u64,
            rsi: 0,
            rbp: 0,
            rbx: 0,
            rdx: 0,
            rax: 0,
            rcx: cur_rip,
            rsp: cur_rsp,
            rip: cur_rip,
            eflags: cur_rflags,
            cs: crate::gdt::USER_CS,
            gs: 0,
            fs: 0,
            _pad: 0,
            err: 0,
            trapno: 0,
            oldmask,
            cr2: 0,
        },
        fpstate: 0,
        _pad2: [0u8; 64],
    };

    let frame_bytes = unsafe {
        core::slice::from_raw_parts(
            &frame as *const RtSigFrame as *const u8,
            core::mem::size_of::<RtSigFrame>(),
        )
    };

    for (i, &byte) in frame_bytes.iter().enumerate() {
        let addr = new_rsp + i as u64;
        if !paging::write_user(addr, byte) {
            crate::serial_println!("[signal] failed to write signal frame at {:#x}", addr);
            return (cur_rip, cur_rsp);
        }
    }

    (handler, new_rsp)
}

pub fn rt_sigreturn(rsp: u64) -> (u64, u64, u64, u64) {
    let frame = unsafe { &*(rsp as *const RtSigFrame) };
    (
        frame.regs.rip,
        frame.regs.rsp,
        frame.regs.eflags,
        frame.regs.oldmask,
    )
}
