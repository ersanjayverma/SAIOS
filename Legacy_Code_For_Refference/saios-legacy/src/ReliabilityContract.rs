//! ReliabilityContract and Red Ring owner surface.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub struct ReliabilityContract;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedRingCause {
    KernelPanic = 1,
    AllocationFailure = 2,
    ContractViolation = 3,
    LockOrderViolation = 4,
    KdsCriticalLoss = 5,
    HardwareFault = 6,
    WatchdogStall = 7,
    PolicyViolation = 8,
    DeadOnCpu = 9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedRingEvidence {
    pub cause: RedRingCause,
    pub evidence_event_id: u64,
    pub invariant_id: u64,
    pub detail: u64,
}

static RED_RING_ACTIVE: AtomicBool = AtomicBool::new(false);
static RED_RING_CAUSE: AtomicU32 = AtomicU32::new(0);
static RED_RING_CPU: AtomicU32 = AtomicU32::new(u32::MAX);
static RED_RING_PID: AtomicU32 = AtomicU32::new(0);
static RED_RING_TIME: AtomicU64 = AtomicU64::new(0);
static RED_RING_EVIDENCE_ID: AtomicU64 = AtomicU64::new(0);
static RED_RING_INVARIANT_ID: AtomicU64 = AtomicU64::new(0);
static RED_RING_DETAIL: AtomicU64 = AtomicU64::new(0);

impl ReliabilityContract {
    pub fn enter_red_ring(evidence: RedRingEvidence) {
        let cpu = crate::process::table::cpu_idx() as u32;
        let pid = crate::process::table::TABLE
            .try_lock()
            .and_then(|table| table.current_on_cpu(cpu as usize))
            .unwrap_or(0);
        let timestamp = crate::time::uptime_ns();

        RED_RING_CAUSE.store(evidence.cause as u32, Ordering::Relaxed);
        RED_RING_CPU.store(cpu, Ordering::Relaxed);
        RED_RING_PID.store(pid, Ordering::Relaxed);
        RED_RING_TIME.store(timestamp, Ordering::Relaxed);
        RED_RING_EVIDENCE_ID.store(evidence.evidence_event_id, Ordering::Relaxed);
        RED_RING_INVARIANT_ID.store(evidence.invariant_id, Ordering::Relaxed);
        RED_RING_DETAIL.store(evidence.detail, Ordering::Relaxed);
        RED_RING_ACTIVE.store(true, Ordering::Release);

        // Constitutional §Red Ring Step 2: NMI broadcast freezes all other CPUs
        // BEFORE seal.  Must happen after RED_RING_ACTIVE is set so NMI handler
        // recognizes the halt request.
        crate::smp::nmi_broadcast_halt();

        crate::kds::kds_event(
            crate::kds::KdsSubsystem::Reliability,
            crate::kds::KdsEventType::RedRingEntered,
            crate::kds::KdsSeverity::Fatal,
            [
                evidence.cause as u64,
                pid as u64,
                evidence.evidence_event_id,
                evidence.invariant_id,
            ],
        );
    }

    pub fn seal_red_ring() {
        let cause = RED_RING_CAUSE.load(Ordering::Relaxed) as u64;
        let pid = RED_RING_PID.load(Ordering::Relaxed) as u64;
        let evidence_event_id = RED_RING_EVIDENCE_ID.load(Ordering::Relaxed);
        let timestamp = RED_RING_TIME.load(Ordering::Relaxed);
        crate::kds::kds_event(
            crate::kds::KdsSubsystem::Reliability,
            crate::kds::KdsEventType::RedRingSealed,
            crate::kds::KdsSeverity::Fatal,
            [cause, pid, evidence_event_id, timestamp],
        );
        match crate::kds::seal_flight_recorder_final() {
            Ok(records) => crate::serial_println!(
                "[red-ring] flight recorder final seal complete records={}",
                records
            ),
            Err(reason) => {
                crate::serial_println!("[red-ring] flight recorder final seal failed: {}", reason)
            }
        }
    }

    pub fn active() -> bool {
        RED_RING_ACTIVE.load(Ordering::Acquire)
    }
}
