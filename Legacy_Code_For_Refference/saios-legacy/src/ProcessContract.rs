//! Canonical process lifecycle authority.
//!
//! All process creation, execution, blocking, wakeup, zombie publication,
//! waiting, and destruction must migrate to this state machine.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractProcessState {
    New,
    Ready,
    Running,
    Blocked,
    Zombie,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessEvent {
    Create,
    Admit,
    Dispatch,
    Block,
    Wake,
    Yield,
    Exit,
    Reap,
    Destroy,
    FailCreate,
}

pub struct ProcessContract;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessEvidenceView {
    pub timestamp: u64,
    pub cpu: u32,
    pub subsystem: &'static str,
    pub event: &'static str,
    pub evidence: [u64; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessCreationKind {
    UserProcess,
    ForkChild,
    UserThread,
    KernelThread,
    IdleThread,
}

pub struct ProcessCreationRequest {
    pub name: alloc::string::String,
    pub parent_pid: u32,
    pub kind: ProcessCreationKind,
    pub tag: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForkRegisterImage {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessExitReason {
    SyscallExit,
    FatalSignal,
    ThreadExit,
    OomKill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExitRequest {
    pub pid: u32,
    pub code: i64,
    pub reason: ProcessExitReason,
    pub tag: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExitDisposition {
    pub pid: u32,
    pub parent_pid: u32,
    pub woke_waiters: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZombiePublication {
    pub pid: u32,
    pub parent_pid: u32,
    pub exit_code: i64,
    /// PML4 to destroy after TABLE is dropped (0 = none).
    /// Deferred because destroy_address_space acquires FRAME_ALLOCATOR
    /// and must never be called while the process table lock is held.
    pub pml4_to_destroy: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessWaitRequest {
    pub parent_pid: u32,
    pub waiter_pid: u32,
    pub want_pid: u32,
    pub options: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessWaitReap {
    pub child_pid: u32,
    pub exit_code: i64,
    pub status: u32,
    pub waiter_pml4: u64,
}

struct ChildWaiter {
    waiter_pid: u32,
    parent_pid: u32,
    want_pid: u32,
}

static CHILD_WAITERS: spin::Mutex<alloc::vec::Vec<ChildWaiter>> =
    spin::Mutex::new(alloc::vec::Vec::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityMutation {
    Uid = 1,
    Gid = 2,
    Reuid = 3,
    Regid = 4,
    Resuid = 5,
    Resgid = 6,
    Pgid = 7,
    Sid = 8,
}

impl ProcessContract {
    pub fn for_each_evidence_view(
        pid: u32,
        limit: usize,
        mut visit: impl FnMut(ProcessEvidenceView),
    ) {
        crate::kds::for_each_event(limit, |record| {
            if record.metadata.process_id == pid {
                visit(ProcessEvidenceView {
                    timestamp: record.metadata.timestamp,
                    cpu: record.metadata.cpu_id,
                    subsystem: crate::kds::subsystem_name(record.metadata.subsystem),
                    event: crate::kds::event_type_name(record.metadata.event_type),
                    evidence: record.payload,
                });
            }
        });
    }

    pub fn create(request: ProcessCreationRequest) -> crate::process::Process {
        crate::serial_println!("[process-contract] create start tag={}", request.tag);
        let pid = crate::process::alloc_pid();
        crate::serial_println!(
            "[process-contract] create pid allocated tag={} pid={}",
            request.tag,
            pid
        );
        crate::serial_println!(
            "[process-contract] create pcb begin tag={} pid={}",
            request.tag,
            pid
        );
        let mut proc = match request.kind {
            ProcessCreationKind::KernelThread | ProcessCreationKind::IdleThread => {
                crate::process::Process::new_kernel(pid, request.name)
            }
            ProcessCreationKind::UserProcess
            | ProcessCreationKind::ForkChild
            | ProcessCreationKind::UserThread => crate::process::Process::new(pid, request.name),
        };
        crate::serial_println!(
            "[process-contract] create pcb complete tag={} pid={}",
            request.tag,
            pid
        );
        proc.parent_pid = request.parent_pid;
        proc.set_contract_state(crate::process::ProcessState::New);
        crate::serial_println!(
            "[process-contract] create event begin tag={} pid={}",
            request.tag,
            pid
        );
        emit_process_kds_event(
            pid,
            crate::kds::KdsEventType::TaskCreate,
            crate::kds::KdsSeverity::Info,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                pid as u64,
                request.parent_pid as u64,
                request.kind as u64,
                request.tag.as_ptr() as u64,
            ],
        );
        crate::serial_println!(
            "[process-contract] create object begin tag={} pid={}",
            request.tag,
            pid
        );
        crate::observability_contract::ObservabilityContract::kds_object(
            match request.kind {
                ProcessCreationKind::UserProcess | ProcessCreationKind::ForkChild => {
                    crate::kds::KdsObjectKind::Process
                }
                ProcessCreationKind::UserThread
                | ProcessCreationKind::KernelThread
                | ProcessCreationKind::IdleThread => crate::kds::KdsObjectKind::Thread,
            },
            request.parent_pid as u64,
            [pid as u64, request.kind as u64],
        );
        crate::serial_println!(
            "[process-contract] create complete tag={} pid={}",
            request.tag,
            pid
        );
        proc
    }

    pub fn inherit_parent_metadata(
        child: &mut crate::process::Process,
        parent: &crate::process::Process,
        inherit_fds: bool,
        share_address_space: bool,
        tag: &'static str,
    ) {
        if parent.pid == 0 || child.pid == 0 {
            Self::dump_existing_process(child.pid, tag, "process: invalid inheritance pid");
            return;
        }
        child.parent_pid = parent.pid;
        child.cwd = parent.cwd.clone();
        child.namespace_view = parent.namespace_view;
        child.mount_namespace = parent.mount_namespace.clone();
        child.uid = parent.uid;
        child.gid = parent.gid;
        child.euid = parent.euid;
        child.egid = parent.egid;
        child.suid = parent.suid;
        child.sgid = parent.sgid;
        child.session_id = parent.session_id;
        child.pgid = parent.pgid;
        child.brk = parent.brk;
        child.mmap_base = parent.mmap_base;
        child.boot_cpu_affine = parent.boot_cpu_affine;
        child.scheduling = parent.scheduling;
        child.signals = parent.signals.clone();
        if inherit_fds {
            child.fd_table = clone_fd_table(&parent.fd_table);
        }
        if share_address_space {
            child.install_address_space(parent.address_space);
            child.owns_address_space = false;
        }
        emit_process_kds_event(
            child.pid,
            crate::kds::KdsEventType::TaskCreate,
            crate::kds::KdsSeverity::Trace,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                child.pid as u64,
                parent.pid as u64,
                inherit_fds as u64,
                share_address_space as u64,
            ],
        );
    }

    pub fn finalize_user_process_image(
        proc: &mut crate::process::Process,
        entry: u64,
        rsp: u64,
        tag: &'static str,
    ) {
        proc.rip = entry;
        proc.rsp = rsp;
        emit_process_kds_event(
            proc.pid,
            crate::kds::KdsEventType::State,
            crate::kds::KdsSeverity::Trace,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                proc.pid as u64,
                ProcessCreationKind::UserProcess as u64,
                entry,
                rsp,
            ],
        );
        let _ = tag;
    }

    pub fn finalize_fork_register_image(
        child: &mut crate::process::Process,
        image: ForkRegisterImage,
        tag: &'static str,
    ) {
        child.rip = image.rip;
        child.rsp = image.rsp;
        child.rflags = crate::process::sanitize_user_rflags(image.rflags);
        child.fork_rax = 0;
        child.fork_rdi = image.rdi;
        child.fork_rsi = image.rsi;
        child.fork_rdx = image.rdx;
        child.fork_r8 = image.r8;
        child.fork_r9 = image.r9;
        child.fork_r10 = image.r10;
        child.fork_rbx = image.rbx;
        child.fork_rbp = image.rbp;
        child.fork_r12 = image.r12;
        child.fork_r13 = image.r13;
        child.fork_r14 = image.r14;
        child.fork_r15 = image.r15;
        emit_process_kds_event(
            child.pid,
            crate::kds::KdsEventType::Fork,
            crate::kds::KdsSeverity::Trace,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [child.pid as u64, image.rip, image.rsp, child.fork_rax],
        );
        let _ = tag;
    }

    pub fn finalize_user_thread_context(
        child: &mut crate::process::Process,
        rip: u64,
        rsp: u64,
        rflags: u64,
        tls: Option<u64>,
        tag: &'static str,
    ) {
        child.rip = rip;
        child.rsp = rsp;
        child.rflags = crate::process::sanitize_user_rflags(rflags);
        child.fork_rax = 0;
        if let Some(tls) = tls {
            child.fs_base.fs_base = tls;
        }
        emit_process_kds_event(
            child.pid,
            crate::kds::KdsEventType::State,
            crate::kds::KdsSeverity::Trace,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                child.pid as u64,
                ProcessCreationKind::UserThread as u64,
                rip,
                rsp,
            ],
        );
        Self::validate_creation_ready_or_panic(ProcessCreationKind::UserThread, child, tag);
    }

    pub fn prepare_kernel_context(
        proc: &mut crate::process::Process,
        boot_cpu_affine: bool,
        tag: &'static str,
    ) {
        proc.clear_address_space();
        proc.boot_cpu_affine = boot_cpu_affine;
        if boot_cpu_affine {
            proc.scheduling.allowed_cpus = 1;
            proc.scheduling.preferred_cpu = Some(0);
        }
        emit_process_kds_event(
            proc.pid,
            crate::kds::KdsEventType::State,
            crate::kds::KdsSeverity::Trace,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                proc.pid as u64,
                ProcessCreationKind::KernelThread as u64,
                boot_cpu_affine as u64,
                0,
            ],
        );
        Self::validate_creation_ready_or_panic(ProcessCreationKind::KernelThread, proc, tag);
    }

    pub fn prepare_idle_context(proc: &mut crate::process::Process, tag: &'static str) {
        proc.clear_address_space();
        emit_process_kds_event(
            proc.pid,
            crate::kds::KdsEventType::State,
            crate::kds::KdsSeverity::Trace,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                proc.pid as u64,
                ProcessCreationKind::IdleThread as u64,
                0,
                0,
            ],
        );
        Self::validate_creation_ready_or_panic(ProcessCreationKind::IdleThread, proc, tag);
    }

    pub fn admit_detached(mut proc: crate::process::Process, tag: &'static str) {
        Self::admit_new_or_ready_process(&mut proc, tag);
        let mut table = crate::process::table::TABLE.lock();
        crate::scheduler_contract::SchedulerContract::insert_detached(&mut table, proc, tag);
    }

    pub fn admit_runnable(
        mut proc: crate::process::Process,
        reason: &'static str,
        caller: &'static str,
    ) {
        Self::admit_new_or_ready_process(&mut proc, reason);
        let mut table = crate::process::table::TABLE.lock();
        crate::scheduler_contract::SchedulerContract::enqueue_runnable(
            &mut table, proc, reason, caller,
        );
    }

    pub fn admit_running_current(
        mut proc: crate::process::Process,
        cpu: usize,
        make_idle: bool,
        tag: &'static str,
    ) -> u32 {
        crate::serial_println!(
            "[process-contract] admit running start tag={} pid={}",
            tag,
            proc.pid
        );
        Self::admit_new_or_ready_process(&mut proc, tag);
        crate::serial_println!(
            "[process-contract] admit running state-ready tag={} pid={}",
            tag,
            proc.pid
        );
        Self::validate_existing_transition_or_panic(
            proc.pid,
            proc.state(),
            &crate::process::ProcessState::Running,
            tag,
        );
        crate::serial_println!(
            "[process-contract] admit running transition-ok tag={} pid={}",
            tag,
            proc.pid
        );
        proc.set_contract_state(crate::process::ProcessState::Running);
        proc.set_contract_cpu_owner(Some(cpu), true);
        let pid = proc.pid;
        crate::serial_println!(
            "[process-contract] admit running table-lock begin tag={} pid={}",
            tag,
            pid
        );
        let mut table = crate::process::table::TABLE.lock();
        crate::serial_println!(
            "[process-contract] admit running table-lock acquired tag={} pid={}",
            tag,
            pid
        );
        table.procs.insert(pid, proc);
        crate::serial_println!(
            "[process-contract] admit running scheduler-register begin tag={} pid={}",
            tag,
            pid
        );
        crate::scheduler_contract::SchedulerContract::register_running_current(
            &mut table, cpu, pid, make_idle, tag,
        );
        crate::serial_println!(
            "[process-contract] admit running scheduler-register complete tag={} pid={}",
            tag,
            pid
        );
        pid
    }

    fn admit_new_or_ready_process(proc: &mut crate::process::Process, tag: &'static str) {
        match proc.state() {
            crate::process::ProcessState::New => {
                let _ = Self::transition(ContractProcessState::New, ProcessEvent::Admit)
                    .expect("process contract admits New -> Ready");
                proc.set_contract_state(crate::process::ProcessState::Ready);
            }
            crate::process::ProcessState::Ready => {}
            other => {
                crate::observability_contract::ObservabilityContract::contract_violation(
                    crate::observability_contract::ContractOwner::Process,
                    tag,
                    "process: invalid admission source state",
                    crate::observability_contract::ResourceClass::Process,
                    crate::observability_contract::ResourceOwner::Pid(proc.pid),
                    [
                        process_state_code(other) as u64,
                        process_state_code(&crate::process::ProcessState::Ready) as u64,
                        proc.pid as u64,
                        0,
                    ],
                );
                crate::serial_println!(
                    "[process-contract] {} admission violation pid={} from={:?}: process: invalid admission source state",
                    tag,
                    proc.pid,
                    other
                );
                Self::dump_existing_process(
                    proc.pid,
                    tag,
                    "process: invalid admission source state",
                );
                panic!(
                    "[process-contract] {} admission violation: process: invalid admission source state",
                    tag
                );
            }
        }
    }

    pub fn set_process_group(
        current_pid: u32,
        target_pid: u32,
        target_pgid: u32,
    ) -> Result<(), i64> {
        let mut table = crate::process::table::TABLE.lock();
        let current_session = table
            .procs
            .get(&current_pid)
            .map(|proc| proc.session_id)
            .unwrap_or(1);
        let Some(target) = table.procs.get_mut(&target_pid) else {
            return Err(-3);
        };
        if target.session_id != current_session || target.pid == target.session_id {
            return Err(-1);
        }
        let old = target.pgid;
        target.pgid = target_pgid;
        emit_identity_event(
            target.pid,
            IdentityMutation::Pgid,
            old as u64,
            target_pgid as u64,
        );
        Ok(())
    }

    pub fn validate_fork_child_or_panic(
        parent_pid: u32,
        child: &crate::process::Process,
        tag: &'static str,
    ) {
        let reason = if parent_pid == 0 || child.pid == 0 {
            Some("process: fork has invalid parent or child pid")
        } else if parent_pid == child.pid {
            Some("process: fork child reused parent pid")
        } else if child.fork_rax != 0 {
            Some("process: fork child rax must be zero")
        } else if child.rip == 0 || child.rsp == 0 {
            Some("process: fork child user context is incomplete")
        } else if child.address_space_pml4() == 0 {
            Some("process: fork child address space is missing")
        } else if child.is_on_cpu() {
            Some("process: fork child must not be on CPU before admission")
        } else if child.state() != &crate::process::ProcessState::New {
            Some("process: fork child must remain New before admission")
        } else {
            None
        };

        if let Some(reason) = reason {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Process,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Process,
                crate::observability_contract::ResourceOwner::Pid(child.pid),
                [
                    parent_pid as u64,
                    child.pid as u64,
                    child.fork_rax,
                    child.address_space_pml4(),
                ],
            );
            Self::dump_existing_process(child.pid, tag, reason);
            panic!("[process-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn validate_creation_ready_or_panic(
        kind: ProcessCreationKind,
        proc: &crate::process::Process,
        tag: &'static str,
    ) {
        let reason = if proc.pid == 0 {
            Some("process: creation target has invalid pid")
        } else if proc.state() != &crate::process::ProcessState::New {
            Some("process: creation target must remain New before admission")
        } else if proc.is_on_cpu() {
            Some("process: creation target must not be on CPU before admission")
        } else {
            match kind {
                ProcessCreationKind::UserProcess => {
                    if proc.address_space_pml4() == 0 {
                        Some("process: user process address space is missing")
                    } else if !proc.owns_address_space {
                        Some("process: user process must own its address space")
                    } else if proc.rip == 0 || proc.rsp == 0 {
                        Some("process: user process entry context is incomplete")
                    } else if proc.kernel_rsp == 0 {
                        Some("process: user process entry frame is missing")
                    } else if proc.stack_base == 0 || proc.stack_size == 0 {
                        Some("process: user process stack is missing")
                    } else {
                        None
                    }
                }
                ProcessCreationKind::ForkChild => {
                    if proc.address_space_pml4() == 0 {
                        Some("process: fork child address space is missing")
                    } else if proc.rip == 0 || proc.rsp == 0 {
                        Some("process: fork child user context is incomplete")
                    } else if proc.fork_rax != 0 {
                        Some("process: fork child rax must be zero")
                    } else if proc.kernel_rsp == 0 {
                        Some("process: fork child return frame is missing")
                    } else {
                        None
                    }
                }
                ProcessCreationKind::UserThread => {
                    if proc.address_space_pml4() == 0 {
                        Some("process: user thread address space is missing")
                    } else if proc.rip == 0 || proc.rsp == 0 {
                        Some("process: user thread context is incomplete")
                    } else if proc.fork_rax != 0 {
                        Some("process: user thread return value must be zero")
                    } else {
                        None
                    }
                }
                ProcessCreationKind::KernelThread | ProcessCreationKind::IdleThread => {
                    if proc.address_space_pml4() != 0 {
                        Some("process: kernel process must not own a user address space")
                    } else {
                        None
                    }
                }
            }
        };

        if let Some(reason) = reason {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Process,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Process,
                crate::observability_contract::ResourceOwner::Pid(proc.pid),
                [
                    proc.pid as u64,
                    kind as u64,
                    proc.rip,
                    proc.address_space_pml4(),
                ],
            );
            Self::dump_existing_process(proc.pid, tag, reason);
            panic!("[process-contract] {} violation: {}", tag, reason);
        }

        emit_process_kds_event(
            proc.pid,
            crate::kds::KdsEventType::State,
            crate::kds::KdsSeverity::Trace,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                proc.pid as u64,
                kind as u64,
                proc.rip,
                proc.address_space_pml4(),
            ],
        );
    }

    pub fn transition_existing_state(
        table: &mut crate::process::table::ProcessTable,
        pid: u32,
        next: crate::process::ProcessState,
        tag: &'static str,
    ) -> bool {
        let Some(current) = table.procs.get(&pid).map(|proc| proc.state().clone()) else {
            return false;
        };
        Self::validate_existing_transition_or_panic(pid, &current, &next, tag);
        table.set_state(pid, next)
    }

    pub fn block_current(
        table: &mut crate::process::table::ProcessTable,
        tag: &'static str,
    ) -> Option<u32> {
        let pid = table.current_pid();
        let current = table.procs.get(&pid).map(|proc| proc.state().clone())?;
        if !matches!(
            current,
            crate::process::ProcessState::Running | crate::process::ProcessState::Ready
        ) {
            Self::validate_existing_transition_or_panic(
                pid,
                &current,
                &crate::process::ProcessState::Blocked,
                tag,
            );
            return None;
        }
        crate::scheduler_contract::SchedulerContract::block_current(table, tag)
    }

    pub fn wake_pid(
        table: &mut crate::process::table::ProcessTable,
        pid: u32,
        tag: &'static str,
    ) -> bool {
        crate::serial_println!("[wake-pid] enter pid={} tag={}", pid, tag);
        let Some(current) = table.procs.get(&pid).map(|proc| proc.state().clone()) else {
            crate::serial_println!("[wake-pid] missing pid={} tag={}", pid, tag);
            return false;
        };
        crate::serial_println!("[wake-pid] state-before={:?}", current);
        if current != crate::process::ProcessState::Blocked {
            Self::validate_existing_transition_or_panic(
                pid,
                &current,
                &crate::process::ProcessState::Ready,
                tag,
            );
            crate::serial_println!(
                "[wake-pid] rejected pid={} state={:?} tag={}",
                pid,
                current,
                tag
            );
            return false;
        }
        let woke = crate::scheduler_contract::SchedulerContract::wake_pid(table, pid, tag);
        let state_after = table.procs.get(&pid).map(|proc| proc.state().clone());
        crate::serial_println!("[wake-pid] state-after={:?}", state_after);
        crate::serial_println!("[wake-pid] enqueue-complete pid={} woke={}", pid, woke);
        woke
    }

    pub fn create_session(pid: u32) -> Result<u32, i64> {
        let mut table = crate::process::table::TABLE.lock();
        if table.procs.values().any(|proc| proc.pgid == pid) {
            return Err(-1);
        }
        let Some(proc) = table.procs.get_mut(&pid) else {
            return Err(-3);
        };
        let old = pack_ids(proc.session_id, proc.pgid);
        proc.session_id = pid;
        proc.pgid = pid;
        emit_identity_event(pid, IdentityMutation::Sid, old, pack_ids(pid, pid));
        Ok(pid)
    }

    pub fn set_uid(pid: u32, uid: u32) -> Result<(), i64> {
        let mut table = crate::process::table::TABLE.lock();
        let Some(proc) = table.procs.get_mut(&pid) else {
            return Err(-3);
        };
        if proc.euid != 0 {
            return Err(-1);
        }
        let old = pack_ids3(proc.uid, proc.euid, proc.suid);
        proc.uid = uid;
        proc.euid = uid;
        proc.suid = uid;
        emit_identity_event(pid, IdentityMutation::Uid, old, pack_ids3(uid, uid, uid));
        Ok(())
    }

    pub fn set_gid(pid: u32, gid: u32) -> Result<(), i64> {
        let mut table = crate::process::table::TABLE.lock();
        let Some(proc) = table.procs.get_mut(&pid) else {
            return Err(-3);
        };
        if proc.euid != 0 {
            return Err(-1);
        }
        let old = pack_ids3(proc.gid, proc.egid, proc.sgid);
        proc.gid = gid;
        proc.egid = gid;
        proc.sgid = gid;
        emit_identity_event(pid, IdentityMutation::Gid, old, pack_ids3(gid, gid, gid));
        Ok(())
    }

    pub fn set_reuid(pid: u32, r_uid: u64, e_uid: u64) -> Result<(), i64> {
        let mut table = crate::process::table::TABLE.lock();
        let Some(proc) = table.procs.get_mut(&pid) else {
            return Err(-3);
        };
        let new_ruid = if r_uid == u64::MAX {
            proc.uid
        } else {
            r_uid as u32
        };
        let new_euid = if e_uid == u64::MAX {
            proc.euid
        } else {
            e_uid as u32
        };
        if proc.euid != 0 {
            let allowed = [proc.uid, proc.euid, proc.suid];
            if !allowed.contains(&new_ruid) || !allowed.contains(&new_euid) {
                return Err(-1);
            }
        }
        let old = pack_ids3(proc.uid, proc.euid, proc.suid);
        proc.uid = new_ruid;
        proc.euid = new_euid;
        if r_uid != u64::MAX || e_uid != u64::MAX {
            proc.suid = proc.euid;
        }
        emit_identity_event(
            pid,
            IdentityMutation::Reuid,
            old,
            pack_ids3(proc.uid, proc.euid, proc.suid),
        );
        Ok(())
    }

    pub fn set_regid(pid: u32, r_gid: u64, e_gid: u64) -> Result<(), i64> {
        let mut table = crate::process::table::TABLE.lock();
        let Some(proc) = table.procs.get_mut(&pid) else {
            return Err(-3);
        };
        let new_rgid = if r_gid == u64::MAX {
            proc.gid
        } else {
            r_gid as u32
        };
        let new_egid = if e_gid == u64::MAX {
            proc.egid
        } else {
            e_gid as u32
        };
        if proc.euid != 0 {
            let allowed = [proc.gid, proc.egid, proc.sgid];
            if !allowed.contains(&new_rgid) || !allowed.contains(&new_egid) {
                return Err(-1);
            }
        }
        let old = pack_ids3(proc.gid, proc.egid, proc.sgid);
        proc.gid = new_rgid;
        proc.egid = new_egid;
        if r_gid != u64::MAX || e_gid != u64::MAX {
            proc.sgid = proc.egid;
        }
        emit_identity_event(
            pid,
            IdentityMutation::Regid,
            old,
            pack_ids3(proc.gid, proc.egid, proc.sgid),
        );
        Ok(())
    }

    pub fn set_resuid(pid: u32, r_uid: u64, e_uid: u64, s_uid: u64) -> Result<(), i64> {
        let mut table = crate::process::table::TABLE.lock();
        let Some(proc) = table.procs.get_mut(&pid) else {
            return Err(-3);
        };
        let new_ruid = if r_uid == u64::MAX {
            proc.uid
        } else {
            r_uid as u32
        };
        let new_euid = if e_uid == u64::MAX {
            proc.euid
        } else {
            e_uid as u32
        };
        let new_suid = if s_uid == u64::MAX {
            proc.suid
        } else {
            s_uid as u32
        };
        if proc.euid != 0 {
            let allowed = [proc.uid, proc.euid, proc.suid];
            if !allowed.contains(&new_ruid)
                || !allowed.contains(&new_euid)
                || !allowed.contains(&new_suid)
            {
                return Err(-1);
            }
        }
        let old = pack_ids3(proc.uid, proc.euid, proc.suid);
        proc.uid = new_ruid;
        proc.euid = new_euid;
        proc.suid = new_suid;
        emit_identity_event(
            pid,
            IdentityMutation::Resuid,
            old,
            pack_ids3(proc.uid, proc.euid, proc.suid),
        );
        Ok(())
    }

    pub fn set_resgid(pid: u32, r_gid: u64, e_gid: u64, s_gid: u64) -> Result<(), i64> {
        let mut table = crate::process::table::TABLE.lock();
        let Some(proc) = table.procs.get_mut(&pid) else {
            return Err(-3);
        };
        let new_rgid = if r_gid == u64::MAX {
            proc.gid
        } else {
            r_gid as u32
        };
        let new_egid = if e_gid == u64::MAX {
            proc.egid
        } else {
            e_gid as u32
        };
        let new_sgid = if s_gid == u64::MAX {
            proc.sgid
        } else {
            s_gid as u32
        };
        if proc.euid != 0 {
            let allowed = [proc.gid, proc.egid, proc.sgid];
            if !allowed.contains(&new_rgid)
                || !allowed.contains(&new_egid)
                || !allowed.contains(&new_sgid)
            {
                return Err(-1);
            }
        }
        let old = pack_ids3(proc.gid, proc.egid, proc.sgid);
        proc.gid = new_rgid;
        proc.egid = new_egid;
        proc.sgid = new_sgid;
        emit_identity_event(
            pid,
            IdentityMutation::Resgid,
            old,
            pack_ids3(proc.gid, proc.egid, proc.sgid),
        );
        Ok(())
    }

    pub fn transition(
        from: ContractProcessState,
        event: ProcessEvent,
    ) -> Result<ContractProcessState, &'static str> {
        use ContractProcessState::*;
        use ProcessEvent::*;

        match (from, event) {
            (New, Admit) => Ok(Ready),
            (New, FailCreate) => Ok(Dead),
            (Ready, Dispatch) => Ok(Running),
            (Running, Yield) => Ok(Ready),
            (Running, Block) => Ok(Blocked),
            (Blocked, Wake) => Ok(Ready),
            (Running, Exit) => Ok(Zombie),
            (Zombie, Reap) => Ok(Dead),
            (Dead, Destroy) => Ok(Dead),
            (New, Create) => Ok(New),
            _ => Err("process: invalid lifecycle transition"),
        }
    }

    pub fn validate_existing_transition(
        from: &crate::process::ProcessState,
        to: &crate::process::ProcessState,
    ) -> Result<(), &'static str> {
        use crate::process::ProcessState;

        if from == to {
            return Ok(());
        }
        match (from, to) {
            (ProcessState::New, ProcessState::Ready)
            | (ProcessState::Ready, ProcessState::Running)
            | (ProcessState::Running, ProcessState::Ready)
            | (ProcessState::Running, ProcessState::Blocked)
            | (ProcessState::Ready, ProcessState::Blocked)
            | (ProcessState::Blocked, ProcessState::Ready)
            | (ProcessState::Running, ProcessState::Zombie)
            | (ProcessState::Zombie, ProcessState::Dead) => Ok(()),
            _ => Err("process: invalid existing ProcessState transition"),
        }
    }

    pub fn validate_existing_transition_or_panic(
        pid: u32,
        from: &crate::process::ProcessState,
        to: &crate::process::ProcessState,
        tag: &'static str,
    ) {
        if let Err(reason) = Self::validate_existing_transition(from, to) {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Process,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Process,
                crate::observability_contract::ResourceOwner::Pid(pid),
                [
                    process_state_code(from) as u64,
                    process_state_code(to) as u64,
                    pid as u64,
                    0,
                ],
            );
            crate::serial_println!(
                "[process-contract] {} violation pid={} from={:?} to={:?}: {}",
                tag,
                pid,
                from,
                to,
                reason
            );
            Self::dump_existing_process(pid, tag, reason);
            panic!("[process-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn request_exit(request: ProcessExitRequest) -> Option<ProcessExitDisposition> {
        if request.pid == 0 {
            Self::dump_exit_request(request, "process: exit request has no pid");
            return None;
        }
        crate::serial_println!(
            "[process-exit] request pid={} code={} reason={:?} tag={}",
            request.pid,
            request.code,
            request.reason,
            request.tag
        );

        let (parent_pid, immediate_publication) = {
            let mut table = crate::process::table::TABLE.lock();
            let parent_pid = table.procs.get(&request.pid).map(|proc| proc.parent_pid)?;
            if !table.mark_exiting(request.pid, request.code, request.tag) {
                Self::dump_exit_request(request, "process: failed to mark process exiting");
                return None;
            }
            let publish_now = table.procs.get(&request.pid).is_some_and(|proc| {
                !proc.is_on_cpu() && !table.pid_has_switch_publication_pending(request.pid)
            });
            let publication = if publish_now {
                Self::publish_zombie_after_switch(
                    &mut table,
                    request.pid,
                    "request_exit immediate publication",
                )
            } else {
                None
            };
            (parent_pid, publication)
        };

        if let Some(publication) = immediate_publication {
            return Some(Self::notify_zombie_publication(publication));
        }

        if crate::diag::diag_proc_on() {
            crate::serial_println!(
                "[process-contract] exit pid={} parent={} code={} reason={:?} tag={} deferred_waiter_wake=true",
                request.pid,
                parent_pid,
                request.code,
                request.reason,
                request.tag
            );
        }

        Some(ProcessExitDisposition {
            pid: request.pid,
            parent_pid,
            woke_waiters: 0,
        })
    }

    pub fn publish_zombie_after_switch(
        table: &mut crate::process::table::ProcessTable,
        pid: u32,
        tag: &'static str,
    ) -> Option<ZombiePublication> {
        crate::serial_println!("[zombie] publish-begin pid={} tag={}", pid, tag);
        let Some(proc) = table.procs.remove(&pid) else {
            crate::serial_println!("[zombie] publish-fail pid={} tag={} reason=procs_remove_returned_none procs_len={} zombies_len={}", pid, tag, table.procs.len(), table.zombies.len());
            Self::dump_existing_process(pid, tag, "process: missing zombie publication target");
            return None;
        };
        crate::serial_println!("[zombie] remove-ok pid={} tag={} procs_remaining={}", pid, tag, table.procs.len());

        Self::validate_existing_transition_or_panic(
            pid,
            &crate::process::ProcessState::Zombie,
            &crate::process::ProcessState::Dead,
            tag,
        );
        crate::serial_println!("[zombie] transition-validated pid={} Zombie->Dead", pid);

        let exit_code = proc.exit_code;
        let parent_pid = proc.parent_pid;
        let proc_name = proc.name.clone();

        // Switch CR3 away from the exiting process's address space if it is
        // still active.  This is safe with TABLE held (no FRAME_ALLOCATOR
        // acquisition).  The actual destroy is deferred to notify_zombie_publication
        // which runs after TABLE is dropped — destroy_address_space acquires
        // FRAME_ALLOCATOR and must never be called inside the TABLE critical section.
        let pml4_to_destroy = if proc.address_space_pml4() != 0 {
            if proc.address_space_pml4() == crate::memory::paging::active_pml4() {
                crate::serial_println!("[zombie] before-activate-as pid={} pml4={:#x}", pid, proc.address_space_pml4());
                crate::execution_contract::ExecutionContract::activate_process_address_space(
                    pid,
                    0,
                    crate::execution_contract::ExecutionTransition::ProcessExit,
                    tag,
                );
                crate::serial_println!("[zombie] after-activate-as pid={}", pid);
            }
            proc.address_space_pml4()
        } else {
            0
        };
        // proc is dropped here — its Drop does not destroy the address space.
        // The PML4 frames are freed later by notify_zombie_publication.

        crate::serial_println!("[zombie] before-zombies-push pid={}", pid);
        table.zombies.push(crate::process::table::ZombieEntry {
            pid,
            parent_pid,
            exit_code,
            exit_signal: if exit_code < 0 {
                (-exit_code) as u32
            } else {
                0
            },
            cpu: crate::process::table::cpu_idx() as u8,
        });
        crate::serial_println!("[zombie] after-zombies-push pid={} zombies_len={}", pid, table.zombies.len());
        crate::serial_println!("[zombie] before-remove-from-runq pid={}", pid);
        table.remove_from_run_queue(pid);
        crate::serial_println!("[zombie] after-remove-from-runq pid={}", pid);
        crate::serial_println!(
            "[zombie] published pid={} parent={} exit_code={} zombies={}",
            pid,
            parent_pid,
            exit_code,
            table.zombies.len()
        );

        if crate::diag::diag_proc_on() {
            crate::println!(
                "[sched] reap pid={} ({}) exit_code={} parent={}",
                pid,
                proc_name,
                exit_code,
                parent_pid
            );
            crate::serial_println!("[proc] pid={} state=Zombie exit_code={}", pid, exit_code);
        }

        Some(ZombiePublication {
            pid,
            parent_pid,
            exit_code,
            pml4_to_destroy,
        })
    }

    pub fn notify_zombie_publication(publication: ZombiePublication) -> ProcessExitDisposition {
        crate::serial_println!("[notify] enter parent={} child={} exit_code={} pml4_to_destroy={:#x}",
            publication.parent_pid, publication.pid, publication.exit_code, publication.pml4_to_destroy);
        // Deferred address-space destruction: publish_zombie_after_switch runs
        // with TABLE held, but destroy_address_space acquires FRAME_ALLOCATOR.
        // We are now outside the TABLE critical section — safe to destroy.
        if publication.pml4_to_destroy != 0 {
            crate::serial_println!("[notify] before-deferred-destroy-as pml4={:#x}", publication.pml4_to_destroy);
            let _ = crate::memory::paging::destroy_address_space(publication.pml4_to_destroy);
            crate::serial_println!("[notify] after-deferred-destroy-as");
        }
        crate::serial_println!("[notify] before-wake_child_waiters");
        let woke_waiters = Self::wake_child_waiters(publication.parent_pid, publication.pid);
        crate::serial_println!("[notify] after-wake_child_waiters woke={}", woke_waiters);
        crate::serial_println!("[notify] before-raise_sigchld");
        let _ = crate::ipc::signal::raise_signal_for_pid(
            publication.parent_pid,
            crate::ipc::signal::SIGCHLD,
        );
        crate::serial_println!("[notify] after-raise_sigchld");
        crate::serial_println!(
            "[parent-wakeup] parent={} child={} woke_waiters={}",
            publication.parent_pid,
            publication.pid,
            woke_waiters
        );
        if crate::diag::diag_proc_on() {
            crate::serial_println!(
                "[process-contract] published exit child={} parent={} code={} woke_waiters={}",
                publication.pid,
                publication.parent_pid,
                publication.exit_code,
                woke_waiters
            );
        }
        ProcessExitDisposition {
            pid: publication.pid,
            parent_pid: publication.parent_pid,
            woke_waiters,
        }
    }

    pub fn register_child_waiter(request: ProcessWaitRequest) {
        let mut waiters = CHILD_WAITERS.lock();
        if waiters
            .iter()
            .any(|waiter| waiter.waiter_pid == request.waiter_pid)
        {
            crate::serial_println!(
                "[waitpid] waiter-already-registered parent={} waiter={} want={}",
                request.parent_pid,
                request.waiter_pid,
                request.want_pid
            );
            return;
        }
        waiters.push(ChildWaiter {
            waiter_pid: request.waiter_pid,
            parent_pid: request.parent_pid,
            want_pid: request.want_pid,
        });
        crate::serial_println!(
            "[waitpid] waiter-registered parent={} waiter={} want={} total_waiters={}",
            request.parent_pid,
            request.waiter_pid,
            request.want_pid,
            waiters.len()
        );
    }

    pub fn unregister_child_waiter(waiter_pid: u32) {
        let mut waiters = CHILD_WAITERS.lock();
        waiters.retain(|waiter| waiter.waiter_pid != waiter_pid);
    }

    pub fn wake_child_waiters(parent_pid: u32, exited_pid: u32) -> usize {
        crate::serial_println!("[parent-wakeup] enter parent={} child={}", parent_pid, exited_pid);
        crate::serial_println!("[parent-wakeup] before-child-waiters-lock");
        let wake_pids = {
            let mut waiters = CHILD_WAITERS.lock();
            crate::serial_println!("[parent-wakeup] after-child-waiters-lock waiter-count={}", waiters.len());
            let mut wake_pids = alloc::vec::Vec::new();
            waiters.retain(|waiter| {
                let matches_parent = waiter.parent_pid == parent_pid;
                let matches_child = waiter.want_pid == 0
                    || waiter.want_pid == exited_pid
                    || waiter.want_pid as i32 == -1;
                let should_wake = matches_parent && matches_child;
                if should_wake {
                    wake_pids.push(waiter.waiter_pid);
                }
                !should_wake
            });
            crate::serial_println!(
                "[parent-wakeup] matched={} remaining={}",
                wake_pids.len(),
                waiters.len()
            );
            wake_pids
        };

        crate::serial_println!("[parent-wakeup] before-proc-table-lock");
        let mut proc_table = crate::process::table::TABLE.lock();
        crate::serial_println!("[parent-wakeup] after-proc-table-lock");
        let mut woke = 0;
        for pid in &wake_pids {
            crate::serial_println!("[parent-wakeup] waking pid={}", pid);
            if Self::wake_pid(&mut proc_table, *pid, "child waiter wake") {
                crate::serial_println!("[parent-wakeup] wake-call-complete pid={} ok=true", pid);
                crate::serial_println!(
                    "[scheduler-wakeup] child-exit parent={} child={} waiter={}",
                    parent_pid,
                    exited_pid,
                    pid
                );
                woke += 1;
            } else {
                crate::serial_println!("[parent-wakeup] wake-call-complete pid={} ok=false", pid);
            }
        }
        crate::serial_println!("[parent-wakeup] return woke={}", woke);
        woke
    }

    pub fn zombie_count() -> usize {
        crate::process::table::TABLE.lock().zombies.len()
    }

    /// Debug-assertion diagnostic: inspect waiter registration and zombie
    /// list for a specific parent/child pair without blocking on locks.
    /// Returns (waiter_present, zombie_present).
    pub fn waitpid_diagnostic(parent_pid: u32, child_pid: u32) -> (bool, bool) {
        let waiter_present = CHILD_WAITERS
            .try_lock()
            .map(|waiters| {
                waiters
                    .iter()
                    .any(|w| w.parent_pid == parent_pid && w.want_pid == child_pid)
            })
            .unwrap_or(false);
        let zombie_present = crate::process::table::TABLE
            .try_lock()
            .map(|table| {
                table
                    .zombies
                    .iter()
                    .any(|z| z.parent_pid == parent_pid && z.pid == child_pid)
            })
            .unwrap_or(false);
        (waiter_present, zombie_present)
    }

    pub fn try_reap_waitable(request: ProcessWaitRequest) -> Option<ProcessWaitReap> {
        let mut table = crate::process::table::TABLE.lock();
        if crate::diag::diag_proc_on() {
            if let Some((child_pid, child)) =
                table.find_waitable_child(request.parent_pid, request.want_pid)
            {
                crate::println!("wait4-debug: wait4: child found pid={}", child_pid);
                crate::println!("wait4-debug: wait4: child state={:?}", child.state());
                crate::println!(
                    "wait4-debug: wait4: child exited={} exit_code={}",
                    matches!(child.state(), crate::process::ProcessState::Zombie),
                    child.exit_code
                );
            }
            if let Some((child_pid, exit_code)) =
                table.find_waitable_zombie(request.parent_pid, request.want_pid)
            {
                crate::println!(
                    "wait4-zombie-found: parent={} child={}",
                    request.parent_pid,
                    child_pid
                );
                crate::println!("wait4-debug: wait4: child found pid={}", child_pid);
                crate::println!(
                    "wait4-debug: wait4: child state={:?} exit_code={}",
                    crate::process::ProcessState::Zombie,
                    exit_code
                );
                crate::println!(
                    "wait4-debug: wait4: child exited={} exit_code={}",
                    true,
                    exit_code
                );
            } else {
                crate::println!(
                    "wait4-zombie-not-found: parent={} child={}",
                    request.parent_pid,
                    request.want_pid
                );
            }
        }

        let Some((child_pid, exit_code)) = table.pop_zombie(request.parent_pid, request.want_pid) else {
            crate::serial_println!(
                "[waitpid] reap-miss parent={} waiter={} want={} zombies={}",
                request.parent_pid,
                request.waiter_pid,
                request.want_pid,
                table.zombies.len()
            );
            return None;
        };
        crate::serial_println!(
            "[reap] parent={} waiter={} child={} exit_code={} remaining_zombies={}",
            request.parent_pid,
            request.waiter_pid,
            child_pid,
            exit_code,
            table.zombies.len()
        );
        let waiter_pml4 = table
            .procs
            .get(&request.waiter_pid)
            .map(|proc| proc.address_space_pml4())
            .unwrap_or(0);
        drop(table);
        Self::unregister_child_waiter(request.waiter_pid);

        Some(ProcessWaitReap {
            child_pid,
            exit_code,
            status: ((exit_code as u32) & 0xFF) << 8,
            waiter_pml4,
        })
    }

    pub fn block_registered_child_waiter(waiter_pid: u32) -> bool {
        crate::process::validate_shadow("wait4 block before schedule");
        let should_schedule = {
            let waiters = CHILD_WAITERS.lock();
            if !waiters.iter().any(|waiter| waiter.waiter_pid == waiter_pid) {
                false
            } else {
                let mut table = crate::process::table::TABLE.lock();
                Self::block_current(&mut table, "wait4 block registered child waiter").is_some()
            }
        };

        if !should_schedule {
            crate::serial_println!("[waitpid] block-skip waiter={}", waiter_pid);
            return false;
        }

        crate::serial_println!("[waitpid] block waiter={}", waiter_pid);
        crate::process::scheduler::schedule_blocking_from("wait4_block");
        let waiter_kstack = {
            let mut table = crate::process::table::TABLE.lock();
            let Some(kstack) = table.restore_running_after_blocked_schedule(waiter_pid) else {
                crate::serial_println!("[waitpid] resume-missing waiter={}", waiter_pid);
                return false;
            };
            kstack
        };
        if waiter_kstack != 0 {
            crate::syscall::set_kernel_stack(waiter_kstack);
        }
        let _ = crate::process::refresh_current_from_pid(waiter_pid);
        crate::process::validate_shadow("wait4 block after schedule");
        crate::serial_println!("[waitpid] resumed waiter={}", waiter_pid);
        true
    }

    pub fn record_wait_success(request: ProcessWaitRequest, reap: ProcessWaitReap) {
        emit_process_kds_event(
            request.parent_pid,
            crate::kds::KdsEventType::Wait,
            crate::kds::KdsSeverity::Info,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                request.parent_pid as u64,
                reap.child_pid as u64,
                reap.exit_code as u64,
                reap.status as u64,
            ],
        );
    }

    pub fn record_wait_nohang(request: ProcessWaitRequest) {
        emit_process_kds_event(
            request.parent_pid,
            crate::kds::KdsEventType::Wait,
            crate::kds::KdsSeverity::Trace,
            crate::observability_contract::ObservationOutcome::Success,
            "",
            [
                request.parent_pid as u64,
                request.want_pid as u64,
                0,
                request.options,
            ],
        );
    }

    pub fn record_wait_interrupted(request: ProcessWaitRequest, errno: i64) {
        Self::unregister_child_waiter(request.waiter_pid);
        emit_process_kds_event(
            request.parent_pid,
            crate::kds::KdsEventType::Wait,
            crate::kds::KdsSeverity::Warn,
            crate::observability_contract::ObservationOutcome::Degraded,
            "process wait interrupted",
            [
                request.parent_pid as u64,
                request.want_pid as u64,
                errno as u64,
                request.options,
            ],
        );
    }

    fn dump_exit_request(request: ProcessExitRequest, reason: &'static str) {
        crate::observability_contract::ObservabilityContract::contract_violation(
            crate::observability_contract::ContractOwner::Process,
            request.tag,
            reason,
            crate::observability_contract::ResourceClass::Process,
            crate::observability_contract::ResourceOwner::Pid(request.pid),
            [
                request.pid as u64,
                request.code as u64,
                request.reason as u64,
                0,
            ],
        );
        crate::serial_println!(
            "[process-contract] exit request violation tag={} reason={} pid={} code={} exit_reason={:?}",
            request.tag,
            reason,
            request.pid,
            request.code,
            request.reason
        );
        Self::dump_existing_process(request.pid, request.tag, reason);
    }

    fn dump_existing_process(pid: u32, tag: &'static str, reason: &'static str) {
        let Some(table) = crate::process::table::TABLE.try_lock() else {
            crate::serial_println!(
                "[process-contract] dump tag={} reason={} pid={} process table locked by caller",
                tag,
                reason,
                pid
            );
            return;
        };
        let snapshot = table.scheduler_snapshot();
        crate::serial_println!(
            "[process-contract] dump tag={} reason={} cpu={} current={:?} run_queue={:?}",
            tag,
            reason,
            crate::process::table::cpu_idx(),
            snapshot.current,
            snapshot.run_queue
        );
        if let Some(proc) = table.procs.get(&pid) {
            crate::serial_println!(
                "[process-contract] proc pid={} ppid={} name={} state={:?} on_cpu={} cpu={:?} rip={:#x} rsp={:#x} ktop={:#x} krsp={:#x} pml4={:#x}",
                proc.pid,
                proc.parent_pid,
                proc.name.as_str(),
                proc.state(),
                proc.is_on_cpu(),
                proc.cpu_owner(),
                proc.rip,
                proc.rsp,
                proc.kernel_stack_top(),
                proc.kernel_rsp,
                proc.address_space_pml4()
            );
        } else {
            crate::serial_println!(
                "[process-contract] pid={} not present in process table",
                pid
            );
        }
    }
}

fn emit_identity_event(pid: u32, mutation: IdentityMutation, old: u64, new: u64) {
    emit_process_kds_event(
        pid,
        crate::kds::KdsEventType::State,
        crate::kds::KdsSeverity::Info,
        crate::observability_contract::ObservationOutcome::Success,
        "process.credential",
        [pid as u64, mutation as u64, old, new],
    );
}

fn process_event_name(event_type: crate::kds::KdsEventType, reason: &'static str) -> &'static str {
    if !reason.is_empty() {
        return reason;
    }
    match event_type {
        crate::kds::KdsEventType::TaskCreate | crate::kds::KdsEventType::Fork => "process.create",
        crate::kds::KdsEventType::TaskExit | crate::kds::KdsEventType::Exit => "process.exit",
        crate::kds::KdsEventType::Wait => "process.reap",
        crate::kds::KdsEventType::TaskBlock | crate::kds::KdsEventType::TaskUnblock => {
            "process.state"
        }
        _ => "process.state",
    }
}

fn emit_process_kds_event(
    pid: u32,
    event_type: crate::kds::KdsEventType,
    severity: crate::kds::KdsSeverity,
    outcome: crate::observability_contract::ObservationOutcome,
    reason: &'static str,
    evidence: [u64; 4],
) {
    crate::observability_contract::ObservabilityContract::emit_as_kds_event(
        crate::observability_contract::EventRecord {
            event: crate::observability_contract::ObservableEvent::Transition,
            contract: crate::observability_contract::ContractId::Process,
            tag: crate::observability_contract::ObservationTag::Transition,
            reason: process_event_name(event_type, reason),
            outcome,
            resource: crate::observability_contract::ResourceClass::Process,
            owner: crate::observability_contract::ResourceOwner::Pid(pid),
            cpu: Some(crate::process::table::cpu_idx()),
            pid: Some(pid),
            correlation_id:
                crate::observability_contract::ObservabilityContract::current_correlation_id(),
            evidence,
        },
        event_type,
        severity,
    );
}

fn pack_ids(a: u32, b: u32) -> u64 {
    ((a as u64) << 32) | b as u64
}

fn pack_ids3(a: u32, b: u32, c: u32) -> u64 {
    ((a as u64 & 0xFFFFF) << 44) | ((b as u64 & 0xFFFFF) << 22) | (c as u64 & 0x3FFFFF)
}

fn clone_fd_table(src: &crate::vfs::file::FdTable) -> crate::vfs::file::FdTable {
    let mut dst = crate::vfs::file::FdTable::new();
    for fd in 0..64usize {
        if let Ok(file) = src.get(fd) {
            dst.insert_at(fd, file);
        }
    }
    dst
}

fn process_state_code(state: &crate::process::ProcessState) -> u8 {
    match state {
        crate::process::ProcessState::Ready => 1,
        crate::process::ProcessState::Running => 2,
        crate::process::ProcessState::Blocked => 3,
        crate::process::ProcessState::Zombie => 4,
        crate::process::ProcessState::New => 5,
        crate::process::ProcessState::Dead => 6,
    }
}
