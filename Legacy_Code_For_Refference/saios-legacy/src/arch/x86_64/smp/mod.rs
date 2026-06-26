//! x86_64 symmetric multi-processing (SMP) — bring application processors online.
//!
//! Pipeline:
//!   1. Parse the ACPI MADT ("APIC" table) to enumerate every CPU's local-APIC ID.
//!   2. Enable the Local APIC on the boot processor (BSP).
//!   3. Copy the real-mode AP trampoline (smp_trampoline.s) to physical 0x8000.
//!   4. For each AP: allocate a kernel stack, patch the trampoline, and send
//!      INIT-SIPI-SIPI via the LAPIC ICR.  The AP walks real→protected→long mode
//!      on the kernel PML4 and calls `ap_entry`.
//!
//! Everything is gated behind a hard timeout: if an AP does not check in, the
//! BSP logs the count and continues — a failed bringup can never brick boot.
//!
//! Each online CPU owns a scheduler `current` slot and idle thread.  BSP
//! preemption is driven by the PIT; AP preemption is driven by each CPU's LAPIC
//! timer, so runnable non-pinned threads can execute across the online mask.

use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

unsafe extern "C" {
    static ap_trampoline_start: u8;
    static ap_trampoline_end: u8;
    static ap_tramp_cr3: u8;
    static ap_tramp_entry: u8;
    static ap_tramp_stack: u8;
}

/// Physical address the trampoline is copied to (SIPI vector 0x08).
const TRAMPOLINE_PHYS: u64 = 0x8000;
const SIPI_VECTOR: u32 = (TRAMPOLINE_PHYS >> 12) as u32; // 0x08
const AP_INITIALIZE_WAIT_NS: u64 = 100_000_000;
const AP_SCHEDULER_VISIBLE_WAIT_NS: u64 = 1_000_000_000;

static CPU_COUNT: AtomicU32 = AtomicU32::new(1);
static AP_STARTED_MASK: AtomicU64 = AtomicU64::new(1); // bit i set => AP reached ap_entry
static INITIALIZED_MASK: AtomicU64 = AtomicU64::new(1); // bit i set => per-CPU setup complete
static AP_ACCEPTED_MASK: AtomicU64 = AtomicU64::new(1); // bit i set => BSP admitted AP to scheduler
static AP_STARTUP_DONE: AtomicBool = AtomicBool::new(false);
static AP_SCHEDULER_RELEASED: AtomicBool = AtomicBool::new(false);
static SCHEDULER_VISIBLE_MASK: AtomicU64 = AtomicU64::new(1); // bit i set => current[] + idle[] valid
static LAPIC_BASE: AtomicU64 = AtomicU64::new(0xFEE0_0000);
static AP_STACK_TOP: AtomicU64 = AtomicU64::new(0); // handed to the next AP
static CPU_NUMA_NODE: [AtomicUsize; crate::process::table::MAX_CPUS] =
    [const { AtomicUsize::new(0) }; crate::process::table::MAX_CPUS];

fn missing_mask(expected: u64, observed: u64) -> u64 {
    expected & !observed
}

// -- MSR / LAPIC helpers -----------------------------------------------------

unsafe fn rdmsr(msr: u32) -> u64 {
    unsafe {
        let (lo, hi): (u32, u32);
        asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack));
        ((hi as u64) << 32) | lo as u64
    }
}

fn lapic_base() -> u64 {
    LAPIC_BASE.load(Ordering::Relaxed)
}

fn lapic_read(reg: u32) -> u32 {
    unsafe { core::ptr::read_volatile((lapic_base() + reg as u64) as *const u32) }
}
fn lapic_write(reg: u32, val: u32) {
    unsafe {
        core::ptr::write_volatile((lapic_base() + reg as u64) as *mut u32, val);
    }
}

const LAPIC_ID: u32 = 0x020;
const LAPIC_SVR: u32 = 0x0F0;
const LAPIC_EOI: u32 = 0x0B0;
const LAPIC_ICRL: u32 = 0x300;
const LAPIC_ICRH: u32 = 0x310;
const LAPIC_LVT_TIMER: u32 = 0x320;
const LAPIC_TIMER_INIT: u32 = 0x380;
const LAPIC_TIMER_CUR: u32 = 0x390;
const LAPIC_TIMER_DIV: u32 = 0x3E0;

/// IDT vector for the per-CPU LAPIC timer (APs use this to preempt).
pub const LAPIC_TIMER_VECTOR: u8 = 0x40;
/// LAPIC spurious-interrupt vector.
pub const LAPIC_SPURIOUS_VECTOR: u8 = 0xFF;

/// Acknowledge a LAPIC interrupt (write 0 to the EOI register).
pub fn lapic_eoi() {
    lapic_write(LAPIC_EOI, 0);
}

/// Configure this CPU's LAPIC timer as a periodic ~10 ms preemption tick,
/// delivering `LAPIC_TIMER_VECTOR`.  Calibrated against the TSC.
fn init_lapic_timer() {
    lapic_write(LAPIC_TIMER_DIV, 0x3); // divide by 16
    lapic_write(LAPIC_LVT_TIMER, 0x1_0000); // masked during calibration
    lapic_write(LAPIC_TIMER_INIT, 0xFFFF_FFFF);
    spin_for_ns_on_this_cpu(10_000_000);
    let elapsed = 0xFFFF_FFFFu32.wrapping_sub(lapic_read(LAPIC_TIMER_CUR));
    let count = elapsed.max(10_000); // ticks per 10 ms
    // Periodic (bit 17) | vector.
    lapic_write(LAPIC_LVT_TIMER, 0x2_0000 | LAPIC_TIMER_VECTOR as u32);
    lapic_write(LAPIC_TIMER_INIT, count);
}

fn spin_for_ns_on_this_cpu(ns: u64) {
    let hz = crate::time::tsc_hz();
    if hz == 0 {
        for _ in 0..1_000_000 {
            core::hint::spin_loop();
        }
        return;
    }

    let ticks = ((hz as u128 * ns as u128) / 1_000_000_000u128).max(1) as u64;
    let start = crate::time::rdtsc();
    while crate::time::rdtsc().wrapping_sub(start) < ticks {
        core::hint::spin_loop();
    }
}

/// This CPU's local-APIC ID.
pub fn lapic_id() -> u32 {
    lapic_read(LAPIC_ID) >> 24
}

const LAPIC_LVT_THERMAL: u32 = 0x330;
const LAPIC_LVT_PERF: u32 = 0x340;
const LAPIC_LVT_LINT0: u32 = 0x350;
const LAPIC_LVT_LINT1: u32 = 0x360;
const LAPIC_LVT_ERROR: u32 = 0x370;

fn enable_lapic() {
    // Mask ALL LVT entries before enabling — BIOS may have left stray vectors
    // programmed that would fire into un-configured IDT slots → #GP → #DF.
    lapic_write(LAPIC_LVT_TIMER, 0x1_0000); // masked
    lapic_write(LAPIC_LVT_THERMAL, 0x1_0000); // masked
    lapic_write(LAPIC_LVT_PERF, 0x1_0000); // masked
    lapic_write(LAPIC_LVT_LINT0, 0x1_0000); // masked
    lapic_write(LAPIC_LVT_LINT1, 0x1_0000); // masked
    lapic_write(LAPIC_LVT_ERROR, 0x1_0000); // masked
    // Set the spurious-interrupt vector register: bit 8 enables the LAPIC.
    lapic_write(LAPIC_SVR, lapic_read(LAPIC_SVR) | 0x100 | 0xFF);
}

/// Busy-wait `us` microseconds using the calibrated TSC.
fn udelay(us: u64) {
    let start = crate::time::uptime_ns();
    while crate::time::uptime_ns().wrapping_sub(start) < us * 1000 {
        core::hint::spin_loop();
    }
}

/// Send an IPI to `apic_id` with the given ICR-low command, waiting for delivery.
fn send_ipi(apic_id: u32, icr_low: u32) {
    lapic_write(LAPIC_ICRH, apic_id << 24);
    lapic_write(LAPIC_ICRL, icr_low);
    // Bit 12 = delivery status; wait until the APIC reports the IPI was sent.
    for _ in 0..1000 {
        if lapic_read(LAPIC_ICRL) & (1 << 12) == 0 {
            break;
        }
        core::hint::spin_loop();
    }
}

// -- MADT enumeration --------------------------------------------------------

/// Return the local-APIC IDs of all enabled processors (BSP first).
fn enumerate_cpus() -> Vec<u32> {
    let mut ids = Vec::new();
    let Some(madt) = crate::driver::acpi::find_table(b"APIC") else {
        return ids;
    };
    let Some(len) = crate::driver::acpi::read_u32(madt + 4) else {
        return ids;
    };
    // MADT local-APIC address at +36; entry list begins at +44.
    if let Some(base) = crate::driver::acpi::read_u32(madt + 36) {
        LAPIC_BASE.store(base as u64, Ordering::Relaxed);
    }
    let end = madt + len as u64;
    let mut p = madt + 44;
    while p + 2 <= end {
        let etype = crate::driver::acpi::read_u8(p).unwrap_or(0xFF);
        let elen = crate::driver::acpi::read_u8(p + 1).unwrap_or(0) as u64;
        if elen < 2 {
            break;
        }
        if etype == 0 {
            // Processor Local APIC: +3 apic_id, +4 flags (bit0 = enabled).
            let apic_id = crate::driver::acpi::read_u8(p + 3).unwrap_or(0) as u32;
            let flags = crate::driver::acpi::read_u32(p + 4).unwrap_or(0);
            if flags & 1 != 0 {
                ids.push(apic_id);
            }
        }
        p += elen;
    }
    ids
}

fn parse_srat_topology() {
    let Some(srat) = crate::driver::acpi::find_table(b"SRAT") else {
        crate::serial_println!("[smp] ACPI SRAT not found - NUMA topology is flat node 0");
        return;
    };
    let Some(len) = crate::driver::acpi::read_u32(srat + 4) else {
        return;
    };
    if len < 48 {
        return;
    }

    let end = srat + len as u64;
    let mut p = srat + 48;
    let mut mapped = 0usize;
    while p + 2 <= end {
        let entry_type = crate::driver::acpi::read_u8(p).unwrap_or(0xFF);
        let entry_len = crate::driver::acpi::read_u8(p + 1).unwrap_or(0) as u64;
        if entry_len < 2 || p + entry_len > end {
            break;
        }

        if entry_type == 0 && entry_len >= 16 {
            let proximity_lo = crate::driver::acpi::read_u8(p + 2).unwrap_or(0) as u32;
            let apic_id = crate::driver::acpi::read_u8(p + 3).unwrap_or(0) as usize;
            let flags = crate::driver::acpi::read_u32(p + 4).unwrap_or(0);
            let proximity_hi = crate::driver::acpi::read_u32(p + 8).unwrap_or(0) & 0x00ff_ffff;
            let proximity = proximity_lo | (proximity_hi << 8);
            if flags & 1 != 0 && apic_id < crate::process::table::MAX_CPUS {
                CPU_NUMA_NODE[apic_id].store(proximity as usize, Ordering::Relaxed);
                mapped += 1;
            }
        }
        p += entry_len;
    }

    if mapped == 0 {
        crate::serial_println!(
            "[smp] SRAT present but no enabled CPU affinity entries - NUMA topology is flat node 0"
        );
    } else {
        crate::serial_println!("[smp] SRAT CPU NUMA entries mapped: {}", mapped);
    }
}

// -- Bringup -----------------------------------------------------------------

pub fn init() {
    let cpus = enumerate_cpus();
    parse_srat_topology();
    if cpus.len() <= 1 {
        crate::serial_println!("[smp] 1 CPU (no MADT APs) — single-core");
        return;
    }
    enable_lapic();
    let bsp = lapic_id();
    crate::serial_println!(
        "[smp] {} CPUs in MADT, BSP apic_id={}, LAPIC @ {:#x}",
        cpus.len(),
        bsp,
        lapic_base()
    );

    // Copy the trampoline blob to 0x8000 (identity-mapped, writable).
    let start = unsafe { &ap_trampoline_start as *const u8 as usize };
    let end = unsafe { &ap_trampoline_end as *const u8 as usize };
    let size = end - start;
    unsafe {
        core::ptr::copy_nonoverlapping(start as *const u8, TRAMPOLINE_PHYS as *mut u8, size);
    }

    // Patch CR3 (kernel PML4 physical address) into the trampoline.
    let cr3 = crate::arch::current_page_table();
    let cr3_off = unsafe { &ap_tramp_cr3 as *const u8 as usize } - start;
    let entry_off = unsafe { &ap_tramp_entry as *const u8 as usize } - start;
    let stack_off = unsafe { &ap_tramp_stack as *const u8 as usize } - start;
    unsafe {
        core::ptr::write_volatile((TRAMPOLINE_PHYS + cr3_off as u64) as *mut u32, cr3 as u32);
        core::ptr::write_volatile(
            (TRAMPOLINE_PHYS + entry_off as u64) as *mut u64,
            ap_entry as *const () as usize as u64,
        );
    }

    for &apic_id in cpus.iter() {
        if apic_id == bsp {
            continue;
        }

        // Fresh 64 KiB kernel stack for this AP; leak it (lives for the AP's life).
        let stack = alloc::vec![0u8; 64 * 1024].leak();
        let top = stack.as_ptr() as u64 + stack.len() as u64;
        AP_STACK_TOP.store(top, Ordering::SeqCst);
        unsafe {
            core::ptr::write_volatile((TRAMPOLINE_PHYS + stack_off as u64) as *mut u64, top);
        }

        let cpu_bit = 1u64 << apic_id.min(63);

        // INIT, then two SIPIs (Intel MP startup protocol).
        send_ipi(apic_id, 0x0000_4500); // INIT assert
        udelay(10_000); // 10 ms
        send_ipi(apic_id, 0x0000_4600 | SIPI_VECTOR); // SIPI
        udelay(200);
        send_ipi(apic_id, 0x0000_4600 | SIPI_VECTOR); // SIPI (retry)

        // Wait for this AP to enter long-mode kernel code (≈100 ms), else give up on it.
        let deadline = crate::time::uptime_ns() + 100_000_000;
        while AP_STARTED_MASK.load(Ordering::SeqCst) & cpu_bit == 0
            && crate::time::uptime_ns() < deadline
        {
            core::hint::spin_loop();
        }
        if AP_STARTED_MASK.load(Ordering::SeqCst) & cpu_bit == 0 {
            crate::serial_println!("[smp] apic_id={} did not start (timeout)", apic_id);
        } else {
            AP_ACCEPTED_MASK.fetch_or(cpu_bit, Ordering::SeqCst);
        }
    }

    let accepted_mask = AP_ACCEPTED_MASK.load(Ordering::SeqCst);
    AP_STARTUP_DONE.store(true, Ordering::SeqCst);
    let started_mask = AP_STARTED_MASK.load(Ordering::SeqCst);
    let mut initialized_mask = INITIALIZED_MASK.load(Ordering::SeqCst);
    let missing_initialized = missing_mask(accepted_mask, initialized_mask);
    if missing_initialized != 0 {
        let wait_start = crate::time::uptime_ns();
        crate::serial_println!(
            "[smp] initialization lag: started={:#x} initialized={:#x} missing_initialized={:#x}; scheduler release blocked",
            started_mask,
            initialized_mask,
            missing_initialized
        );
        crate::observability_contract::ObservabilityContract::kds_event_for(
            crate::kds::KdsSubsystem::Smp,
            crate::kds::KdsEventType::SchedulerStall,
            crate::kds::KdsSeverity::Warn,
            0,
            0,
            [
                started_mask,
                initialized_mask,
                accepted_mask,
                missing_initialized,
            ],
        );

        let deadline = wait_start + AP_INITIALIZE_WAIT_NS;
        while missing_mask(accepted_mask, initialized_mask) != 0
            && crate::time::uptime_ns() < deadline
        {
            core::hint::spin_loop();
            initialized_mask = INITIALIZED_MASK.load(Ordering::SeqCst);
        }

        let elapsed_ms = crate::time::uptime_ns().wrapping_sub(wait_start) / 1_000_000;
        let remaining = missing_mask(accepted_mask, initialized_mask);
        if remaining == 0 {
            crate::serial_println!(
                "[smp] initialization lag resolved elapsed_ms={} initialized={:#x}",
                elapsed_ms,
                initialized_mask
            );
        } else {
            crate::serial_println!(
                "[smp] initialization lag unresolved elapsed_ms={} initialized={:#x} missing_initialized={:#x}",
                elapsed_ms,
                initialized_mask,
                remaining
            );
        }
        crate::observability_contract::ObservabilityContract::kds_state(
            crate::kds::KdsSubsystem::Smp,
            3,
            SCHEDULER_VISIBLE_MASK.load(Ordering::SeqCst),
            if remaining == 0 {
                crate::kds::KdsSeverity::Info
            } else {
                crate::kds::KdsSeverity::Warn
            },
            [initialized_mask, remaining],
        );
    }
    crate::observability_contract::ObservabilityContract::kds_state(
        crate::kds::KdsSubsystem::Smp,
        1,
        SCHEDULER_VISIBLE_MASK.load(Ordering::SeqCst),
        crate::kds::KdsSeverity::Info,
        [started_mask, initialized_mask],
    );
    crate::serial_println!(
        "[smp] started={:#x} initialized={:#x} scheduler_visible={:#x} accepted={:#x}; scheduler release deferred",
        started_mask,
        initialized_mask,
        SCHEDULER_VISIBLE_MASK.load(Ordering::SeqCst),
        accepted_mask
    );
}

/// Release accepted APs into scheduler-visible ownership after the BSP has a
/// registered boot/idle thread. Before this point, only the BSP is online to the
/// scheduler even if AP hardware has started successfully.
pub fn release_scheduler_cpus() {
    let accepted_mask = AP_ACCEPTED_MASK.load(Ordering::SeqCst);
    if accepted_mask == 1 {
        CPU_COUNT.store(1, Ordering::SeqCst);
        return;
    }

    AP_SCHEDULER_RELEASED.store(true, Ordering::SeqCst);
    let mut scheduler_visible_mask = SCHEDULER_VISIBLE_MASK.load(Ordering::SeqCst);
    let missing_scheduler_visible = missing_mask(accepted_mask, scheduler_visible_mask);
    if missing_scheduler_visible != 0 {
        let wait_start = crate::time::uptime_ns();
        crate::serial_println!(
            "[smp] scheduler visibility lag: initialized={:#x} scheduler_visible={:#x} missing_scheduler_visible={:#x}; AP idle registration in progress",
            INITIALIZED_MASK.load(Ordering::SeqCst),
            scheduler_visible_mask,
            missing_scheduler_visible
        );
        crate::observability_contract::ObservabilityContract::kds_event_for(
            crate::kds::KdsSubsystem::Smp,
            crate::kds::KdsEventType::SchedulerStall,
            crate::kds::KdsSeverity::Warn,
            0,
            0,
            [
                INITIALIZED_MASK.load(Ordering::SeqCst),
                scheduler_visible_mask,
                accepted_mask,
                missing_scheduler_visible,
            ],
        );

        let deadline = wait_start + AP_SCHEDULER_VISIBLE_WAIT_NS;
        while missing_mask(accepted_mask, scheduler_visible_mask) != 0
            && crate::time::uptime_ns() < deadline
        {
            core::hint::spin_loop();
            scheduler_visible_mask = SCHEDULER_VISIBLE_MASK.load(Ordering::SeqCst);
        }

        let elapsed_ms = crate::time::uptime_ns().wrapping_sub(wait_start) / 1_000_000;
        let remaining = missing_mask(accepted_mask, scheduler_visible_mask);
        if remaining == 0 {
            crate::serial_println!(
                "[smp] scheduler visibility lag resolved elapsed_ms={} scheduler_visible={:#x}",
                elapsed_ms,
                scheduler_visible_mask
            );
        } else {
            crate::serial_println!(
                "[smp] scheduler visibility lag unresolved elapsed_ms={} scheduler_visible={:#x} missing_scheduler_visible={:#x}",
                elapsed_ms,
                scheduler_visible_mask,
                remaining
            );
        }
        crate::observability_contract::ObservabilityContract::kds_state(
            crate::kds::KdsSubsystem::Smp,
            4,
            scheduler_visible_mask,
            if remaining == 0 {
                crate::kds::KdsSeverity::Info
            } else {
                crate::kds::KdsSeverity::Warn
            },
            [INITIALIZED_MASK.load(Ordering::SeqCst), remaining],
        );
    }

    let started_mask = AP_STARTED_MASK.load(Ordering::SeqCst);
    let initialized_mask = INITIALIZED_MASK.load(Ordering::SeqCst);
    let scheduler_visible_mask = SCHEDULER_VISIBLE_MASK.load(Ordering::SeqCst);
    let online = scheduler_visible_mask.count_ones();
    CPU_COUNT.store(online, Ordering::SeqCst);
    crate::observability_contract::ObservabilityContract::kds_state(
        crate::kds::KdsSubsystem::Smp,
        2,
        scheduler_visible_mask,
        crate::kds::KdsSeverity::Info,
        [started_mask, initialized_mask],
    );
    if missing_mask(accepted_mask, scheduler_visible_mask) != 0 {
        crate::serial_println!(
            "[smp] scheduler registration incomplete (accepted {:#x}, scheduler_visible {:#x})",
            accepted_mask,
            scheduler_visible_mask
        );
    }
    crate::serial_println!(
        "[smp] started={:#x} initialized={:#x} scheduler_visible={:#x}",
        started_mask,
        initialized_mask,
        scheduler_visible_mask
    );
    crate::serial_println!("[smp] {} scheduler core(s) online", online,);
}

/// 64-bit entry for an application processor (jumped to from the trampoline).
#[unsafe(no_mangle)]
pub extern "C" fn ap_entry() -> ! {
    // We are in long mode on the kernel PML4, on our own stack.
    crate::gdt::load_on_ap();
    crate::interrupts::load_idt_on_ap();

    let id = lapic_id();
    let cpu_bit = 1u64 << id.min(63);
    AP_STARTED_MASK.fetch_or(cpu_bit, Ordering::SeqCst);

    while AP_ACCEPTED_MASK.load(Ordering::SeqCst) & cpu_bit == 0 {
        if AP_STARTUP_DONE.load(Ordering::SeqCst)
            && AP_ACCEPTED_MASK.load(Ordering::SeqCst) & cpu_bit == 0
        {
            loop {
                crate::arch::halt();
            }
        }
        core::hint::spin_loop();
    }

    enable_lapic();
    crate::syscall::init();
    INITIALIZED_MASK.fetch_or(cpu_bit, Ordering::SeqCst);

    while !AP_SCHEDULER_RELEASED.load(Ordering::SeqCst) {
        core::hint::spin_loop();
    }

    // Register this AP's idle thread (becomes current[cpu]) so the scheduler has
    // a valid context to save our stack into, then start the preemption timer.
    crate::process::scheduler::register_ap_idle();
    init_lapic_timer();

    let scheduler_visible_mask =
        SCHEDULER_VISIBLE_MASK.fetch_or(cpu_bit, Ordering::SeqCst) | cpu_bit;
    CPU_COUNT.store(scheduler_visible_mask.count_ones(), Ordering::SeqCst);
    crate::observability_contract::ObservabilityContract::kds_event_for(
        crate::kds::KdsSubsystem::Smp,
        crate::kds::KdsEventType::CpuOnline,
        crate::kds::KdsSeverity::Info,
        0,
        0,
        [
            id as u64,
            cpu_bit,
            INITIALIZED_MASK.load(Ordering::SeqCst),
            scheduler_visible_mask,
        ],
    );
    crate::observability_contract::ObservabilityContract::kds_object(
        crate::kds::KdsObjectKind::Cpu,
        0,
        [id as u64, cpu_bit],
    );

    // Idle: the LAPIC timer IRQ drives schedule(), which pulls runnable
    // (non-BSP-pinned) threads from the shared run queue onto this core.
    crate::arch::enable_interrupts();
    loop {
        crate::arch::halt();
    }
}

/// Number of CPUs currently online (BSP + started APs).
pub fn cpu_count() -> u32 {
    CPU_COUNT.load(Ordering::Relaxed)
}

/// Send NMI to all other CPUs to freeze them for Red Ring.
/// Constitutional requirement (SSOT §Red Ring Step 2).
/// MUST be called with RED_RING_ACTIVE already set to true so the NMI handler
/// on each recipient recognizes this as a halt-request.
/// Does NOT acquire any lock — safe to call from panic/fault context.
pub fn nmi_broadcast_halt() {
    let self_id = lapic_id();
    let mask = SCHEDULER_VISIBLE_MASK.load(Ordering::Relaxed);
    // The scheduler-visible mask uses APIC ID as the bit position (1 << apic_id).
    // ICR delivery mode = NMI (100b = 0x400), level assert.
    for apic_id in 0..64u32 {
        if mask & (1u64 << apic_id) == 0 {
            continue;
        }
        if apic_id == self_id {
            continue;
        }
        send_ipi(apic_id, 0x0000_4400);
    }
}

/// Bitmask of CPUs that reached long-mode AP entry.
pub fn started_mask() -> u64 {
    AP_STARTED_MASK.load(Ordering::Relaxed)
}

/// Bitmask of CPUs with per-CPU architecture setup complete.
pub fn initialized_mask() -> u64 {
    INITIALIZED_MASK.load(Ordering::Relaxed)
}

/// Bitmask of CPUs with valid scheduler current[] and idle[] slots.
pub fn scheduler_visible_mask() -> u64 {
    SCHEDULER_VISIBLE_MASK.load(Ordering::Relaxed)
}

/// Compatibility alias for scheduler-visible CPU APIC IDs.
pub fn online_mask() -> u64 {
    scheduler_visible_mask()
}

/// NUMA node for a scheduler CPU. Defaults to flat node 0 until SRAT says otherwise.
pub fn numa_node_for_cpu(cpu: usize) -> Option<usize> {
    if cpu >= crate::process::table::MAX_CPUS {
        return None;
    }
    Some(CPU_NUMA_NODE[cpu].load(Ordering::Relaxed))
}
