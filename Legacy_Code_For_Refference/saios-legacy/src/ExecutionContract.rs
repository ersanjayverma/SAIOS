//! Canonical execution-state authority.
//!
//! This contract is the migration target for current process, current CPU,
//! PID ownership view, kernel stack/RSP, user return image, active address
//! space, TSS.RSP0, and GS state. During migration, existing process,
//! scheduler, syscall, and interrupt code should be moved behind these APIs
//! one transition at a time.

use crate::process::{KERNEL_STACK_SIZE, USER_STACK_SIZE, USER_STACK_TOP};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTransition {
    Fork,
    Execve,
    Schedule,
    SwitchTo,
    SyscallEntry,
    SyscallExit,
    IretqReturn,
    ProcessExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserReturnOrigin {
    Direct,
    Interrupt,
    Syscall,
    ForkChild,
    Execve,
    SignalReturn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserContext {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub fs_base: u64,
    pub gs_base: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionSnapshot {
    pub cpu: usize,
    pub pid: u32,
    pub kernel_stack_top: u64,
    pub kernel_rsp: u64,
    pub pml4: u64,
    pub tss_rsp0: u64,
    pub kernel_gs_active: bool,
    pub user: UserContext,
}

pub struct ExecutionContract;

impl ExecutionContract {
    fn emit_execution_event(reason: &'static str, pid: u32, evidence: [u64; 4]) {
        crate::observability_contract::ObservabilityContract::emit(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::Execution,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason,
                outcome: crate::observability_contract::ObservationOutcome::Success,
                resource: crate::observability_contract::ResourceClass::Process,
                owner: crate::observability_contract::ResourceOwner::Pid(pid),
                cpu: Some(crate::process::table::cpu_idx()),
                pid: Some(pid),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence,
            },
        );
    }

    pub fn install_scheduled_process(
        transition: ExecutionTransition,
        pid: u32,
        kernel_stack_top: u64,
        kernel_rsp: u64,
        pml4: u64,
        tag: &'static str,
    ) {
        if kernel_stack_top != 0 {
            crate::syscall::set_kernel_stack(kernel_stack_top);
        }
        Self::activate_process_address_space(pid, pml4, transition, tag);
        Self::validate_scheduled_process_or_panic(
            transition,
            pid,
            kernel_stack_top,
            kernel_rsp,
            pml4,
            tag,
        );
        Self::emit_execution_event(
            "execution.current.install",
            pid,
            [kernel_stack_top, kernel_rsp, pml4, transition as u64],
        );
    }

    pub fn activate_process_address_space(
        pid: u32,
        pml4: u64,
        transition: ExecutionTransition,
        tag: &'static str,
    ) {
        if pml4 == 0 {
            return;
        }
        let handle = crate::address_space_contract::AddressSpaceHandle {
            id: pml4,
            pml4,
            owner_pid: pid,
        };
        crate::address_space_contract::AddressSpaceContract::validate_handle_or_panic(handle, tag);
        crate::memory::paging::switch_address_space(pml4);
        if crate::memory::paging::active_pml4() != pml4 {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Execution,
                tag,
                "execution: CR3 activation did not install requested address space",
                crate::observability_contract::ResourceClass::AddressSpace,
                crate::observability_contract::ResourceOwner::Pid(pid),
                [
                    pml4,
                    transition as u64,
                    crate::memory::paging::active_pml4(),
                    0,
                ],
            );
            panic!("[execution-contract] {} CR3 activation failed", tag);
        }
        Self::emit_execution_event(
            "execution.cr3.switch",
            pid,
            [
                pml4,
                transition as u64,
                crate::memory::paging::active_pml4(),
                0,
            ],
        );
    }

    pub fn validate_transition(
        _transition: ExecutionTransition,
        snapshot: &ExecutionSnapshot,
    ) -> Result<(), &'static str> {
        if snapshot.pid == 0 {
            return Err("execution: pid is not canonical");
        }
        if snapshot.kernel_stack_top == 0 || snapshot.kernel_rsp == 0 {
            return Err("execution: kernel stack is not installed");
        }
        if snapshot.pml4 == 0 {
            return Err("execution: address space is not installed");
        }
        if snapshot.tss_rsp0 != snapshot.kernel_stack_top {
            return Err("execution: TSS.RSP0 does not mirror current kernel stack");
        }
        Ok(())
    }

    pub fn validate_scheduled_process(
        transition: ExecutionTransition,
        pid: u32,
        kernel_stack_top: u64,
        kernel_rsp: u64,
        pml4: u64,
    ) -> Result<(), &'static str> {
        if pid == 0 {
            return Err("execution: scheduled pid is empty");
        }
        if kernel_stack_top == 0 {
            return Err("execution: scheduled kernel stack is invalid");
        }
        if kernel_rsp == 0 || kernel_rsp & 0x7 != 0 {
            return Err("execution: scheduled kernel RSP is invalid");
        }
        if pml4 != 0 {
            let kernel_stack_bottom = kernel_stack_top.saturating_sub(KERNEL_STACK_SIZE as u64);
            if kernel_rsp < kernel_stack_bottom || kernel_rsp >= kernel_stack_top {
                return Err("execution: scheduled kernel RSP is outside kernel stack");
            }
        }
        if pml4 != 0 && pml4 & 0xFFF != 0 {
            return Err("execution: scheduled PML4 is not frame aligned");
        }

        if matches!(
            transition,
            ExecutionTransition::SwitchTo | ExecutionTransition::Schedule
        ) && pml4 != 0
            && crate::memory::paging::active_pml4() != pml4
        {
            return Err("execution: active CR3 does not match scheduled process");
        }
        Ok(())
    }

    pub fn validate_scheduled_process_or_panic(
        transition: ExecutionTransition,
        pid: u32,
        kernel_stack_top: u64,
        kernel_rsp: u64,
        pml4: u64,
        tag: &'static str,
    ) {
        if let Err(reason) =
            Self::validate_scheduled_process(transition, pid, kernel_stack_top, kernel_rsp, pml4)
        {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Execution,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Process,
                crate::observability_contract::ResourceOwner::Pid(pid),
                [kernel_stack_top, kernel_rsp, pml4, transition as u64],
            );
            crate::serial_println!(
                "[execution-contract] {} violation pid={} transition={:?} kstack={:#x} kernel_rsp={:#x} pml4={:#x}: {}",
                tag,
                pid,
                transition,
                kernel_stack_top,
                kernel_rsp,
                pml4,
                reason
            );
            Self::dump_execution_state(tag, pid, kernel_stack_top, kernel_rsp, pml4, reason);
            panic!("[execution-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn dump_execution_state(
        tag: &'static str,
        pid: u32,
        kernel_stack_top: u64,
        kernel_rsp: u64,
        pml4: u64,
        reason: &'static str,
    ) {
        crate::serial_println!(
            "[execution-contract] dump tag={} reason={} cpu={} pid={} cr3={:#x} expected_pml4={:#x} ktop={:#x} krsp={:#x} kernel_gs_active={}",
            tag,
            reason,
            crate::process::table::cpu_idx(),
            pid,
            crate::memory::paging::active_pml4(),
            pml4,
            kernel_stack_top,
            kernel_rsp,
            crate::arch::syscall::kernel_gs_active()
        );
        let (saved_rip, saved_rsp, saved_rflags) = crate::arch::syscall::saved_user_syscall_site();
        crate::serial_println!(
            "[execution-contract] syscall-site user_rsp={:#x} user_rip={:#x} user_rflags={:#x}",
            saved_rsp,
            saved_rip,
            saved_rflags
        );
    }

    pub fn dump_user_return(
        tag: &'static str,
        pid: u32,
        rip: u64,
        rsp: u64,
        rflags: u64,
        fs_base: u64,
        gs_base: u64,
    ) {
        crate::serial_println!(
            "[execution-contract] return {} pid={} cpu={} rip={:#x} rsp={:#x} rflags={:#x} fs={:#x} gs={:#x} cr3={:#x} kernel_gs_active={}",
            tag,
            pid,
            crate::process::table::cpu_idx(),
            rip,
            rsp,
            rflags,
            fs_base,
            gs_base,
            crate::memory::paging::active_pml4(),
            crate::arch::syscall::kernel_gs_active()
        );
    }

    pub fn validate_user_return(
        origin: UserReturnOrigin,
        context: &UserContext,
    ) -> Result<(), &'static str> {
        if context.rip == 0 || context.rsp == 0 {
            return Err("execution: user return frame is incomplete");
        }
        if is_user_stack_address(context.rip) {
            return Err("execution: user return RIP points into user stack");
        }
        if context.rflags & 0x2 == 0 {
            return Err("execution: reserved RFLAGS bit must be set");
        }
        if context.rflags & (1 << 8) != 0 {
            return Err("execution: user return has Trap Flag set without debugger owner");
        }
        let pid = crate::process::current_pid().unwrap_or(0);
        Self::emit_execution_event(
            "execution.user_return",
            pid,
            [context.rip, context.rsp, context.rflags, origin as u64],
        );
        Self::emit_execution_event(
            "execution.gs.transition",
            pid,
            [context.fs_base, context.gs_base, origin as u64, 0],
        );
        Ok(())
    }
}

fn is_user_stack_address(addr: u64) -> bool {
    let stack_bottom = USER_STACK_TOP.saturating_sub(USER_STACK_SIZE as u64);
    addr >= stack_bottom && addr < USER_STACK_TOP
}
