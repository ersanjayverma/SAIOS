//! Kernel Data Store (KDS): append-only kernel evidence streams.
//!
//! KDS is the authoritative telemetry substrate for events, metrics, traces,
//! objects, and state.  Phase 1 keeps fixed-size in-kernel append-only streams
//! so producers can emit evidence from scheduler, interrupt, and watchdog paths
//! without heap allocation or userspace services.  The stream path constants are
//! the durable storage contract for the later VFS flusher.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::multiboot::{MMAP_AVAILABLE, MemRegion};
use crate::process::table::MAX_CPUS;

const EVENT_CAPACITY: usize = 4096;
const METRIC_CAPACITY: usize = 4096;
const TRACE_CAPACITY: usize = 2048;
const OBJECT_CAPACITY: usize = 1024;
const STATE_CAPACITY: usize = 1024;
const AGGREGATE_CAPACITY: usize = 64;
const KDS_SLOT_SIZE: usize = 256;
const KDS_DEFAULT_SIZE: u64 = 512 * 1024 * 1024;
const KDS_MIN_SIZE: u64 = 32 * 1024 * 1024;
const KDS_MAX_RAM_DIVISOR: u64 = 4;
const KDS_CRITICAL_WAIT_NS: u64 = 1_000_000;
const KDS_IDENTITY_LIMIT: u64 = 128 * 1024 * 1024 * 1024;
const KDS_SCHEMA_VERSION: u16 = 1;
const KDS_SCHEMA_FLAG_VERSION: u32 = 1 << 0;
const KDS_SCHEMA_FLAG_UUID_V7: u32 = 1 << 1;
const KDS_SCHEMA_FLAG_SOURCE_CONTRACT: u32 = 1 << 2;
const KDS_SCHEMA_FLAG_SEVERITY_VOCABULARY: u32 = 1 << 3;
const KDS_SCHEMA_FLAG_EVENT_CATEGORY: u32 = 1 << 4;
const KDS_SCHEMA_FLAG_TYPED_SHAPE: u32 = 1 << 5;
const KDS_SCHEMA_FLAG_CONTEXT_TAGS: u32 = 1 << 6;
const KDS_SCHEMA_REQUIRED_FLAGS: u32 = KDS_SCHEMA_FLAG_VERSION
    | KDS_SCHEMA_FLAG_UUID_V7
    | KDS_SCHEMA_FLAG_SOURCE_CONTRACT
    | KDS_SCHEMA_FLAG_SEVERITY_VOCABULARY
    | KDS_SCHEMA_FLAG_EVENT_CATEGORY
    | KDS_SCHEMA_FLAG_TYPED_SHAPE
    | KDS_SCHEMA_FLAG_CONTEXT_TAGS;
const FLIGHT_RECORDER_BLOCK_SIZE: usize = 64 * 1024;
const FLIGHT_RECORDER_RECORDS_PER_BLOCK: usize = (FLIGHT_RECORDER_BLOCK_SIZE / KDS_SLOT_SIZE) - 1;
const FLIGHT_RECORDER_MAGIC: u64 = 0x5341_494f_5346_5242;
const FLIGHT_RECORDER_FLAG_FINAL: u16 = 1 << 0;
const TIER_1_BIT: u32 = 1 << 0;
const TIER_2_BIT: u32 = 1 << 1;

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdsSubsystem {
    Kernel = 1,
    Scheduler = 2,
    Memory = 3,
    Vfs = 4,
    Process = 5,
    Interrupt = 6,
    Smp = 7,
    Watchdog = 8,
    Syscall = 9,
    Driver = 10,
    Network = 11,
    Storage = 12,
    Security = 13,
    Shell = 14,
    Override = 15,
    Reliability = 16,
    Ipc = 17,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdsEventType {
    Boot = 1,
    ContextSwitch = 2,
    TaskCreate = 3,
    TaskExit = 4,
    TaskBlock = 5,
    TaskUnblock = 6,
    CpuIdle = 7,
    CpuOnline = 8,
    CpuOffline = 9,
    IpiSend = 10,
    IpiReceive = 11,
    PageAlloc = 12,
    PageFree = 13,
    CowFault = 14,
    PageFault = 15,
    Mmap = 16,
    Munmap = 17,
    Mprotect = 18,
    FileOpen = 19,
    FileClose = 20,
    FileRead = 21,
    FileWrite = 22,
    Mount = 23,
    Unmount = 24,
    Fork = 25,
    Execve = 26,
    Exit = 27,
    Wait = 28,
    Signal = 29,
    InterruptEnter = 30,
    InterruptExit = 31,
    Fault = 32,
    Exception = 33,
    WatchdogCpuStall = 34,
    SchedulerStall = 35,
    LockContention = 36,
    LockTimeout = 37,
    Metric = 38,
    TraceBegin = 39,
    TraceEnd = 40,
    Object = 41,
    State = 42,
    HardwareScanBegin = 43,
    HardwareScanComplete = 44,
    CompatibilityPass = 45,
    CompatibilityWarning = 46,
    CompatibilityFailure = 47,
    InstallApproved = 48,
    InstallAdvisory = 49,
    DiskOperationBegin = 50,
    DiskOperationProgress = 51,
    DiskOperationComplete = 52,
    DiskOperationFailure = 53,
    DiskOperationRollback = 54,
    BootRepairBegin = 55,
    BootRepairComplete = 56,
    RecoveryBegin = 57,
    RecoveryComplete = 58,
    OverrideRequest = 59,
    OverrideApproved = 60,
    OverrideExecuting = 61,
    OverrideComplete = 62,
    OverrideFailed = 63,
    OverrideAborted = 64,
    BootKdsReady = 65,
    KdsOverflow = 66,
    KdsCriticalLoss = 67,
    ResourceQuotaExceeded = 68,
    QuotaChanged = 69,
    AccountingAttributionFailure = 70,
    AccountingInvariantViolated = 71,
    ResourceAccountPeriod = 72,
    BootGatePassed = 73,
    BootGateFailed = 74,
    BootComplete = 75,
    SecuritySyscallDenied = 76,
    SecurityPrivilegeEscalation = 77,
    SecurityNamespaceEscape = 78,
    SecurityMacDenied = 79,
    SecurityAuditExec = 80,
    SecurityNetworkPolicyDeny = 81,
    RedRingEntered = 82,
    RedRingSealed = 83,
    ContractViolation = 84,
    LockOrderViolation = 85,
    NumaKdsSegment = 86,
    FrNodeAssignment = 87,
    FlightRecorderCriticalAck = 88,
    FlightRecorderFinalSeal = 89,
    FlightRecorderSealFailure = 90,
    SchedulerStarvation = 91,
    IrqStorm = 92,
    IpcPipeCreate = 93,
    FutexContention = 94,
    TestStart = 95,
    TestStep = 96,
    TestPass = 97,
    TestFail = 98,
    TestTimeout = 99,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdsSeverity {
    Trace = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
}

impl KdsSeverity {
    pub const fn schema_name(self) -> &'static str {
        match self {
            Self::Trace => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "CRITICAL",
        }
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdsEventCategory {
    Boot = 1,
    Process = 2,
    Memory = 3,
    Network = 4,
    Filesystem = 5,
    Scheduler = 6,
    Hardware = 7,
    Driver = 8,
    Security = 9,
    Syscall = 10,
    Reliability = 11,
    KdsSelf = 12,
    Numa = 13,
    Accounting = 14,
    Intelligence = 15,
    Storage = 16,
    Override = 17,
}

impl KdsEventCategory {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Process => "process",
            Self::Memory => "memory",
            Self::Network => "network",
            Self::Filesystem => "filesystem",
            Self::Scheduler => "scheduler",
            Self::Hardware => "hardware",
            Self::Driver => "driver",
            Self::Security => "security",
            Self::Syscall => "syscall",
            Self::Reliability => "reliability",
            Self::KdsSelf => "kds-self",
            Self::Numa => "numa",
            Self::Accounting => "accounting",
            Self::Intelligence => "intelligence",
            Self::Storage => "storage",
            Self::Override => "override",
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdsMetricId {
    CpuHeartbeat = 1,
    SchedulerProgress = 2,
    PageAlloc = 3,
    PageFree = 4,
    MmapBytes = 5,
    MunmapBytes = 6,
    WatchdogStallMs = 7,
    ContextSwitches = 8,
    Interrupts = 9,
    Faults = 10,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryTier {
    AlwaysOn = 0,
    Diagnostic = 1,
    DeepInvestigation = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdsObjectKind {
    Process = 1,
    Thread = 2,
    File = 3,
    Socket = 4,
    Driver = 5,
    Device = 6,
    Mount = 7,
    User = 8,
    Cpu = 9,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdsStreamId {
    Events = 1,
    Metrics = 2,
    Traces = 3,
    Objects = 4,
    State = 5,
}

impl KdsStreamId {
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Events => "events.bin",
            Self::Metrics => "metrics.bin",
            Self::Traces => "traces.bin",
            Self::Objects => "objects.bin",
            Self::State => "state.bin",
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Metrics => "metrics",
            Self::Traces => "traces",
            Self::Objects => "objects",
            Self::State => "state",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KdsStorageProvider {
    MemoryOnly,
    Tmpfs,
    SystemData,
}

impl KdsStorageProvider {
    pub const fn name(self) -> &'static str {
        match self {
            Self::MemoryOnly => "memory",
            Self::Tmpfs => "tmpfs",
            Self::SystemData => "system-data",
        }
    }

    pub const fn base_path(self) -> &'static str {
        match self {
            Self::MemoryOnly => "memory://kds",
            Self::Tmpfs => "/tmp",
            Self::SystemData => "/system/data",
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdsMetadata {
    pub timestamp: u64,
    pub cpu_id: u32,
    pub thread_id: u32,
    pub process_id: u32,
    pub subsystem: KdsSubsystem,
    pub event_type: KdsEventType,
    pub severity: KdsSeverity,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdsEventShape {
    pub contract: u16,
    pub tag: u16,
    pub outcome: u16,
    pub resource: u16,
    pub owner: u64,
    pub correlation_id: u64,
    pub reason_hash: u64,
}

impl KdsEventShape {
    pub const EMPTY: Self = Self {
        contract: 0,
        tag: 0,
        outcome: 0,
        resource: 0,
        owner: 0,
        correlation_id: 0,
        reason_hash: 0,
    };
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdsContextTag {
    pub key: u64,
    pub value: u64,
}

impl KdsContextTag {
    pub const EMPTY: Self = Self { key: 0, value: 0 };
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRecord {
    pub metadata: KdsMetadata,
    pub event_id: u64,
    pub event_uuid: [u64; 2],
    pub schema_version: u16,
    pub context_tag_count: u16,
    pub schema_flags: u32,
    pub shape: KdsEventShape,
    pub payload: [u64; 4],
    pub context_tags: [KdsContextTag; 8],
}

const _: [(); KDS_SLOT_SIZE] = [(); core::mem::size_of::<EventRecord>()];

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricRecord {
    pub metadata: KdsMetadata,
    pub metric_id: KdsMetricId,
    pub value: u64,
    pub payload: [u64; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceRecord {
    pub metadata: KdsMetadata,
    pub trace_id: u64,
    pub parent_trace_id: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub duration: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectRecord {
    pub metadata: KdsMetadata,
    pub object_id: u64,
    pub object_kind: KdsObjectKind,
    pub parent_object_id: u64,
    pub payload: [u64; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateRecord {
    pub metadata: KdsMetadata,
    pub state_id: u64,
    pub value: u64,
    pub payload: [u64; 2],
}

#[derive(Debug, Clone, Copy)]
pub struct KdsStreamStats {
    pub stream_id: KdsStreamId,
    pub storage_provider: KdsStorageProvider,
    pub base_path: &'static str,
    pub filename: &'static str,
    pub records: u64,
    pub dropped: u64,
    pub record_size: usize,
    pub capacity: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct KdsStats {
    pub events: KdsStreamStats,
    pub metrics: KdsStreamStats,
    pub traces: KdsStreamStats,
    pub objects: KdsStreamStats,
    pub state: KdsStreamStats,
    pub aggregates_used: usize,
    pub reserved_base: u64,
    pub reserved_size: u64,
    pub sealed: bool,
    pub cpu_rings: usize,
    pub critical_loss: u64,
    pub flight_recorder_degraded: u64,
    pub flight_recorder_writes: u64,
    pub flight_recorder_bytes: u64,
    pub flight_recorder_blocks: u64,
    pub flight_recorder_critical_acks: u64,
    pub flight_recorder_critical_failures: u64,
    pub flight_recorder_failures: u64,
    pub flight_recorder_final_seal_attempts: u64,
    pub flight_recorder_final_seals: u64,
    pub flight_recorder_final_seal_failures: u64,
    pub flight_recorder_final_records: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdsEventDescriptor {
    pub event_type: KdsEventType,
    pub name: &'static str,
    pub owner: KdsSubsystem,
    pub category: KdsEventCategory,
    pub baseline_severity: KdsSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdsRingAssignment {
    pub cpu: usize,
    pub base: u64,
    pub size: u64,
    pub slots: u64,
    pub slot_size: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlightRecorderBlockHeader {
    magic: u64,
    sequence: u64,
    records: u32,
    checksum: u32,
    block_size: u32,
    slot_size: u32,
    schema_version: u16,
    flags: u16,
    reserved: [u64; 27],
}

const EMPTY_FLIGHT_HEADER: FlightRecorderBlockHeader = FlightRecorderBlockHeader {
    magic: FLIGHT_RECORDER_MAGIC,
    sequence: 0,
    records: 0,
    checksum: 0,
    block_size: FLIGHT_RECORDER_BLOCK_SIZE as u32,
    slot_size: KDS_SLOT_SIZE as u32,
    schema_version: KDS_SCHEMA_VERSION,
    flags: 0,
    reserved: [0; 27],
};

const _: [(); KDS_SLOT_SIZE] = [(); core::mem::size_of::<FlightRecorderBlockHeader>()];

struct FlightRecorderBlock {
    bytes: [u8; FLIGHT_RECORDER_BLOCK_SIZE],
    records: usize,
    sequence: u64,
}

impl FlightRecorderBlock {
    const fn new() -> Self {
        Self {
            bytes: [0; FLIGHT_RECORDER_BLOCK_SIZE],
            records: 0,
            sequence: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdsReservation {
    pub base: u64,
    pub size: u64,
    pub cpu_rings: usize,
}

#[repr(C, align(64))]
struct KdsCpuRing {
    base: AtomicU64,
    capacity_slots: AtomicU64,
    write_head: AtomicU64,
    read_tail: AtomicU64,
    overflow: AtomicU64,
    critical_loss: AtomicU64,
}

impl KdsCpuRing {
    const fn new() -> Self {
        Self {
            base: AtomicU64::new(0),
            capacity_slots: AtomicU64::new(0),
            write_head: AtomicU64::new(0),
            read_tail: AtomicU64::new(0),
            overflow: AtomicU64::new(0),
            critical_loss: AtomicU64::new(0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KdsValidationReport {
    pub event_creation: bool,
    pub schema_bearing_events: bool,
    pub metric_creation: bool,
    pub trace_creation: bool,
    pub object_creation: bool,
    pub state_update: bool,
    pub stream_integrity: bool,
    pub buffer_accounting: bool,
    pub drop_accounting: bool,
    pub attribution_present: bool,
    pub taxonomy_coverage: bool,
}

impl KdsValidationReport {
    pub fn passed(self) -> bool {
        self.event_creation
            && self.schema_bearing_events
            && self.metric_creation
            && self.trace_creation
            && self.object_creation
            && self.state_update
            && self.stream_integrity
            && self.buffer_accounting
            && self.drop_accounting
            && self.attribution_present
            && self.taxonomy_coverage
    }
}

#[derive(Clone, Copy)]
struct AggregateSlot {
    subsystem: KdsSubsystem,
    metric_id: KdsMetricId,
    used: bool,
    count: u64,
    sum: u64,
    min: u64,
    max: u64,
    last: u64,
}

const EMPTY_AGGREGATE: AggregateSlot = AggregateSlot {
    subsystem: KdsSubsystem::Kernel,
    metric_id: KdsMetricId::CpuHeartbeat,
    used: false,
    count: 0,
    sum: 0,
    min: 0,
    max: 0,
    last: 0,
};

const EMPTY_META: KdsMetadata = KdsMetadata {
    timestamp: 0,
    cpu_id: 0,
    thread_id: 0,
    process_id: 0,
    subsystem: KdsSubsystem::Kernel,
    event_type: KdsEventType::Boot,
    severity: KdsSeverity::Info,
};

const EMPTY_EVENT: EventRecord = EventRecord {
    metadata: EMPTY_META,
    event_id: 0,
    event_uuid: [0; 2],
    schema_version: KDS_SCHEMA_VERSION,
    context_tag_count: 0,
    schema_flags: KDS_SCHEMA_REQUIRED_FLAGS,
    shape: KdsEventShape::EMPTY,
    payload: [0; 4],
    context_tags: [KdsContextTag::EMPTY; 8],
};
const EMPTY_METRIC: MetricRecord = MetricRecord {
    metadata: EMPTY_META,
    metric_id: KdsMetricId::CpuHeartbeat,
    value: 0,
    payload: [0; 2],
};
const EMPTY_TRACE: TraceRecord = TraceRecord {
    metadata: EMPTY_META,
    trace_id: 0,
    parent_trace_id: 0,
    start_time: 0,
    end_time: 0,
    duration: 0,
};
const EMPTY_OBJECT: ObjectRecord = ObjectRecord {
    metadata: EMPTY_META,
    object_id: 0,
    object_kind: KdsObjectKind::Process,
    parent_object_id: 0,
    payload: [0; 2],
};
const EMPTY_STATE: StateRecord = StateRecord {
    metadata: EMPTY_META,
    state_id: 0,
    value: 0,
    payload: [0; 2],
};

struct Stream<T: Copy, const N: usize> {
    records: [T; N],
    written: u64,
    dropped: u64,
}

impl<T: Copy, const N: usize> Stream<T, N> {
    const fn new(empty: T) -> Self {
        Self {
            records: [empty; N],
            written: 0,
            dropped: 0,
        }
    }

    fn append(&mut self, record: T) -> u64 {
        let seq = self.written;
        self.written = self.written.wrapping_add(1);
        if (seq as usize) < N {
            self.records[seq as usize] = record;
        } else {
            self.dropped = self.dropped.wrapping_add(1);
        }
        seq
    }

    fn stats(
        &self,
        stream_id: KdsStreamId,
        storage_provider: KdsStorageProvider,
        record_size: usize,
    ) -> KdsStreamStats {
        KdsStreamStats {
            stream_id,
            storage_provider,
            base_path: storage_provider.base_path(),
            filename: stream_id.filename(),
            records: self.written.min(N as u64),
            dropped: self.dropped,
            record_size,
            capacity: N,
        }
    }

    fn for_each_recent(&self, limit: usize, mut f: impl FnMut(&T)) {
        let stored = self.written.min(N as u64) as usize;
        let start = stored.saturating_sub(limit);
        for record in &self.records[start..stored] {
            f(record);
        }
    }
}

static EVENTS: Mutex<Stream<EventRecord, EVENT_CAPACITY>> = Mutex::new(Stream::new(EMPTY_EVENT));
static METRICS: Mutex<Stream<MetricRecord, METRIC_CAPACITY>> =
    Mutex::new(Stream::new(EMPTY_METRIC));
static TRACES: Mutex<Stream<TraceRecord, TRACE_CAPACITY>> = Mutex::new(Stream::new(EMPTY_TRACE));
static OBJECTS: Mutex<Stream<ObjectRecord, OBJECT_CAPACITY>> =
    Mutex::new(Stream::new(EMPTY_OBJECT));
static STATE: Mutex<Stream<StateRecord, STATE_CAPACITY>> = Mutex::new(Stream::new(EMPTY_STATE));

static NEXT_EVENT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TRACE_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(1);
static EVENT_LOCK_DROPS: AtomicU64 = AtomicU64::new(0);
static METRIC_LOCK_DROPS: AtomicU64 = AtomicU64::new(0);
static TRACE_LOCK_DROPS: AtomicU64 = AtomicU64::new(0);
static OBJECT_LOCK_DROPS: AtomicU64 = AtomicU64::new(0);
static STATE_LOCK_DROPS: AtomicU64 = AtomicU64::new(0);
static TELEMETRY_FLAGS: AtomicU32 = AtomicU32::new(0);
static AGGREGATES: Mutex<[AggregateSlot; AGGREGATE_CAPACITY]> =
    Mutex::new([EMPTY_AGGREGATE; AGGREGATE_CAPACITY]);
static KDS_RINGS: [KdsCpuRing; MAX_CPUS] = [const { KdsCpuRing::new() }; MAX_CPUS];
static KDS_RECURSION_GUARDS: [AtomicBool; MAX_CPUS] = [const { AtomicBool::new(false) }; MAX_CPUS];
pub static KDS_READY: AtomicBool = AtomicBool::new(false);
static KDS_SEALED: AtomicBool = AtomicBool::new(false);
static KDS_REGION_BASE: AtomicU64 = AtomicU64::new(0);
static KDS_REGION_SIZE: AtomicU64 = AtomicU64::new(0);
static KDS_RING_COUNT: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_DEGRADED: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_WRITES: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_BYTES: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_BLOCKS: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_CRITICAL_ACKS: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_CRITICAL_FAILURES: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_FAILURES: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_FINAL_SEAL_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_FINAL_SEALS: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_FINAL_SEAL_FAILURES: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_FINAL_RECORDS: AtomicU64 = AtomicU64::new(0);
static FLIGHT_RECORDER_DEGRADED_RECORDED: AtomicBool = AtomicBool::new(false);
static FLIGHT_RECORDER_IO_ACTIVE: AtomicBool = AtomicBool::new(false);
static FLIGHT_RECORDER_BLOCK: Mutex<FlightRecorderBlock> = Mutex::new(FlightRecorderBlock::new());

pub fn reserve_from_memory_map(
    regions: &[MemRegion],
    kernel_start: u64,
    kernel_end: u64,
) -> Result<KdsReservation, &'static str> {
    let total_available = regions
        .iter()
        .filter(|region| region.kind == MMAP_AVAILABLE)
        .fold(0u64, |sum, region| sum.saturating_add(region.len));
    let max_size = align_down_u64(
        total_available / KDS_MAX_RAM_DIVISOR,
        crate::memory::frame::FRAME_SIZE as u64,
    );
    let preferred_size = KDS_DEFAULT_SIZE.min(max_size).max(KDS_MIN_SIZE);

    for target_size in [preferred_size, KDS_MIN_SIZE] {
        for region in regions
            .iter()
            .filter(|region| region.kind == MMAP_AVAILABLE)
        {
            let mut start = align_up_u64(region.base, crate::memory::frame::FRAME_SIZE as u64);
            let region_end = align_down_u64(
                region
                    .base
                    .saturating_add(region.len)
                    .min(KDS_IDENTITY_LIMIT),
                crate::memory::frame::FRAME_SIZE as u64,
            );
            if ranges_overlap(start, region_end, kernel_start, kernel_end) && start < kernel_end {
                start = align_up_u64(kernel_end, crate::memory::frame::FRAME_SIZE as u64);
            }
            if start.saturating_add(target_size) <= region_end {
                return Ok(KdsReservation {
                    base: start,
                    size: target_size,
                    cpu_rings: MAX_CPUS,
                });
            }
        }
    }

    Err("Gate 0: unable to reserve minimum KDS region")
}

fn append_event_record(record: EventRecord) {
    if FLIGHT_RECORDER_IO_ACTIVE.load(Ordering::Acquire)
        && record.metadata.subsystem == KdsSubsystem::Vfs
    {
        EVENT_LOCK_DROPS.fetch_add(1, Ordering::Relaxed);
        return;
    }

    if KDS_READY.load(Ordering::Acquire) {
        append_ring_event(record);
        return;
    }

    let _ = record;
    EVENT_LOCK_DROPS.fetch_add(1, Ordering::Relaxed);
}

fn append_ring_event(record: EventRecord) {
    let cpu = (record.metadata.cpu_id as usize).min(MAX_CPUS.saturating_sub(1));
    let guard = &KDS_RECURSION_GUARDS[cpu];
    if guard.swap(true, Ordering::Acquire) {
        return;
    }
    write_ring_event(cpu, record);
    guard.store(false, Ordering::Release);
}

fn write_ring_event(cpu: usize, record: EventRecord) {
    let ring = &KDS_RINGS[cpu];
    let capacity = ring.capacity_slots.load(Ordering::Acquire);
    if capacity == 0 {
        ring.overflow.fetch_add(1, Ordering::Relaxed);
        return;
    }

    let mut head = ring.write_head.load(Ordering::Relaxed);
    let mut tail = ring.read_tail.load(Ordering::Acquire);
    if head.saturating_sub(tail) >= capacity {
        if !critical_delivery_required(record.metadata.severity) {
            ring.overflow.fetch_add(1, Ordering::Relaxed);
            return;
        }

        let start = crate::time::uptime_ns();
        while head.saturating_sub(tail) >= capacity {
            if crate::time::uptime_ns().saturating_sub(start) >= KDS_CRITICAL_WAIT_NS {
                note_critical_loss(cpu, record.event_id, KDS_CRITICAL_WAIT_NS);
                // Ring is full and not draining — drop this event gracefully.
                ring.overflow.fetch_add(1, Ordering::Relaxed);
                return;
            }
            core::hint::spin_loop();
            head = ring.write_head.load(Ordering::Relaxed);
            tail = ring.read_tail.load(Ordering::Acquire);
        }
    }

    let base = ring.base.load(Ordering::Acquire);
    let slot = head % capacity;
    let ptr = (base + slot * KDS_SLOT_SIZE as u64) as *mut EventRecord;
    unsafe {
        // SAFETY: Gate 4 initializes each ring base from the sealed KDS region,
        // each CPU writes only its own head-selected slot, and EventRecord fits
        // within the fixed 256-byte KDS slot.
        core::ptr::write_volatile(ptr, record);
    }
    let published_head = head.wrapping_add(1);
    ring.write_head.store(published_head, Ordering::Release);
    if critical_delivery_required(record.metadata.severity)
        && persist_critical_event(cpu, published_head).is_err()
    {
        FLIGHT_RECORDER_CRITICAL_FAILURES.fetch_add(1, Ordering::Relaxed);
        // Best-effort: log the failure but do NOT panic.  Telemetry must never
        // crash the kernel — if storage isn't available yet (early boot) or is
        // full, we degrade gracefully by incrementing counters and continuing.
        note_critical_loss(cpu, record.event_id, 0);
    }
}

/// Record a critical persistence failure.  Does NOT panic — telemetry infrastructure
/// must never bring down the kernel.  The event is still in the ring buffer and may
/// be flushed later by the flight recorder thread once storage becomes available.
fn note_critical_loss(cpu: usize, lost_event_id: u64, wait_ns: u64) {
    KDS_RINGS[cpu].critical_loss.fetch_add(1, Ordering::Relaxed);
    FLIGHT_RECORDER_DEGRADED.fetch_add(1, Ordering::Relaxed);
    crate::serial_println!(
        "[kds] CRITICAL loss cpu={} lost_event={} wait_ns={} (degraded, not fatal)",
        cpu,
        lost_event_id,
        wait_ns
    );
}

fn critical_delivery_required(severity: KdsSeverity) -> bool {
    matches!(severity, KdsSeverity::Error | KdsSeverity::Fatal)
}

fn append_metric_record(record: MetricRecord) {
    if let Some(mut metrics) = METRICS.try_lock() {
        metrics.append(record);
    } else {
        METRIC_LOCK_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

fn append_trace_record(record: TraceRecord) {
    if let Some(mut traces) = TRACES.try_lock() {
        traces.append(record);
    } else {
        TRACE_LOCK_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

fn append_object_record(record: ObjectRecord) {
    if let Some(mut objects) = OBJECTS.try_lock() {
        objects.append(record);
    } else {
        OBJECT_LOCK_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

fn append_state_record(record: StateRecord) {
    if let Some(mut state) = STATE.try_lock() {
        state.append(record);
    } else {
        STATE_LOCK_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn init(reservation: KdsReservation) {
    let per_cpu_size = align_down_u64(
        reservation.size / reservation.cpu_rings.max(1) as u64,
        KDS_SLOT_SIZE as u64,
    );
    let slots = per_cpu_size / KDS_SLOT_SIZE as u64;
    for cpu in 0..reservation.cpu_rings.min(MAX_CPUS) {
        let ring = &KDS_RINGS[cpu];
        ring.base.store(
            reservation.base + per_cpu_size.saturating_mul(cpu as u64),
            Ordering::Release,
        );
        ring.capacity_slots.store(slots, Ordering::Release);
        ring.write_head.store(0, Ordering::Release);
        ring.read_tail.store(0, Ordering::Release);
        ring.overflow.store(0, Ordering::Release);
        ring.critical_loss.store(0, Ordering::Release);
        KDS_RECURSION_GUARDS[cpu].store(false, Ordering::Release);
    }
    KDS_REGION_BASE.store(reservation.base, Ordering::Release);
    KDS_REGION_SIZE.store(reservation.size, Ordering::Release);
    KDS_RING_COUNT.store(
        reservation.cpu_rings.min(MAX_CPUS) as u64,
        Ordering::Release,
    );
    KDS_SEALED.store(true, Ordering::Release);
    KDS_READY.store(true, Ordering::Release);
    kds_event(
        KdsSubsystem::Kernel,
        KdsEventType::BootKdsReady,
        KdsSeverity::Info,
        [
            reservation.base,
            reservation.size,
            slots,
            reservation.cpu_rings as u64,
        ],
    );
    crate::serial_println!(
        "[kds] reserved sealed region {:#x}-{:#x} rings={} slots_per_cpu={}",
        reservation.base,
        reservation.base + reservation.size,
        reservation.cpu_rings.min(MAX_CPUS),
        slots
    );
}

pub fn ring_assignment(cpu: usize) -> Option<KdsRingAssignment> {
    if cpu >= MAX_CPUS || cpu >= KDS_RING_COUNT.load(Ordering::Acquire) as usize {
        return None;
    }
    let ring = &KDS_RINGS[cpu];
    let base = ring.base.load(Ordering::Acquire);
    let slots = ring.capacity_slots.load(Ordering::Acquire);
    if base == 0 || slots == 0 {
        return None;
    }
    Some(KdsRingAssignment {
        cpu,
        base,
        size: slots.saturating_mul(KDS_SLOT_SIZE as u64),
        slots,
        slot_size: KDS_SLOT_SIZE,
    })
}

pub const fn slot_size() -> usize {
    KDS_SLOT_SIZE
}

pub fn storage_provider() -> KdsStorageProvider {
    if crate::vfs::resolve("/system/data").is_ok() {
        KdsStorageProvider::SystemData
    } else if crate::vfs::resolve("/tmp").is_ok() {
        KdsStorageProvider::Tmpfs
    } else {
        KdsStorageProvider::MemoryOnly
    }
}

pub fn flush_flight_recorder(limit: usize) -> Result<usize, &'static str> {
    let Some(path) = flight_recorder_path() else {
        note_flight_recorder_degraded();
        return Err("flight recorder storage unavailable");
    };
    if storage_provider() != KdsStorageProvider::SystemData {
        note_flight_recorder_degraded();
    }

    let mut flushed = 0usize;
    let ring_count = KDS_RING_COUNT.load(Ordering::Acquire) as usize;
    for cpu in 0..ring_count.min(MAX_CPUS) {
        if flushed >= limit {
            break;
        }
        let remaining = limit - flushed;
        flushed += drain_ring_to_flight_recorder(cpu, path, remaining)?;
    }

    Ok(flushed)
}

pub fn seal_flight_recorder_final() -> Result<usize, &'static str> {
    FLIGHT_RECORDER_FINAL_SEAL_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    let Some(path) = flight_recorder_path() else {
        note_flight_recorder_degraded();
        FLIGHT_RECORDER_FINAL_SEAL_FAILURES.fetch_add(1, Ordering::Relaxed);
        kds_event(
            KdsSubsystem::Reliability,
            KdsEventType::FlightRecorderSealFailure,
            KdsSeverity::Error,
            [0, 0, storage_provider() as u64, 1],
        );
        return Err("flight recorder storage unavailable");
    };

    let mut flushed = 0usize;
    let ring_count = KDS_RING_COUNT.load(Ordering::Acquire) as usize;
    for cpu in 0..ring_count.min(MAX_CPUS) {
        match drain_ring_to_flight_recorder(cpu, path, usize::MAX) {
            Ok(records) => flushed += records,
            Err(reason) => {
                FLIGHT_RECORDER_FINAL_SEAL_FAILURES.fetch_add(1, Ordering::Relaxed);
                kds_event(
                    KdsSubsystem::Reliability,
                    KdsEventType::FlightRecorderSealFailure,
                    KdsSeverity::Error,
                    [cpu as u64, flushed as u64, storage_provider() as u64, 2],
                );
                return Err(reason);
            }
        }
    }

    let mut block = FLIGHT_RECORDER_BLOCK.lock();
    if let Err(reason) =
        flush_flight_recorder_block_with_flags(path, &mut block, FLIGHT_RECORDER_FLAG_FINAL)
    {
        FLIGHT_RECORDER_FINAL_SEAL_FAILURES.fetch_add(1, Ordering::Relaxed);
        kds_event(
            KdsSubsystem::Reliability,
            KdsEventType::FlightRecorderSealFailure,
            KdsSeverity::Error,
            [
                ring_count as u64,
                flushed as u64,
                storage_provider() as u64,
                3,
            ],
        );
        return Err(reason);
    }
    FLIGHT_RECORDER_FINAL_SEALS.fetch_add(1, Ordering::Relaxed);
    FLIGHT_RECORDER_FINAL_RECORDS.fetch_add(flushed as u64, Ordering::Relaxed);
    kds_event(
        KdsSubsystem::Reliability,
        KdsEventType::FlightRecorderFinalSeal,
        KdsSeverity::Info,
        [
            ring_count as u64,
            flushed as u64,
            storage_provider() as u64,
            0,
        ],
    );
    Ok(flushed)
}

fn persist_critical_event(cpu: usize, published_head: u64) -> Result<(), &'static str> {
    let Some(path) = flight_recorder_path() else {
        note_flight_recorder_degraded();
        return Err("flight recorder storage unavailable");
    };

    let ring = &KDS_RINGS[cpu];
    while ring.read_tail.load(Ordering::Acquire) < published_head {
        let before = ring.read_tail.load(Ordering::Acquire);
        let remaining = published_head.saturating_sub(before).min(usize::MAX as u64) as usize;
        drain_ring_to_flight_recorder(cpu, path, remaining)?;
        let after = ring.read_tail.load(Ordering::Acquire);
        if after <= before {
            note_flight_recorder_degraded();
            return Err("flight recorder critical persistence stalled");
        }
    }

    let mut block = FLIGHT_RECORDER_BLOCK.lock();
    flush_flight_recorder_block(path, &mut block)?;
    FLIGHT_RECORDER_CRITICAL_ACKS.fetch_add(1, Ordering::Relaxed);
    kds_event(
        KdsSubsystem::Reliability,
        KdsEventType::FlightRecorderCriticalAck,
        KdsSeverity::Info,
        [cpu as u64, published_head, storage_provider() as u64, 0],
    );
    Ok(())
}

pub extern "C" fn flight_recorder_thread() {
    // F-KDS-05: Periodic persistence — flush every 5 seconds (500 ticks at 100 Hz PIT).
    const FLUSH_INTERVAL_TICKS: u64 = 500;
    const FLUSH_BATCH: usize = 256;

    // Ensure storage directory exists on first iteration.
    let _ = crate::vfs_contract::VfsContract::mkdir("/system/data", 0o755);

    loop {
        let wake_tick = crate::shell::commands::boot_ticks().wrapping_add(FLUSH_INTERVAL_TICKS);
        crate::interrupts::block_until_tick(wake_tick);
        let _ = flush_flight_recorder(FLUSH_BATCH);
    }
}

fn drain_ring_to_flight_recorder(
    cpu: usize,
    path: &'static str,
    limit: usize,
) -> Result<usize, &'static str> {
    let ring = &KDS_RINGS[cpu];
    let capacity = ring.capacity_slots.load(Ordering::Acquire);
    if capacity == 0 || limit == 0 {
        return Ok(0);
    }

    let head = ring.write_head.load(Ordering::Acquire);
    let mut tail = ring.read_tail.load(Ordering::Acquire);
    if head.saturating_sub(tail) > capacity {
        let skipped = head - capacity - tail;
        ring.overflow.fetch_add(skipped, Ordering::Relaxed);
        tail = head - capacity;
        ring.read_tail.store(tail, Ordering::Release);
    }

    let mut flushed = 0usize;
    while tail < head && flushed < limit {
        let slot = tail % capacity;
        let ptr =
            (ring.base.load(Ordering::Acquire) + slot * KDS_SLOT_SIZE as u64) as *const EventRecord;
        let record = unsafe {
            // SAFETY: The Flight Recorder consumes slots inside the sealed KDS
            // ring. The writer publishes slots with a release write_head store;
            // this acquire-side read observes only published records.
            core::ptr::read_volatile(ptr)
        };
        if record.event_id == 0 || record.schema_version != KDS_SCHEMA_VERSION {
            tail = tail.wrapping_add(1);
            ring.read_tail.store(tail, Ordering::Release);
            continue;
        }

        if append_flight_recorder_record(path, &record).is_err() {
            FLIGHT_RECORDER_FAILURES.fetch_add(1, Ordering::Relaxed);
            note_flight_recorder_degraded();
            return Err("flight recorder append failed");
        }

        FLIGHT_RECORDER_WRITES.fetch_add(1, Ordering::Relaxed);
        flushed += 1;
        tail = tail.wrapping_add(1);
        ring.read_tail.store(tail, Ordering::Release);
    }

    Ok(flushed)
}

fn append_flight_recorder_record(
    path: &'static str,
    record: &EventRecord,
) -> Result<(), &'static str> {
    let mut block = FLIGHT_RECORDER_BLOCK.lock();
    if block.records >= FLIGHT_RECORDER_RECORDS_PER_BLOCK {
        flush_flight_recorder_block(path, &mut block)?;
    }

    let offset = KDS_SLOT_SIZE + block.records * KDS_SLOT_SIZE;
    let src = event_record_bytes(record);
    block.bytes[offset..offset + KDS_SLOT_SIZE].copy_from_slice(src);
    block.records += 1;

    if block.records >= FLIGHT_RECORDER_RECORDS_PER_BLOCK {
        flush_flight_recorder_block(path, &mut block)?;
    }
    Ok(())
}

fn flush_flight_recorder_block(
    path: &'static str,
    block: &mut FlightRecorderBlock,
) -> Result<(), &'static str> {
    flush_flight_recorder_block_with_flags(path, block, 0)
}

fn flush_flight_recorder_block_with_flags(
    path: &'static str,
    block: &mut FlightRecorderBlock,
    flags: u16,
) -> Result<(), &'static str> {
    if block.records == 0 && flags == 0 {
        return Ok(());
    }

    block.bytes[..KDS_SLOT_SIZE].fill(0);
    let checksum = checksum32(&block.bytes[KDS_SLOT_SIZE..]);
    let header = FlightRecorderBlockHeader {
        sequence: block.sequence,
        records: block.records as u32,
        checksum,
        flags,
        ..EMPTY_FLIGHT_HEADER
    };
    let header_bytes = flight_header_bytes(&header);
    block.bytes[..KDS_SLOT_SIZE].copy_from_slice(header_bytes);

    if flight_recorder_append_file(path, &block.bytes).is_err() {
        return Err("flight recorder block append failed");
    }

    FLIGHT_RECORDER_BLOCKS.fetch_add(1, Ordering::Relaxed);
    FLIGHT_RECORDER_BYTES.fetch_add(FLIGHT_RECORDER_BLOCK_SIZE as u64, Ordering::Relaxed);
    block.sequence = block.sequence.wrapping_add(1);
    block.records = 0;
    block.bytes.fill(0);
    Ok(())
}

fn flight_recorder_append_file(path: &'static str, bytes: &[u8]) -> Result<(), &'static str> {
    if FLIGHT_RECORDER_IO_ACTIVE.swap(true, Ordering::AcqRel) {
        return Err("flight recorder recursive append");
    }
    let result = crate::vfs_contract::VfsContract::append_file(path, bytes, 0o600);
    FLIGHT_RECORDER_IO_ACTIVE.store(false, Ordering::Release);
    result.map_err(|_| "flight recorder block append failed")
}

fn checksum32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0x82f6_3b78 & mask);
        }
    }
    !crc
}

fn flight_header_bytes(header: &FlightRecorderBlockHeader) -> &[u8] {
    unsafe {
        // SAFETY: FlightRecorderBlockHeader is repr(C), Copy, and checked to
        // occupy one fixed KDS slot used as the 64 KiB block header.
        core::slice::from_raw_parts(
            header as *const FlightRecorderBlockHeader as *const u8,
            core::mem::size_of::<FlightRecorderBlockHeader>(),
        )
    }
}

fn note_flight_recorder_degraded() {
    if !FLIGHT_RECORDER_DEGRADED_RECORDED.swap(true, Ordering::AcqRel) {
        FLIGHT_RECORDER_DEGRADED.fetch_add(1, Ordering::Relaxed);
    }
}

fn flight_recorder_path() -> Option<&'static str> {
    match storage_provider() {
        KdsStorageProvider::SystemData => Some("/system/data/kds-flight-events.bin"),
        KdsStorageProvider::Tmpfs => Some("/tmp/kds-flight-events.bin"),
        KdsStorageProvider::MemoryOnly => None,
    }
}

fn event_record_bytes(record: &EventRecord) -> &[u8] {
    unsafe {
        // SAFETY: EventRecord is repr(C), Copy, and compile-time checked to be
        // exactly one fixed KDS slot, so exposing its bytes preserves the slot
        // image written into the Flight Recorder.
        core::slice::from_raw_parts(
            record as *const EventRecord as *const u8,
            core::mem::size_of::<EventRecord>(),
        )
    }
}

pub fn kds_event(
    subsystem: KdsSubsystem,
    event_type: KdsEventType,
    severity: KdsSeverity,
    payload: [u64; 4],
) -> u64 {
    let (pid, tid) = current_process_thread();
    kds_event_for(subsystem, event_type, severity, pid, tid, payload)
}

pub fn kds_event_tier(
    tier: TelemetryTier,
    subsystem: KdsSubsystem,
    event_type: KdsEventType,
    severity: KdsSeverity,
    payload: [u64; 4],
) -> Option<u64> {
    if !tier_enabled(tier) {
        return None;
    }
    Some(kds_event(subsystem, event_type, severity, payload))
}

pub fn kds_event_for(
    subsystem: KdsSubsystem,
    event_type: KdsEventType,
    severity: KdsSeverity,
    pid: u32,
    tid: u32,
    payload: [u64; 4],
) -> u64 {
    kds_event_record_for(
        subsystem,
        event_type,
        severity,
        pid,
        tid,
        KdsEventShape::EMPTY,
        payload,
    )
}

pub fn kds_event_record_for(
    subsystem: KdsSubsystem,
    event_type: KdsEventType,
    severity: KdsSeverity,
    pid: u32,
    tid: u32,
    shape: KdsEventShape,
    payload: [u64; 4],
) -> u64 {
    let event_id = NEXT_EVENT_ID.fetch_add(1, Ordering::Relaxed);
    let metadata = metadata_for(subsystem, event_type, severity, pid, tid);
    let record = EventRecord {
        metadata,
        event_id,
        event_uuid: event_uuid_for(event_id, &metadata),
        schema_version: KDS_SCHEMA_VERSION,
        context_tag_count: 0,
        schema_flags: schema_flags_for(&metadata, 0, &shape),
        shape,
        payload,
        context_tags: [KdsContextTag::EMPTY; 8],
    };
    append_event_record(record);
    event_id
}

pub fn kds_metric(metric_id: KdsMetricId, value: u64, payload: [u64; 2]) {
    let (pid, tid) = current_process_thread();
    let record = MetricRecord {
        metadata: metadata_for(
            KdsSubsystem::Kernel,
            KdsEventType::Metric,
            KdsSeverity::Trace,
            pid,
            tid,
        ),
        metric_id,
        value,
        payload,
    };
    append_metric_record(record);
}

pub fn set_telemetry_tier(tier: TelemetryTier, enabled: bool) {
    let bit = match tier {
        TelemetryTier::AlwaysOn => return,
        TelemetryTier::Diagnostic => TIER_1_BIT,
        TelemetryTier::DeepInvestigation => TIER_2_BIT,
    };
    if enabled {
        TELEMETRY_FLAGS.fetch_or(bit, Ordering::Relaxed);
    } else {
        TELEMETRY_FLAGS.fetch_and(!bit, Ordering::Relaxed);
    }
}

pub fn tier_enabled(tier: TelemetryTier) -> bool {
    match tier {
        TelemetryTier::AlwaysOn => true,
        TelemetryTier::Diagnostic => TELEMETRY_FLAGS.load(Ordering::Relaxed) & TIER_1_BIT != 0,
        TelemetryTier::DeepInvestigation => {
            TELEMETRY_FLAGS.load(Ordering::Relaxed) & TIER_2_BIT != 0
        }
    }
}

pub fn obs_counter(subsystem: KdsSubsystem, metric_id: KdsMetricId, delta: u64) {
    update_aggregate(subsystem, metric_id, delta, AggregateMode::Counter);
}

pub fn obs_gauge(subsystem: KdsSubsystem, metric_id: KdsMetricId, value: u64) {
    update_aggregate(subsystem, metric_id, value, AggregateMode::Gauge);
}

pub fn obs_histogram(subsystem: KdsSubsystem, metric_id: KdsMetricId, sample: u64) {
    update_aggregate(subsystem, metric_id, sample, AggregateMode::Histogram);
}

pub fn flush_aggregates() {
    let (pid, tid) = current_process_thread();
    let Some(mut aggregates) = AGGREGATES.try_lock() else {
        return;
    };
    for slot in aggregates.iter_mut().filter(|slot| slot.used) {
        let value = slot.sum;
        let payload = [slot.count, (slot.min << 32) | slot.max.min(u32::MAX as u64)];
        let record = MetricRecord {
            metadata: metadata_for(
                slot.subsystem,
                KdsEventType::Metric,
                KdsSeverity::Trace,
                pid,
                tid,
            ),
            metric_id: slot.metric_id,
            value,
            payload,
        };
        append_metric_record(record);
        slot.count = 0;
        slot.sum = 0;
        slot.min = 0;
        slot.max = 0;
    }
}

pub fn kds_metric_for(
    subsystem: KdsSubsystem,
    metric_id: KdsMetricId,
    value: u64,
    pid: u32,
    tid: u32,
    payload: [u64; 2],
) {
    let record = MetricRecord {
        metadata: metadata_for(
            subsystem,
            KdsEventType::Metric,
            KdsSeverity::Trace,
            pid,
            tid,
        ),
        metric_id,
        value,
        payload,
    };
    append_metric_record(record);
}

pub fn kds_trace_begin(subsystem: KdsSubsystem, parent_trace_id: u64) -> u64 {
    let trace_id = NEXT_TRACE_ID.fetch_add(1, Ordering::Relaxed);
    let (pid, tid) = current_process_thread();
    let now = crate::time::uptime_ns();
    let record = TraceRecord {
        metadata: metadata_for(
            subsystem,
            KdsEventType::TraceBegin,
            KdsSeverity::Trace,
            pid,
            tid,
        ),
        trace_id,
        parent_trace_id,
        start_time: now,
        end_time: 0,
        duration: 0,
    };
    append_trace_record(record);
    trace_id
}

pub fn kds_trace_end(
    subsystem: KdsSubsystem,
    trace_id: u64,
    parent_trace_id: u64,
    start_time: u64,
) {
    let (pid, tid) = current_process_thread();
    let end_time = crate::time::uptime_ns();
    let record = TraceRecord {
        metadata: metadata_for(
            subsystem,
            KdsEventType::TraceEnd,
            KdsSeverity::Trace,
            pid,
            tid,
        ),
        trace_id,
        parent_trace_id,
        start_time,
        end_time,
        duration: end_time.saturating_sub(start_time),
    };
    append_trace_record(record);
}

pub fn kds_object(object_kind: KdsObjectKind, parent_object_id: u64, payload: [u64; 2]) -> u64 {
    let object_id = NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed);
    let (pid, tid) = current_process_thread();
    let record = ObjectRecord {
        metadata: metadata_for(
            KdsSubsystem::Kernel,
            KdsEventType::Object,
            KdsSeverity::Info,
            pid,
            tid,
        ),
        object_id,
        object_kind,
        parent_object_id,
        payload,
    };
    append_object_record(record);
    object_id
}

pub fn kds_state(
    subsystem: KdsSubsystem,
    state_id: u64,
    value: u64,
    severity: KdsSeverity,
    payload: [u64; 2],
) {
    let (pid, tid) = current_process_thread();
    kds_state_for(subsystem, state_id, value, severity, pid, tid, payload);
}

pub fn kds_state_for(
    subsystem: KdsSubsystem,
    state_id: u64,
    value: u64,
    severity: KdsSeverity,
    pid: u32,
    tid: u32,
    payload: [u64; 2],
) {
    let record = StateRecord {
        metadata: metadata_for(subsystem, KdsEventType::State, severity, pid, tid),
        state_id,
        value,
        payload,
    };
    append_state_record(record);
}

pub fn stats() -> KdsStats {
    let provider = storage_provider();
    let mut events = if KDS_READY.load(Ordering::Acquire) {
        ring_event_stats(provider)
    } else {
        EVENTS.lock().stats(
            KdsStreamId::Events,
            provider,
            core::mem::size_of::<EventRecord>(),
        )
    };
    events.dropped = events
        .dropped
        .saturating_add(EVENT_LOCK_DROPS.load(Ordering::Relaxed));
    let mut metrics = METRICS.lock().stats(
        KdsStreamId::Metrics,
        provider,
        core::mem::size_of::<MetricRecord>(),
    );
    metrics.dropped = metrics
        .dropped
        .saturating_add(METRIC_LOCK_DROPS.load(Ordering::Relaxed));
    let mut traces = TRACES.lock().stats(
        KdsStreamId::Traces,
        provider,
        core::mem::size_of::<TraceRecord>(),
    );
    traces.dropped = traces
        .dropped
        .saturating_add(TRACE_LOCK_DROPS.load(Ordering::Relaxed));
    let mut objects = OBJECTS.lock().stats(
        KdsStreamId::Objects,
        provider,
        core::mem::size_of::<ObjectRecord>(),
    );
    objects.dropped = objects
        .dropped
        .saturating_add(OBJECT_LOCK_DROPS.load(Ordering::Relaxed));
    let mut state = STATE.lock().stats(
        KdsStreamId::State,
        provider,
        core::mem::size_of::<StateRecord>(),
    );
    state.dropped = state
        .dropped
        .saturating_add(STATE_LOCK_DROPS.load(Ordering::Relaxed));
    KdsStats {
        events,
        metrics,
        traces,
        objects,
        state,
        aggregates_used: AGGREGATES.lock().iter().filter(|slot| slot.used).count(),
        reserved_base: KDS_REGION_BASE.load(Ordering::Acquire),
        reserved_size: KDS_REGION_SIZE.load(Ordering::Acquire),
        sealed: KDS_SEALED.load(Ordering::Acquire),
        cpu_rings: KDS_RING_COUNT.load(Ordering::Acquire) as usize,
        critical_loss: critical_loss_count(),
        flight_recorder_degraded: FLIGHT_RECORDER_DEGRADED.load(Ordering::Relaxed),
        flight_recorder_writes: FLIGHT_RECORDER_WRITES.load(Ordering::Relaxed),
        flight_recorder_bytes: FLIGHT_RECORDER_BYTES.load(Ordering::Relaxed),
        flight_recorder_blocks: FLIGHT_RECORDER_BLOCKS.load(Ordering::Relaxed),
        flight_recorder_critical_acks: FLIGHT_RECORDER_CRITICAL_ACKS.load(Ordering::Relaxed),
        flight_recorder_critical_failures: FLIGHT_RECORDER_CRITICAL_FAILURES
            .load(Ordering::Relaxed),
        flight_recorder_failures: FLIGHT_RECORDER_FAILURES.load(Ordering::Relaxed),
        flight_recorder_final_seal_attempts: FLIGHT_RECORDER_FINAL_SEAL_ATTEMPTS
            .load(Ordering::Relaxed),
        flight_recorder_final_seals: FLIGHT_RECORDER_FINAL_SEALS.load(Ordering::Relaxed),
        flight_recorder_final_seal_failures: FLIGHT_RECORDER_FINAL_SEAL_FAILURES
            .load(Ordering::Relaxed),
        flight_recorder_final_records: FLIGHT_RECORDER_FINAL_RECORDS.load(Ordering::Relaxed),
    }
}

pub fn for_each_event(limit: usize, f: impl FnMut(&EventRecord)) {
    if KDS_READY.load(Ordering::Acquire) {
        for_each_ring_event(limit, f);
    } else {
        EVENTS.lock().for_each_recent(limit, f);
    }
}

pub fn for_each_metric(limit: usize, f: impl FnMut(&MetricRecord)) {
    METRICS.lock().for_each_recent(limit, f);
}

pub fn for_each_trace(limit: usize, f: impl FnMut(&TraceRecord)) {
    TRACES.lock().for_each_recent(limit, f);
}

pub fn for_each_object(limit: usize, f: impl FnMut(&ObjectRecord)) {
    OBJECTS.lock().for_each_recent(limit, f);
}

pub fn for_each_state(limit: usize, f: impl FnMut(&StateRecord)) {
    STATE.lock().for_each_recent(limit, f);
}

pub fn latest_event(event_type: KdsEventType) -> Option<EventRecord> {
    if KDS_READY.load(Ordering::Acquire) {
        let mut latest = None;
        for_each_ring_event(EVENT_CAPACITY, |record| {
            if record.metadata.event_type == event_type && record.event_id != 0 {
                latest = Some(*record);
            }
        });
        return latest;
    }
    let events = EVENTS.try_lock()?;
    let stored = events.written.min(EVENT_CAPACITY as u64) as usize;
    let mut idx = stored;
    while idx > 0 {
        idx -= 1;
        let record = events.records[idx];
        if record.metadata.event_type == event_type && record.event_id != 0 {
            return Some(record);
        }
    }
    None
}

fn ring_event_stats(storage_provider: KdsStorageProvider) -> KdsStreamStats {
    let mut records = 0u64;
    let mut dropped = 0u64;
    let mut capacity = 0usize;
    for cpu in 0..KDS_RING_COUNT.load(Ordering::Acquire) as usize {
        let ring = &KDS_RINGS[cpu.min(MAX_CPUS - 1)];
        let ring_capacity = ring.capacity_slots.load(Ordering::Acquire);
        let written = ring.write_head.load(Ordering::Acquire);
        records = records.saturating_add(written.min(ring_capacity));
        dropped = dropped
            .saturating_add(ring.overflow.load(Ordering::Relaxed))
            .saturating_add(ring.critical_loss.load(Ordering::Relaxed));
        capacity = capacity.saturating_add(ring_capacity as usize);
    }
    KdsStreamStats {
        stream_id: KdsStreamId::Events,
        storage_provider,
        base_path: storage_provider.base_path(),
        filename: KdsStreamId::Events.filename(),
        records,
        dropped,
        record_size: core::mem::size_of::<EventRecord>(),
        capacity,
    }
}

fn for_each_ring_event(limit: usize, mut f: impl FnMut(&EventRecord)) {
    let mut remaining = limit;
    for cpu in 0..KDS_RING_COUNT.load(Ordering::Acquire) as usize {
        if remaining == 0 {
            break;
        }
        let ring = &KDS_RINGS[cpu.min(MAX_CPUS - 1)];
        let capacity = ring.capacity_slots.load(Ordering::Acquire);
        if capacity == 0 {
            continue;
        }
        let head = ring.write_head.load(Ordering::Acquire);
        let visible = head.min(capacity);
        let start = head.saturating_sub(visible.min(remaining as u64));
        for idx in start..head {
            let slot = idx % capacity;
            let ptr = (ring.base.load(Ordering::Acquire) + slot * KDS_SLOT_SIZE as u64)
                as *const EventRecord;
            let record = unsafe {
                // SAFETY: Reader inspects initialized slots within the sealed
                // KDS ring bounds using volatile loads so halt-path evidence is
                // read from memory, not compiler-cached state.
                core::ptr::read_volatile(ptr)
            };
            f(&record);
            remaining = remaining.saturating_sub(1);
            if remaining == 0 {
                break;
            }
        }
    }
}

fn critical_loss_count() -> u64 {
    let mut count = 0u64;
    for cpu in 0..KDS_RING_COUNT.load(Ordering::Acquire) as usize {
        count = count.saturating_add(
            KDS_RINGS[cpu.min(MAX_CPUS - 1)]
                .critical_loss
                .load(Ordering::Relaxed),
        );
    }
    count
}

pub fn count_events(event_type: KdsEventType) -> u64 {
    let mut count = 0;
    for_each_event(EVENT_CAPACITY, |record| {
        if record.metadata.event_type == event_type {
            count += 1;
        }
    });
    count
}

pub fn count_metrics(metric_id: KdsMetricId) -> u64 {
    let mut count = 0;
    for_each_metric(METRIC_CAPACITY, |record| {
        if record.metric_id == metric_id {
            count += 1;
        }
    });
    count
}

pub fn count_events_for_subsystem(subsystem: KdsSubsystem) -> u64 {
    let mut count = 0;
    for_each_event(EVENT_CAPACITY, |record| {
        if record.metadata.subsystem == subsystem {
            count += 1;
        }
    });
    count
}

pub fn aggregate_exists(subsystem: KdsSubsystem, metric_id: KdsMetricId) -> bool {
    AGGREGATES
        .lock()
        .iter()
        .any(|slot| slot.used && slot.subsystem == subsystem && slot.metric_id == metric_id)
}

pub fn aggregate_value(subsystem: KdsSubsystem, metric_id: KdsMetricId) -> Option<u64> {
    AGGREGATES
        .lock()
        .iter()
        .find(|slot| slot.used && slot.subsystem == subsystem && slot.metric_id == metric_id)
        .map(|slot| slot.sum)
}

pub fn subsystem_name(subsystem: KdsSubsystem) -> &'static str {
    match subsystem {
        KdsSubsystem::Kernel => "kernel",
        KdsSubsystem::Scheduler => "scheduler",
        KdsSubsystem::Memory => "memory",
        KdsSubsystem::Vfs => "vfs",
        KdsSubsystem::Process => "process",
        KdsSubsystem::Interrupt => "interrupt",
        KdsSubsystem::Smp => "smp",
        KdsSubsystem::Watchdog => "watchdog",
        KdsSubsystem::Syscall => "syscall",
        KdsSubsystem::Driver => "driver",
        KdsSubsystem::Network => "network",
        KdsSubsystem::Storage => "storage",
        KdsSubsystem::Security => "security",
        KdsSubsystem::Shell => "shell",
        KdsSubsystem::Override => "override",
        KdsSubsystem::Reliability => "reliability",
        KdsSubsystem::Ipc => "ipc",
    }
}

pub fn event_type_name(event_type: KdsEventType) -> &'static str {
    match event_type {
        KdsEventType::Boot => "boot",
        KdsEventType::ContextSwitch => "context_switch",
        KdsEventType::TaskCreate => "task_create",
        KdsEventType::TaskExit => "task_exit",
        KdsEventType::TaskBlock => "task_block",
        KdsEventType::TaskUnblock => "task_unblock",
        KdsEventType::CpuIdle => "cpu_idle",
        KdsEventType::CpuOnline => "cpu_online",
        KdsEventType::CpuOffline => "cpu_offline",
        KdsEventType::IpiSend => "ipi_send",
        KdsEventType::IpiReceive => "ipi_receive",
        KdsEventType::PageAlloc => "page_alloc",
        KdsEventType::PageFree => "page_free",
        KdsEventType::CowFault => "cow_fault",
        KdsEventType::PageFault => "page_fault",
        KdsEventType::Mmap => "mmap",
        KdsEventType::Munmap => "munmap",
        KdsEventType::Mprotect => "mprotect",
        KdsEventType::FileOpen => "file_open",
        KdsEventType::FileClose => "file_close",
        KdsEventType::FileRead => "file_read",
        KdsEventType::FileWrite => "file_write",
        KdsEventType::Mount => "mount",
        KdsEventType::Unmount => "unmount",
        KdsEventType::Fork => "fork",
        KdsEventType::Execve => "execve",
        KdsEventType::Exit => "exit",
        KdsEventType::Wait => "wait",
        KdsEventType::Signal => "signal",
        KdsEventType::InterruptEnter => "interrupt_enter",
        KdsEventType::InterruptExit => "interrupt_exit",
        KdsEventType::Fault => "fault",
        KdsEventType::Exception => "exception",
        KdsEventType::WatchdogCpuStall => "watchdog_cpu_stall",
        KdsEventType::SchedulerStall => "scheduler_stall",
        KdsEventType::LockContention => "lock_contention",
        KdsEventType::LockTimeout => "lock_timeout",
        KdsEventType::Metric => "metric",
        KdsEventType::TraceBegin => "trace_begin",
        KdsEventType::TraceEnd => "trace_end",
        KdsEventType::Object => "object",
        KdsEventType::State => "state",
        KdsEventType::HardwareScanBegin => "hardware_scan_begin",
        KdsEventType::HardwareScanComplete => "hardware_scan_complete",
        KdsEventType::CompatibilityPass => "compatibility_pass",
        KdsEventType::CompatibilityWarning => "compatibility_warning",
        KdsEventType::CompatibilityFailure => "compatibility_failure",
        KdsEventType::InstallApproved => "install_approved",
        KdsEventType::InstallAdvisory => "install_advisory",
        KdsEventType::DiskOperationBegin => "disk_operation_begin",
        KdsEventType::DiskOperationProgress => "disk_operation_progress",
        KdsEventType::DiskOperationComplete => "disk_operation_complete",
        KdsEventType::DiskOperationFailure => "disk_operation_failure",
        KdsEventType::DiskOperationRollback => "disk_operation_rollback",
        KdsEventType::BootRepairBegin => "boot_repair_begin",
        KdsEventType::BootRepairComplete => "boot_repair_complete",
        KdsEventType::RecoveryBegin => "recovery_begin",
        KdsEventType::RecoveryComplete => "recovery_complete",
        KdsEventType::OverrideRequest => "override_request",
        KdsEventType::OverrideApproved => "override_approved",
        KdsEventType::OverrideExecuting => "override_executing",
        KdsEventType::OverrideComplete => "override_complete",
        KdsEventType::OverrideFailed => "override_failed",
        KdsEventType::OverrideAborted => "override_aborted",
        KdsEventType::BootKdsReady => "boot_kds_ready",
        KdsEventType::KdsOverflow => "kds_overflow",
        KdsEventType::KdsCriticalLoss => "kds_critical_loss",
        KdsEventType::ResourceQuotaExceeded => "resource_quota_exceeded",
        KdsEventType::QuotaChanged => "quota_changed",
        KdsEventType::AccountingAttributionFailure => "accounting_attribution_failure",
        KdsEventType::AccountingInvariantViolated => "accounting_invariant_violated",
        KdsEventType::ResourceAccountPeriod => "resource_account_period",
        KdsEventType::BootGatePassed => "boot_gate_passed",
        KdsEventType::BootGateFailed => "boot_gate_failed",
        KdsEventType::BootComplete => "boot_complete",
        KdsEventType::SecuritySyscallDenied => "security_syscall_denied",
        KdsEventType::SecurityPrivilegeEscalation => "security_privilege_escalation",
        KdsEventType::SecurityNamespaceEscape => "security_namespace_escape",
        KdsEventType::SecurityMacDenied => "security_mac_denied",
        KdsEventType::SecurityAuditExec => "security_audit_exec",
        KdsEventType::SecurityNetworkPolicyDeny => "security_network_policy_deny",
        KdsEventType::RedRingEntered => "red_ring_entered",
        KdsEventType::RedRingSealed => "red_ring_sealed",
        KdsEventType::ContractViolation => "contract_violation",
        KdsEventType::LockOrderViolation => "lock_order_violation",
        KdsEventType::NumaKdsSegment => "numa_kds_segment",
        KdsEventType::FrNodeAssignment => "fr_node_assignment",
        KdsEventType::FlightRecorderCriticalAck => "flight_recorder_critical_ack",
        KdsEventType::FlightRecorderFinalSeal => "flight_recorder_final_seal",
        KdsEventType::FlightRecorderSealFailure => "flight_recorder_seal_failure",
        KdsEventType::SchedulerStarvation => "scheduler_starvation",
        KdsEventType::IrqStorm => "irq_storm",
        KdsEventType::IpcPipeCreate => "ipc_pipe_create",
        KdsEventType::FutexContention => "futex_contention",
        KdsEventType::TestStart => "test_start",
        KdsEventType::TestStep => "test_step",
        KdsEventType::TestPass => "test_pass",
        KdsEventType::TestFail => "test_fail",
        KdsEventType::TestTimeout => "test_timeout",
    }
}

pub fn event_category(event_type: KdsEventType) -> KdsEventCategory {
    match event_type {
        KdsEventType::Boot
        | KdsEventType::BootKdsReady
        | KdsEventType::BootGatePassed
        | KdsEventType::BootGateFailed
        | KdsEventType::BootComplete => KdsEventCategory::Boot,
        KdsEventType::TaskCreate
        | KdsEventType::TaskExit
        | KdsEventType::TaskBlock
        | KdsEventType::TaskUnblock
        | KdsEventType::Fork
        | KdsEventType::Execve
        | KdsEventType::Exit
        | KdsEventType::Wait
        | KdsEventType::Signal => KdsEventCategory::Process,
        KdsEventType::PageAlloc
        | KdsEventType::PageFree
        | KdsEventType::CowFault
        | KdsEventType::PageFault
        | KdsEventType::Mmap
        | KdsEventType::Munmap
        | KdsEventType::Mprotect => KdsEventCategory::Memory,
        KdsEventType::ContextSwitch
        | KdsEventType::CpuIdle
        | KdsEventType::CpuOnline
        | KdsEventType::CpuOffline
        | KdsEventType::SchedulerStall
        | KdsEventType::LockContention
        | KdsEventType::LockTimeout => KdsEventCategory::Scheduler,
        KdsEventType::FileOpen
        | KdsEventType::FileClose
        | KdsEventType::FileRead
        | KdsEventType::FileWrite
        | KdsEventType::Mount
        | KdsEventType::Unmount => KdsEventCategory::Filesystem,
        KdsEventType::IpiSend
        | KdsEventType::IpiReceive
        | KdsEventType::InterruptEnter
        | KdsEventType::InterruptExit
        | KdsEventType::Fault
        | KdsEventType::Exception
        | KdsEventType::WatchdogCpuStall => KdsEventCategory::Hardware,
        KdsEventType::Metric
        | KdsEventType::TraceBegin
        | KdsEventType::TraceEnd
        | KdsEventType::Object
        | KdsEventType::State
        | KdsEventType::KdsOverflow
        | KdsEventType::KdsCriticalLoss => KdsEventCategory::KdsSelf,
        KdsEventType::HardwareScanBegin
        | KdsEventType::HardwareScanComplete
        | KdsEventType::CompatibilityPass
        | KdsEventType::CompatibilityWarning
        | KdsEventType::CompatibilityFailure => KdsEventCategory::Driver,
        KdsEventType::InstallApproved
        | KdsEventType::InstallAdvisory
        | KdsEventType::DiskOperationBegin
        | KdsEventType::DiskOperationProgress
        | KdsEventType::DiskOperationComplete
        | KdsEventType::DiskOperationFailure
        | KdsEventType::DiskOperationRollback => KdsEventCategory::Storage,
        KdsEventType::BootRepairBegin
        | KdsEventType::BootRepairComplete
        | KdsEventType::RecoveryBegin
        | KdsEventType::RecoveryComplete
        | KdsEventType::OverrideRequest
        | KdsEventType::OverrideApproved
        | KdsEventType::OverrideExecuting
        | KdsEventType::OverrideComplete
        | KdsEventType::OverrideFailed
        | KdsEventType::OverrideAborted => KdsEventCategory::Override,
        KdsEventType::ResourceQuotaExceeded
        | KdsEventType::QuotaChanged
        | KdsEventType::AccountingAttributionFailure
        | KdsEventType::AccountingInvariantViolated
        | KdsEventType::ResourceAccountPeriod => KdsEventCategory::Accounting,
        KdsEventType::SecuritySyscallDenied
        | KdsEventType::SecurityPrivilegeEscalation
        | KdsEventType::SecurityNamespaceEscape
        | KdsEventType::SecurityMacDenied
        | KdsEventType::SecurityAuditExec
        | KdsEventType::SecurityNetworkPolicyDeny => KdsEventCategory::Security,
        KdsEventType::RedRingEntered
        | KdsEventType::RedRingSealed
        | KdsEventType::ContractViolation
        | KdsEventType::LockOrderViolation => KdsEventCategory::Reliability,
        KdsEventType::NumaKdsSegment | KdsEventType::FrNodeAssignment => KdsEventCategory::Numa,
        KdsEventType::FlightRecorderCriticalAck
        | KdsEventType::FlightRecorderFinalSeal
        | KdsEventType::FlightRecorderSealFailure => KdsEventCategory::KdsSelf,
        KdsEventType::SchedulerStarvation => KdsEventCategory::Scheduler,
        KdsEventType::IrqStorm => KdsEventCategory::Hardware,
        KdsEventType::IpcPipeCreate
        | KdsEventType::FutexContention
        | KdsEventType::TestStart
        | KdsEventType::TestStep
        | KdsEventType::TestPass
        | KdsEventType::TestFail
        | KdsEventType::TestTimeout => KdsEventCategory::Process,
    }
}

const ALL_KDS_EVENT_TYPES: [KdsEventType; 99] = [
    KdsEventType::Boot,
    KdsEventType::ContextSwitch,
    KdsEventType::TaskCreate,
    KdsEventType::TaskExit,
    KdsEventType::TaskBlock,
    KdsEventType::TaskUnblock,
    KdsEventType::CpuIdle,
    KdsEventType::CpuOnline,
    KdsEventType::CpuOffline,
    KdsEventType::IpiSend,
    KdsEventType::IpiReceive,
    KdsEventType::PageAlloc,
    KdsEventType::PageFree,
    KdsEventType::CowFault,
    KdsEventType::PageFault,
    KdsEventType::Mmap,
    KdsEventType::Munmap,
    KdsEventType::Mprotect,
    KdsEventType::FileOpen,
    KdsEventType::FileClose,
    KdsEventType::FileRead,
    KdsEventType::FileWrite,
    KdsEventType::Mount,
    KdsEventType::Unmount,
    KdsEventType::Fork,
    KdsEventType::Execve,
    KdsEventType::Exit,
    KdsEventType::Wait,
    KdsEventType::Signal,
    KdsEventType::InterruptEnter,
    KdsEventType::InterruptExit,
    KdsEventType::Fault,
    KdsEventType::Exception,
    KdsEventType::WatchdogCpuStall,
    KdsEventType::SchedulerStall,
    KdsEventType::LockContention,
    KdsEventType::LockTimeout,
    KdsEventType::Metric,
    KdsEventType::TraceBegin,
    KdsEventType::TraceEnd,
    KdsEventType::Object,
    KdsEventType::State,
    KdsEventType::HardwareScanBegin,
    KdsEventType::HardwareScanComplete,
    KdsEventType::CompatibilityPass,
    KdsEventType::CompatibilityWarning,
    KdsEventType::CompatibilityFailure,
    KdsEventType::InstallApproved,
    KdsEventType::InstallAdvisory,
    KdsEventType::DiskOperationBegin,
    KdsEventType::DiskOperationProgress,
    KdsEventType::DiskOperationComplete,
    KdsEventType::DiskOperationFailure,
    KdsEventType::DiskOperationRollback,
    KdsEventType::BootRepairBegin,
    KdsEventType::BootRepairComplete,
    KdsEventType::RecoveryBegin,
    KdsEventType::RecoveryComplete,
    KdsEventType::OverrideRequest,
    KdsEventType::OverrideApproved,
    KdsEventType::OverrideExecuting,
    KdsEventType::OverrideComplete,
    KdsEventType::OverrideFailed,
    KdsEventType::OverrideAborted,
    KdsEventType::BootKdsReady,
    KdsEventType::KdsOverflow,
    KdsEventType::KdsCriticalLoss,
    KdsEventType::ResourceQuotaExceeded,
    KdsEventType::QuotaChanged,
    KdsEventType::AccountingAttributionFailure,
    KdsEventType::AccountingInvariantViolated,
    KdsEventType::ResourceAccountPeriod,
    KdsEventType::BootGatePassed,
    KdsEventType::BootGateFailed,
    KdsEventType::BootComplete,
    KdsEventType::SecuritySyscallDenied,
    KdsEventType::SecurityPrivilegeEscalation,
    KdsEventType::SecurityNamespaceEscape,
    KdsEventType::SecurityMacDenied,
    KdsEventType::SecurityAuditExec,
    KdsEventType::SecurityNetworkPolicyDeny,
    KdsEventType::RedRingEntered,
    KdsEventType::RedRingSealed,
    KdsEventType::ContractViolation,
    KdsEventType::LockOrderViolation,
    KdsEventType::NumaKdsSegment,
    KdsEventType::FrNodeAssignment,
    KdsEventType::FlightRecorderCriticalAck,
    KdsEventType::FlightRecorderFinalSeal,
    KdsEventType::FlightRecorderSealFailure,
    KdsEventType::SchedulerStarvation,
    KdsEventType::IrqStorm,
    KdsEventType::IpcPipeCreate,
    KdsEventType::FutexContention,
    KdsEventType::TestStart,
    KdsEventType::TestStep,
    KdsEventType::TestPass,
    KdsEventType::TestFail,
    KdsEventType::TestTimeout,
];

pub fn event_descriptor(event_type: KdsEventType) -> KdsEventDescriptor {
    KdsEventDescriptor {
        event_type,
        name: event_type_name(event_type),
        owner: event_owner(event_type),
        category: event_category(event_type),
        baseline_severity: event_baseline_severity(event_type),
    }
}

pub fn registered_event_types() -> &'static [KdsEventType] {
    &ALL_KDS_EVENT_TYPES
}

pub fn event_owner(event_type: KdsEventType) -> KdsSubsystem {
    match event_type {
        KdsEventType::ContextSwitch
        | KdsEventType::CpuIdle
        | KdsEventType::CpuOnline
        | KdsEventType::CpuOffline
        | KdsEventType::SchedulerStall
        | KdsEventType::LockContention
        | KdsEventType::LockTimeout => KdsSubsystem::Scheduler,
        KdsEventType::TaskCreate
        | KdsEventType::TaskExit
        | KdsEventType::TaskBlock
        | KdsEventType::TaskUnblock
        | KdsEventType::Fork
        | KdsEventType::Execve
        | KdsEventType::Exit
        | KdsEventType::Wait
        | KdsEventType::Signal => KdsSubsystem::Process,
        KdsEventType::PageAlloc
        | KdsEventType::PageFree
        | KdsEventType::CowFault
        | KdsEventType::PageFault
        | KdsEventType::Mmap
        | KdsEventType::Munmap
        | KdsEventType::Mprotect => KdsSubsystem::Memory,
        KdsEventType::FileOpen
        | KdsEventType::FileClose
        | KdsEventType::FileRead
        | KdsEventType::FileWrite
        | KdsEventType::Mount
        | KdsEventType::Unmount => KdsSubsystem::Vfs,
        KdsEventType::IpiSend
        | KdsEventType::IpiReceive
        | KdsEventType::InterruptEnter
        | KdsEventType::InterruptExit
        | KdsEventType::Fault
        | KdsEventType::Exception => KdsSubsystem::Interrupt,
        KdsEventType::HardwareScanBegin
        | KdsEventType::HardwareScanComplete
        | KdsEventType::CompatibilityPass
        | KdsEventType::CompatibilityWarning
        | KdsEventType::CompatibilityFailure => KdsSubsystem::Driver,
        KdsEventType::InstallApproved
        | KdsEventType::InstallAdvisory
        | KdsEventType::DiskOperationBegin
        | KdsEventType::DiskOperationProgress
        | KdsEventType::DiskOperationComplete
        | KdsEventType::DiskOperationFailure
        | KdsEventType::DiskOperationRollback => KdsSubsystem::Storage,
        KdsEventType::BootRepairBegin
        | KdsEventType::BootRepairComplete
        | KdsEventType::RecoveryBegin
        | KdsEventType::RecoveryComplete
        | KdsEventType::OverrideRequest
        | KdsEventType::OverrideApproved
        | KdsEventType::OverrideExecuting
        | KdsEventType::OverrideComplete
        | KdsEventType::OverrideFailed
        | KdsEventType::OverrideAborted => KdsSubsystem::Override,
        KdsEventType::ResourceQuotaExceeded
        | KdsEventType::QuotaChanged
        | KdsEventType::AccountingAttributionFailure
        | KdsEventType::AccountingInvariantViolated
        | KdsEventType::ResourceAccountPeriod => KdsSubsystem::Kernel,
        KdsEventType::SecuritySyscallDenied
        | KdsEventType::SecurityPrivilegeEscalation
        | KdsEventType::SecurityNamespaceEscape
        | KdsEventType::SecurityMacDenied
        | KdsEventType::SecurityAuditExec
        | KdsEventType::SecurityNetworkPolicyDeny => KdsSubsystem::Security,
        KdsEventType::RedRingEntered
        | KdsEventType::RedRingSealed
        | KdsEventType::ContractViolation
        | KdsEventType::LockOrderViolation
        | KdsEventType::FlightRecorderCriticalAck
        | KdsEventType::FlightRecorderFinalSeal
        | KdsEventType::FlightRecorderSealFailure => KdsSubsystem::Reliability,
        KdsEventType::NumaKdsSegment | KdsEventType::FrNodeAssignment => KdsSubsystem::Smp,
        KdsEventType::WatchdogCpuStall => KdsSubsystem::Watchdog,
        KdsEventType::Boot
        | KdsEventType::BootKdsReady
        | KdsEventType::BootGatePassed
        | KdsEventType::BootGateFailed
        | KdsEventType::BootComplete
        | KdsEventType::Metric
        | KdsEventType::TraceBegin
        | KdsEventType::TraceEnd
        | KdsEventType::Object
        | KdsEventType::State
        | KdsEventType::KdsOverflow
        | KdsEventType::KdsCriticalLoss => KdsSubsystem::Kernel,
        KdsEventType::SchedulerStarvation => KdsSubsystem::Scheduler,
        KdsEventType::IrqStorm => KdsSubsystem::Interrupt,
        KdsEventType::IpcPipeCreate | KdsEventType::FutexContention => KdsSubsystem::Ipc,
        KdsEventType::TestStart
        | KdsEventType::TestStep
        | KdsEventType::TestPass
        | KdsEventType::TestFail
        | KdsEventType::TestTimeout => KdsSubsystem::Shell,
    }
}

pub fn event_baseline_severity(event_type: KdsEventType) -> KdsSeverity {
    match event_type {
        KdsEventType::BootGateFailed
        | KdsEventType::KdsCriticalLoss
        | KdsEventType::SecuritySyscallDenied
        | KdsEventType::SecurityPrivilegeEscalation
        | KdsEventType::SecurityNamespaceEscape
        | KdsEventType::SecurityMacDenied
        | KdsEventType::SecurityNetworkPolicyDeny
        | KdsEventType::RedRingEntered
        | KdsEventType::RedRingSealed
        | KdsEventType::ContractViolation
        | KdsEventType::AccountingInvariantViolated
        | KdsEventType::FlightRecorderSealFailure => KdsSeverity::Fatal,
        KdsEventType::CompatibilityFailure
        | KdsEventType::DiskOperationFailure
        | KdsEventType::OverrideFailed
        | KdsEventType::SchedulerStall
        | KdsEventType::WatchdogCpuStall
        | KdsEventType::Fault
        | KdsEventType::Exception
        | KdsEventType::AccountingAttributionFailure => KdsSeverity::Error,
        KdsEventType::CompatibilityWarning
        | KdsEventType::OverrideAborted
        | KdsEventType::KdsOverflow
        | KdsEventType::ResourceQuotaExceeded
        | KdsEventType::LockContention
        | KdsEventType::LockTimeout
        | KdsEventType::FrNodeAssignment
        | KdsEventType::SchedulerStarvation
        | KdsEventType::IrqStorm => KdsSeverity::Warn,
        KdsEventType::ContextSwitch
        | KdsEventType::CpuIdle
        | KdsEventType::PageAlloc
        | KdsEventType::PageFree
        | KdsEventType::InterruptEnter
        | KdsEventType::InterruptExit
        | KdsEventType::Metric
        | KdsEventType::TraceBegin
        | KdsEventType::TraceEnd => KdsSeverity::Trace,
        _ => KdsSeverity::Info,
    }
}

pub fn validate_event_taxonomy() -> bool {
    let events = registered_event_types();
    let mut idx = 0usize;
    while idx < events.len() {
        let descriptor = event_descriptor(events[idx]);
        if descriptor.name.is_empty()
            || descriptor.category.name().is_empty()
            || subsystem_name(descriptor.owner).is_empty()
            || descriptor.baseline_severity.schema_name().is_empty()
        {
            return false;
        }
        let mut other = idx + 1;
        while other < events.len() {
            if descriptor.name == event_type_name(events[other]) {
                return false;
            }
            other += 1;
        }
        idx += 1;
    }
    true
}

pub fn metric_name(metric_id: KdsMetricId) -> &'static str {
    match metric_id {
        KdsMetricId::CpuHeartbeat => "cpu_heartbeat",
        KdsMetricId::SchedulerProgress => "scheduler_progress",
        KdsMetricId::PageAlloc => "page_alloc",
        KdsMetricId::PageFree => "page_free",
        KdsMetricId::MmapBytes => "mmap_bytes",
        KdsMetricId::MunmapBytes => "munmap_bytes",
        KdsMetricId::WatchdogStallMs => "watchdog_stall_ms",
        KdsMetricId::ContextSwitches => "context_switches",
        KdsMetricId::Interrupts => "interrupts",
        KdsMetricId::Faults => "faults",
    }
}

pub fn object_kind_name(kind: KdsObjectKind) -> &'static str {
    match kind {
        KdsObjectKind::Process => "process",
        KdsObjectKind::Thread => "thread",
        KdsObjectKind::File => "file",
        KdsObjectKind::Socket => "socket",
        KdsObjectKind::Driver => "driver",
        KdsObjectKind::Device => "device",
        KdsObjectKind::Mount => "mount",
        KdsObjectKind::User => "user",
        KdsObjectKind::Cpu => "cpu",
    }
}

pub fn note_cpu_heartbeat(cpu: usize, pid: u32) {
    let _ = pid;
    obs_gauge(
        KdsSubsystem::Watchdog,
        KdsMetricId::CpuHeartbeat,
        cpu as u64,
    );
}

pub fn note_scheduler_progress(cpu: usize, pid: u32) {
    let _ = (cpu, pid);
    obs_counter(KdsSubsystem::Scheduler, KdsMetricId::SchedulerProgress, 1);
}

pub fn validate_architecture() -> KdsValidationReport {
    flush_aggregates();
    let before = stats();
    let trace_start = crate::time::uptime_ns();
    let trace_id = kds_trace_begin(KdsSubsystem::Kernel, 0);
    kds_event(
        KdsSubsystem::Kernel,
        KdsEventType::State,
        KdsSeverity::Info,
        [
            trace_id,
            before.events.records,
            before.metrics.records,
            before.state.records,
        ],
    );
    kds_metric_for(
        KdsSubsystem::Kernel,
        KdsMetricId::CpuHeartbeat,
        1,
        crate::process::current_pid().unwrap_or(0),
        crate::process::current_pid().unwrap_or(0),
        [trace_id, 0],
    );
    kds_trace_end(KdsSubsystem::Kernel, trace_id, 0, trace_start);
    kds_object(
        KdsObjectKind::Cpu,
        0,
        [crate::process::table::cpu_idx() as u64, trace_id],
    );
    kds_state(
        KdsSubsystem::Kernel,
        trace_id,
        1,
        KdsSeverity::Info,
        [trace_id, 0],
    );
    flush_aggregates();
    let after = stats();

    let mut attribution_present = false;
    let mut schema_bearing_events = false;
    for_each_event(64, |record| {
        if record.event_id != 0
            && record.metadata.timestamp != 0
            && record.metadata.event_type == KdsEventType::State
        {
            attribution_present = true;
        }
        if record.event_id != 0
            && record.schema_version == KDS_SCHEMA_VERSION
            && record.schema_flags & KDS_SCHEMA_REQUIRED_FLAGS == KDS_SCHEMA_REQUIRED_FLAGS
            && record.event_uuid != [0; 2]
            && !record.metadata.severity.schema_name().is_empty()
            && !event_category(record.metadata.event_type).name().is_empty()
            && !subsystem_name(record.metadata.subsystem).is_empty()
            && record.context_tag_count <= record.context_tags.len() as u16
        {
            schema_bearing_events = true;
        }
    });

    KdsValidationReport {
        event_creation: after.events.records > before.events.records,
        schema_bearing_events,
        metric_creation: after.metrics.records > before.metrics.records,
        trace_creation: after.traces.records >= before.traces.records.saturating_add(2),
        object_creation: after.objects.records > before.objects.records,
        state_update: after.state.records > before.state.records,
        stream_integrity: after.events.record_size == core::mem::size_of::<EventRecord>()
            && after.metrics.record_size == core::mem::size_of::<MetricRecord>()
            && after.traces.record_size == core::mem::size_of::<TraceRecord>()
            && after.objects.record_size == core::mem::size_of::<ObjectRecord>()
            && after.state.record_size == core::mem::size_of::<StateRecord>()
            && after.events.capacity > 0
            && after.metrics.capacity > 0
            && after.traces.capacity > 0
            && after.objects.capacity > 0
            && after.state.capacity > 0,
        buffer_accounting: after.events.records <= after.events.capacity as u64
            && after.metrics.records <= after.metrics.capacity as u64
            && after.traces.records <= after.traces.capacity as u64
            && after.objects.records <= after.objects.capacity as u64
            && after.state.records <= after.state.capacity as u64,
        drop_accounting: after.events.dropped >= before.events.dropped
            && after.metrics.dropped >= before.metrics.dropped
            && after.traces.dropped >= before.traces.dropped
            && after.objects.dropped >= before.objects.dropped
            && after.state.dropped >= before.state.dropped,
        attribution_present,
        taxonomy_coverage: validate_event_taxonomy(),
    }
}

fn metadata_for(
    subsystem: KdsSubsystem,
    event_type: KdsEventType,
    severity: KdsSeverity,
    pid: u32,
    tid: u32,
) -> KdsMetadata {
    KdsMetadata {
        timestamp: crate::time::uptime_ns(),
        cpu_id: crate::process::table::cpu_idx() as u32,
        thread_id: tid,
        process_id: pid,
        subsystem,
        event_type,
        severity,
    }
}

fn event_uuid_for(event_id: u64, metadata: &KdsMetadata) -> [u64; 2] {
    let timestamp = metadata.timestamp.max(1);
    let uuid_high = (0x7u64 << 60) | (timestamp & 0x0fff_ffff_ffff_ffff);
    let uuid_low = ((metadata.cpu_id as u64) << 48)
        | ((metadata.process_id as u64) << 16)
        | (event_id & 0xffff);
    [uuid_high, uuid_low]
}

fn schema_flags_for(metadata: &KdsMetadata, context_tag_count: u16, _shape: &KdsEventShape) -> u32 {
    let mut flags = KDS_SCHEMA_FLAG_VERSION
        | KDS_SCHEMA_FLAG_UUID_V7
        | KDS_SCHEMA_FLAG_SOURCE_CONTRACT
        | KDS_SCHEMA_FLAG_SEVERITY_VOCABULARY
        | KDS_SCHEMA_FLAG_EVENT_CATEGORY
        | KDS_SCHEMA_FLAG_TYPED_SHAPE;
    if context_tag_count <= 8 {
        flags |= KDS_SCHEMA_FLAG_CONTEXT_TAGS;
    }
    let _ = metadata.severity.schema_name();
    let _ = event_category(metadata.event_type);
    flags
}

fn current_process_thread() -> (u32, u32) {
    let pid = crate::process::table::TABLE
        .try_lock()
        .map(|table| table.current_pid())
        .unwrap_or(0);
    (pid, pid)
}

fn align_up_u64(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}

fn align_down_u64(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}

fn ranges_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

#[derive(Clone, Copy)]
enum AggregateMode {
    Counter,
    Gauge,
    Histogram,
}

fn update_aggregate(
    subsystem: KdsSubsystem,
    metric_id: KdsMetricId,
    value: u64,
    mode: AggregateMode,
) {
    let Some(mut aggregates) = AGGREGATES.try_lock() else {
        return;
    };
    let mut empty = None;
    let mut target = None;
    for (idx, slot) in aggregates.iter().enumerate() {
        if slot.used && slot.subsystem == subsystem && slot.metric_id == metric_id {
            target = Some(idx);
            break;
        }
        if !slot.used && empty.is_none() {
            empty = Some(idx);
        }
    }

    let Some(idx) = target.or(empty) else {
        return;
    };
    let slot = &mut aggregates[idx];
    if !slot.used {
        slot.used = true;
        slot.subsystem = subsystem;
        slot.metric_id = metric_id;
        slot.min = value;
        slot.max = value;
    }
    match mode {
        AggregateMode::Counter => {
            slot.count = slot.count.saturating_add(1);
            slot.sum = slot.sum.saturating_add(value);
            slot.last = slot.sum;
        }
        AggregateMode::Gauge => {
            slot.count = slot.count.saturating_add(1);
            slot.sum = value;
            slot.last = value;
        }
        AggregateMode::Histogram => {
            slot.count = slot.count.saturating_add(1);
            slot.sum = slot.sum.saturating_add(value);
            slot.last = value;
        }
    }
    if slot.count == 1 || value < slot.min {
        slot.min = value;
    }
    if value > slot.max {
        slot.max = value;
    }
}

#[macro_export]
macro_rules! OBS_COUNTER {
    ($subsystem:expr, $metric_id:expr, $delta:expr $(,)?) => {{
        $crate::observability_contract::ObservabilityContract::obs_counter(
            $subsystem, $metric_id, $delta,
        )
    }};
}

#[macro_export]
macro_rules! OBS_GAUGE {
    ($subsystem:expr, $metric_id:expr, $value:expr $(,)?) => {{
        $crate::observability_contract::ObservabilityContract::obs_gauge(
            $subsystem, $metric_id, $value,
        )
    }};
}

#[macro_export]
macro_rules! OBS_HISTOGRAM {
    ($subsystem:expr, $metric_id:expr, $sample:expr $(,)?) => {{
        $crate::observability_contract::ObservabilityContract::obs_histogram(
            $subsystem, $metric_id, $sample,
        )
    }};
}
