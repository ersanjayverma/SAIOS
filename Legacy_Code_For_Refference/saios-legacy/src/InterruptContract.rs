//! Canonical interrupt and exception entry authority.
//!
//! IDT handlers should decode hardware-specific frames and immediately enter
//! this contract for user/kernel classification, fault disposition, EOI, and
//! scheduler handoff.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptKind {
    PageFault,
    GeneralProtectionFault,
    DoubleFault,
    Syscall,
    Timer,
    Device,
    Other(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultDisposition {
    RecoverCow,
    GrowStack,
    DeliverSignal(u32),
    TerminateProcess(u32),
    KernelPanic,
}

pub struct InterruptContract;

impl InterruptContract {
    fn emit_interrupt_event(
        reason: &'static str,
        outcome: crate::observability_contract::ObservationOutcome,
        kind: InterruptKind,
        evidence: [u64; 4],
    ) {
        crate::observability_contract::ObservabilityContract::emit_as_kds_event(
            crate::observability_contract::EventRecord {
                event: crate::observability_contract::ObservableEvent::Transition,
                contract: crate::observability_contract::ContractId::Interrupt,
                tag: crate::observability_contract::ObservationTag::Transition,
                reason,
                outcome,
                resource: crate::observability_contract::ResourceClass::Interrupt,
                owner: crate::observability_contract::ResourceOwner::Cpu(
                    crate::process::table::cpu_idx(),
                ),
                cpu: Some(crate::process::table::cpu_idx()),
                pid: current_pid_snapshot(),
                correlation_id:
                    crate::observability_contract::ObservabilityContract::current_correlation_id(),
                evidence: [
                    interrupt_kind_code(kind) as u64,
                    evidence[0],
                    evidence[1],
                    evidence[2],
                ],
            },
            match reason {
                "irq.eoi" => crate::kds::KdsEventType::InterruptExit,
                "fault.classify" | "fault.recover" | "fault.terminate" => {
                    crate::kds::KdsEventType::Fault
                }
                _ => crate::kds::KdsEventType::InterruptEnter,
            },
            match outcome {
                crate::observability_contract::ObservationOutcome::Success => {
                    crate::kds::KdsSeverity::Trace
                }
                crate::observability_contract::ObservationOutcome::Faulted => {
                    crate::kds::KdsSeverity::Warn
                }
                crate::observability_contract::ObservationOutcome::Failed => {
                    crate::kds::KdsSeverity::Error
                }
                _ => crate::kds::KdsSeverity::Info,
            },
        );
    }

    pub fn record_irq_entry(vector: u8) {
        irq_storm_count(vector);
        irq_entry_timestamp_store();
        Self::emit_interrupt_event(
            "irq.entry",
            crate::observability_contract::ObservationOutcome::Success,
            InterruptKind::Other(vector),
            [vector as u64, 0, 0, 0],
        );
    }

    pub fn record_irq_eoi(irq: u8) {
        let handler_ns = irq_handler_elapsed_ns();
        Self::emit_interrupt_event(
            "irq.eoi",
            crate::observability_contract::ObservationOutcome::Success,
            InterruptKind::Other(irq),
            [irq as u64, handler_ns, 0, 0],
        );
    }

    pub fn record_fault_recover(kind: InterruptKind, evidence: [u64; 4]) {
        Self::emit_interrupt_event(
            "fault.recover",
            crate::observability_contract::ObservationOutcome::Success,
            kind,
            evidence,
        );
    }

    pub fn record_fault_terminate(kind: InterruptKind, evidence: [u64; 4]) {
        Self::emit_interrupt_event(
            "fault.terminate",
            crate::observability_contract::ObservationOutcome::Faulted,
            kind,
            evidence,
        );
    }

    pub fn validate_kind(_kind: InterruptKind) -> Result<(), &'static str> {
        Ok(())
    }

    pub fn validate_kind_or_panic(kind: InterruptKind, tag: &'static str) {
        if let Err(reason) = Self::validate_kind(kind) {
            crate::observability_contract::ObservabilityContract::contract_violation(
                crate::observability_contract::ContractOwner::Interrupt,
                tag,
                reason,
                crate::observability_contract::ResourceClass::Interrupt,
                crate::observability_contract::ResourceOwner::Cpu(crate::process::table::cpu_idx()),
                [interrupt_kind_code(kind) as u64, 0, 0, 0],
            );
            Self::dump_interrupt(kind, tag, reason);
            panic!("[interrupt-contract] {} violation: {}", tag, reason);
        }
    }

    pub fn dump_interrupt(kind: InterruptKind, tag: &'static str, reason: &'static str) {
        crate::serial_println!(
            "[interrupt-contract] dump tag={} reason={} kind={:?} cpu={} current_pid={:?} cr3={:#x} kernel_gs_active={}",
            tag,
            reason,
            kind,
            crate::process::table::cpu_idx(),
            current_pid_snapshot(),
            crate::memory::paging::active_pml4(),
            crate::arch::syscall::kernel_gs_active()
        );
    }

    pub fn dump_fault_frame(
        kind: InterruptKind,
        tag: &'static str,
        rip: u64,
        rsp: u64,
        error_code: u64,
        cr2: u64,
    ) {
        Self::emit_interrupt_event(
            "fault.classify",
            crate::observability_contract::ObservationOutcome::Faulted,
            kind,
            [rip, rsp, error_code, cr2],
        );
        crate::serial_println!(
            "[interrupt-contract] frame tag={} kind={:?} rip={:#x} rsp={:#x} error={:#x} cr2={:#x} cpu={} current_pid={:?} cr3={:#x}",
            tag,
            kind,
            rip,
            rsp,
            error_code,
            cr2,
            crate::process::table::cpu_idx(),
            current_pid_snapshot(),
            crate::memory::paging::active_pml4()
        );
    }
}

fn interrupt_kind_code(kind: InterruptKind) -> u8 {
    match kind {
        InterruptKind::PageFault => 14,
        InterruptKind::GeneralProtectionFault => 13,
        InterruptKind::DoubleFault => 8,
        InterruptKind::Syscall => 0x80,
        InterruptKind::Timer => 32,
        InterruptKind::Device => 33,
        InterruptKind::Other(vector) => vector,
    }
}

fn current_pid_snapshot() -> Option<u32> {
    let pid = crate::process::table::TABLE
        .try_lock()
        .map(|table| table.current_pid())
        .unwrap_or(0);
    if pid == 0 { None } else { Some(pid) }
}

// ─── IRQ Storm Detection (DOC-08 §ProgressContract) ────────────────────────
//
// Constitutional requirement: "IRQ storm (CPU utilisation by IRQs above 80%)
// — threshold 5 seconds — emits IRQ_STORM."
//
// Implementation: Per-vector atomic counter. BSP timer calls `irq_storm_tick()`
// each second. If any vector fires > STORM_THRESHOLD per second for 5 consecutive
// seconds, emit KDS IrqStorm event.

use core::sync::atomic::{AtomicU64, Ordering};

const MAX_VECTORS: usize = 256;
/// Per-vector total IRQ counter (monotonically increasing).
static IRQ_COUNTS: [AtomicU64; MAX_VECTORS] = [const { AtomicU64::new(0) }; MAX_VECTORS];
/// Per-vector snapshot at last tick.
static IRQ_LAST: [AtomicU64; MAX_VECTORS] = [const { AtomicU64::new(0) }; MAX_VECTORS];
/// Per-vector consecutive-seconds-above-threshold counter.
static IRQ_STORM_STREAK: [AtomicU64; MAX_VECTORS] = [const { AtomicU64::new(0) }; MAX_VECTORS];

/// Threshold: more than 10,000 IRQs per second on a single vector is storm-like.
const STORM_THRESHOLD_PER_SEC: u64 = 10_000;
/// Must sustain for 5 consecutive seconds before emitting.
const STORM_SUSTAIN_SECS: u64 = 5;

/// Called from every IRQ entry point to count per-vector frequency.
#[inline]
pub fn irq_storm_count(vector: u8) {
    IRQ_COUNTS[vector as usize].fetch_add(1, Ordering::Relaxed);
}

/// Called once per second from BSP timer to detect storms.
pub fn irq_storm_tick() {
    for v in 0..MAX_VECTORS {
        let current = IRQ_COUNTS[v].load(Ordering::Relaxed);
        let last = IRQ_LAST[v].swap(current, Ordering::Relaxed);
        let delta = current.saturating_sub(last);
        if delta > STORM_THRESHOLD_PER_SEC {
            let streak = IRQ_STORM_STREAK[v].fetch_add(1, Ordering::Relaxed) + 1;
            if streak == STORM_SUSTAIN_SECS {
                crate::kds::kds_event(
                    crate::kds::KdsSubsystem::Interrupt,
                    crate::kds::KdsEventType::IrqStorm,
                    crate::kds::KdsSeverity::Warn,
                    [v as u64, delta, streak, 0],
                );
                crate::serial_println!(
                    "[irq-storm] vector={} rate={}/s sustained={}s",
                    v,
                    delta,
                    streak
                );
            }
        } else {
            IRQ_STORM_STREAK[v].store(0, Ordering::Relaxed);
        }
    }
}

// ─── Per-IRQ Handler Timing (F-INT-03) ─────────────────────────────────────
//
// Constitution requires per-IRQ telemetry: irq_number, cpu, handler_time_ns.
// We store the entry TSC per-CPU and compute elapsed on EOI.

use crate::process::table::MAX_CPUS;

/// Per-CPU entry timestamp (TSC ticks at IRQ entry).
static IRQ_ENTRY_TSC: [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];

#[inline]
fn irq_entry_timestamp_store() {
    let cpu = crate::process::table::cpu_idx();
    IRQ_ENTRY_TSC[cpu].store(rdtsc(), Ordering::Relaxed);
}

#[inline]
fn irq_handler_elapsed_ns() -> u64 {
    let cpu = crate::process::table::cpu_idx();
    let start = IRQ_ENTRY_TSC[cpu].load(Ordering::Relaxed);
    if start == 0 {
        return 0;
    }
    let elapsed_ticks = rdtsc().saturating_sub(start);
    crate::time::tsc_ticks_to_ns(elapsed_ticks)
}

#[inline]
fn rdtsc() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}
