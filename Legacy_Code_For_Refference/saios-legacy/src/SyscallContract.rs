//! Canonical syscall lifecycle authority.
//!
//! Every syscall must follow entry, validation, dispatch, and return through
//! this contract. Per-CPU syscall state and syscall GS activity belong here.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallStage {
    Entry,
    Validation,
    Dispatch,
    Return,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallOutcome {
    Return(i64),
    Execve(crate::execution_contract::UserContext),
    Exit(i64),
    SignalReturn,
    Schedule,
}

pub struct SyscallContract;

impl SyscallContract {
    fn emit_syscall_event(
        stage: SyscallStage,
        outcome: crate::observability_contract::ObservationOutcome,
        reason: &'static str,
        evidence: [u64; 4],
    ) {
        crate::observability_contract::ObservabilityContract::emit_as_kds_event(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::Syscall,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason,
                outcome,
                resource: crate::observability_contract::ResourceClass::Syscall,
                owner: crate::observability_contract::ObservabilityContract::current_pid_owner(),
                cpu: Some(crate::process::table::cpu_idx()),
                pid: crate::process::current_pid(),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence: [stage as u64, evidence[0], evidence[1], evidence[2]],
            },
            match outcome {
                crate::observability_contract::ObservationOutcome::Denied => {
                    crate::kds::KdsEventType::CompatibilityFailure
                }
                _ => crate::kds::KdsEventType::State,
            },
            match outcome {
                crate::observability_contract::ObservationOutcome::Success => {
                    crate::kds::KdsSeverity::Trace
                }
                crate::observability_contract::ObservationOutcome::Denied => {
                    crate::kds::KdsSeverity::Warn
                }
                _ => crate::kds::KdsSeverity::Info,
            },
        );
    }

    pub fn observe_entry(num: u64, user_rip: u64) {
        Self::emit_syscall_event(
            SyscallStage::Entry,
            crate::observability_contract::ObservationOutcome::Success,
            "syscall.entry",
            [num, user_rip, 0, 0],
        );
    }

    pub fn observe_dispatch(num: u64, a: u64, b: u64) {
        Self::emit_syscall_event(
            SyscallStage::Dispatch,
            crate::observability_contract::ObservationOutcome::Success,
            "syscall.dispatch",
            [num, a, b, 0],
        );
    }

    pub fn observe_exit(num: u64, ret: i64) {
        Self::emit_syscall_event(
            SyscallStage::Return,
            crate::observability_contract::ObservationOutcome::Success,
            "syscall.exit",
            [num, ret as u64, 0, 0],
        );
        Self::emit_syscall_event(
            SyscallStage::Return,
            crate::observability_contract::ObservationOutcome::Success,
            "syscall.outcome",
            [num, ret as u64, 0, 0],
        );
    }

    pub fn observe_denied(num: u64, ret: i64) {
        Self::emit_syscall_event(
            SyscallStage::Return,
            crate::observability_contract::ObservationOutcome::Denied,
            "syscall.denied",
            [num, ret as u64, 0, 0],
        );
    }

    pub fn validate_stage(stage: SyscallStage) -> Result<(), &'static str> {
        match stage {
            SyscallStage::Entry
            | SyscallStage::Validation
            | SyscallStage::Dispatch
            | SyscallStage::Return => Ok(()),
        }
    }

    pub fn validate_stage_or_panic(stage: SyscallStage, tag: &'static str) {
        if let Err(reason) = Self::validate_stage(stage) {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Syscall,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Syscall,
                crate::observability_contract::ObservabilityContract::current_pid_owner(),
                [stage as u64, 0, 0, 0],
            );
            Self::dump_stage(stage, tag, reason);
            panic!("[syscall-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn dump_stage(stage: SyscallStage, tag: &'static str, reason: &'static str) {
        let (user_rip, user_rsp, user_rflags) = crate::arch::syscall::saved_user_syscall_site();
        crate::serial_println!(
            "[syscall-contract] dump tag={} reason={} stage={:?} cpu={} current_pid={:?} cr3={:#x} kernel_gs_active={} user_rip={:#x} user_rsp={:#x} user_rflags={:#x}",
            tag,
            reason,
            stage,
            crate::process::table::cpu_idx(),
            crate::process::current_pid(),
            crate::memory::paging::active_pml4(),
            crate::arch::syscall::kernel_gs_active(),
            user_rip,
            user_rsp,
            user_rflags
        );
    }

    pub fn execve_outcome(
        pid: u32,
        path_ptr: u64,
        context: crate::execution_contract::UserContext,
    ) -> Result<SyscallOutcome, &'static str> {
        crate::execution_contract::ExecutionContract::validate_user_return(
            crate::execution_contract::UserReturnOrigin::Execve,
            &context,
        )?;
        crate::observability_contract::ObservabilityContract::emit_as_kds_event(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::Process,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason: "",
                outcome: crate::observability_contract::ObservationOutcome::Success,
                resource: crate::observability_contract::ResourceClass::Process,
                owner: crate::observability_contract::ResourceOwner::Pid(pid),
                cpu: Some(crate::process::table::cpu_idx()),
                pid: Some(pid),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence: [pid as u64, context.rip, context.rsp, path_ptr],
            },
            crate::kds::KdsEventType::Execve,
            crate::kds::KdsSeverity::Info,
        );
        Ok(SyscallOutcome::Execve(context))
    }
}
