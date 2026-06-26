use crate::config;
use crate::diag::watchdog;
use crate::version;
use crate::{print, println};
use alloc::format;
use alloc::string::{String, ToString};

/// Render a one-line description of a process state.
fn state_str(s: &crate::process::ProcessState) -> &'static str {
    use crate::process::ProcessState::*;
    match s {
        Ready => "ready",
        Running => "run",
        Blocked => "block",
        Zombie => "zombie",
        New => "new",
        Dead => "dead",
    }
}

/// Snapshot the process table into (pid, name, state, cpu-or-(-1), pinned).
fn proc_snapshot() -> alloc::vec::Vec<(u32, String, &'static str, i32, bool)> {
    let t = crate::process::table::TABLE.lock();
    let mut out = alloc::vec::Vec::new();
    for (&pid, p) in t.procs.iter() {
        let mut cpu = -1i32;
        for c in 0..crate::process::table::MAX_CPUS {
            if t.current_on_cpu(c) == Some(pid) {
                cpu = c as i32;
                break;
            }
        }
        out.push((
            pid,
            p.name.clone(),
            state_str(p.state()),
            cpu,
            p.boot_cpu_affine,
        ));
    }
    out
}

pub fn uname() {
    println!(
        "{} {}  arch=x86_64  mode=64-bit  boot=GRUB/Multiboot2",
        version::SAIOS_NAME,
        version::SAIOS_VERSION_TAG
    );
    println!("{}", version::SAIOS_FULL_NAME);
    println!("Built with Rust (nightly) - no_std bare-metal");
}

/// `sysinfo` - complete hardware, CPU, memory, filesystem and process snapshot.
pub fn sysinfo() {
    let (mtot, mfree, mused) = crate::memory::frame_stats();
    let secs = crate::time::uptime_secs();
    let (rt, _) = crate::time::realtime();
    let (y, mo, d, h, mi, s) = crate::time::civil_from_epoch(rt);

    println!("╔═ SAIOS System Information ═══════════════════════════");
    println!("System");
    println!(
        "  OS         : {} {} ({}, x86_64)",
        version::SAIOS_NAME,
        version::SAIOS_VERSION_TAG,
        version::SAIOS_ABI_LABEL
    );
    println!(
        "  Date (UTC) : {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, mo, d, h, mi, s
    );
    println!(
        "  Uptime     : {}h {:02}m {:02}s",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    );

    println!("CPU");
    println!("  Model      : {}", cpuid_brand());
    println!(
        "  Cores      : {} scheduler-visible",
        crate::smp::cpu_count(),
    );
    println!("  Started    : {:#x}", crate::smp::started_mask());
    println!("  Initialized: {:#x}", crate::smp::initialized_mask());
    println!("  Scheduler  : {:#x}", crate::smp::scheduler_visible_mask());
    let mhz = crate::time::tsc_hz() / 1_000_000;
    println!("  TSC clock  : {} MHz", mhz);

    println!("Memory (4 KiB frames)");
    println!("  Total      : {} MiB", mtot * 4 / 1024);
    println!(
        "  Used       : {} MiB ({}%)",
        mused * 4 / 1024,
        mused
            .checked_mul(100)
            .and_then(|n| n.checked_div(mtot))
            .unwrap_or(0)
    );
    println!("  Free       : {} MiB", mfree * 4 / 1024);

    println!("Filesystems");
    for (path, fstype) in crate::vfs::list_mounts() {
        println!("  {:<10} {}", fstype, path);
    }

    let net = crate::network_contract::NetworkContract::status_view();
    let ip = net.identity.ip;
    let mac = net.identity.mac;
    println!("Network");
    println!("  IP         : {}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
    println!(
        "  MAC        : {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
    );

    let procs = proc_snapshot();
    println!("Processes ({} total)", procs.len());
    println!("  PID  CPU  STATE   NAME");
    for (pid, name, st, cpu, pinned) in &procs {
        let cpu_s = if *cpu >= 0 {
            alloc::format!("{}", cpu)
        } else {
            String::from("-")
        };
        println!(
            "  {:>3}  {:>3}  {:<6}  {}{}",
            pid,
            cpu_s,
            st,
            name,
            if *pinned { " [bsp]" } else { "" }
        );
    }
    println!("╚══════════════════════════════════════════════════════");
}

/// `resmon` - live resource monitor.  Refreshes ~1 Hz; press q or Esc to quit.
pub fn resmon() {
    loop {
        crate::vga_buffer::clear();
        let (mtot, mfree, mused) = crate::memory::frame_stats();
        let secs = crate::time::uptime_secs();
        let kb = crate::interrupts::KB_IRQS.load(core::sync::atomic::Ordering::Relaxed);
        let tm = crate::interrupts::TIMER_IRQS.load(core::sync::atomic::Ordering::Relaxed);
        let ms = crate::interrupts::MOUSE_IRQS.load(core::sync::atomic::Ordering::Relaxed);

        println!(
            "SAIOS resmon - q to quit            uptime {}h {:02}m {:02}s",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        );
        println!("-------------------------------------------------");

        let pct = mused
            .checked_mul(100)
            .and_then(|n| n.checked_div(mtot))
            .unwrap_or(0);
        let fill = pct * 20 / 100;
        let mut bar = String::new();
        for i in 0..20 {
            bar.push(if i < fill { '#' } else { '.' });
        }
        println!(
            "MEM [{}] {:>3}%  {} / {} MiB",
            bar,
            pct,
            mused * 4 / 1024,
            mtot * 4 / 1024
        );

        println!(
            "CPU {} cores online (mask {:#x})",
            crate::smp::cpu_count(),
            crate::smp::online_mask()
        );
        println!("IRQ  timer={}  kb={}  mouse={}", tm, kb, ms);
        println!();
        println!("PID  CPU  STATE   NAME");
        for (pid, name, st, cpu, pinned) in proc_snapshot() {
            let cpu_s = if cpu >= 0 {
                alloc::format!("{}", cpu)
            } else {
                String::from("-")
            };
            println!(
                "{:>3}  {:>3}  {:<6}  {}{}",
                pid,
                cpu_s,
                st,
                name,
                if pinned { " [bsp]" } else { "" }
            );
        }

        let start = crate::time::uptime_ns();
        let mut quit = false;
        while crate::time::uptime_ns().wrapping_sub(start) < 1_000_000_000 {
            if let Some(
                crate::driver::keyboard::KeyEvent::Char('q')
                | crate::driver::keyboard::KeyEvent::Char('\x03')
                | crate::driver::keyboard::KeyEvent::Escape,
            ) = crate::driver::keyboard::poll()
            {
                quit = true;
                break;
            }
            x86_64::instructions::hlt();
        }
        if quit {
            break;
        }
    }
    crate::vga_buffer::clear();
    println!("resmon: exited");
}

pub fn smptest() {
    let n = crate::smp::cpu_count();
    let expected_mask = crate::smp::online_mask();
    let workers = (n * 2).max(4);
    crate::shell::SMP_TEST_CPU_MASK.store(0, core::sync::atomic::Ordering::Relaxed);
    for _ in 0..workers {
        crate::process::kthread::spawn("smpworker", crate::shell::smp_worker_thread);
    }
    let deadline = crate::time::uptime_ns() + 3_000_000_000;
    while crate::time::uptime_ns() < deadline {
        let observed = crate::shell::SMP_TEST_CPU_MASK.load(core::sync::atomic::Ordering::Relaxed);
        if observed & expected_mask == expected_mask {
            break;
        }
        crate::process::scheduler::yield_now();
    }
    let observed = crate::shell::SMP_TEST_CPU_MASK.load(core::sync::atomic::Ordering::Relaxed);
    println!("spawned {} compute threads across {} core(s)", workers, n);
    println!("expected CPU mask: {:#x}", expected_mask);
    println!("observed CPU mask: {:#x}", observed);
    if observed & expected_mask == expected_mask {
        println!("smptest PASS: workers executed on all online scheduler CPUs");
    } else {
        println!(
            "smptest FAIL: missing CPU mask {:#x}",
            expected_mask & !observed
        );
    }
}

pub fn kds(args: &str) {
    match args.trim() {
        "" => kds_health(),
        "health" => kds_health(),
        "events" => kds_events(),
        "metrics" => kds_metrics(),
        "traces" => kds_traces(),
        "objects" => kds_objects(),
        "state" => kds_state(),
        _ => println!("usage: kds [health|events|metrics|traces|objects|state]"),
    }
}

pub fn obs(args: &str) {
    let mut parts = args.split_whitespace();
    match parts.next() {
        Some("last") => match parts.next().and_then(contract_id_from_name) {
            Some(contract) => obs_last(contract),
            None => println!("usage: obs last <contract>"),
        },
        Some("trace") => match parts.next().and_then(parse_u64_arg) {
            Some(correlation_id) => obs_trace(correlation_id),
            None => println!("usage: obs trace <correlation_id>"),
        },
        Some("gaps") => obs_gaps(),
        _ => println!("usage: obs [last <contract>|trace <correlation_id>|gaps]"),
    }
}

fn obs_last(contract: u16) {
    let mut found = None;
    crate::kds::for_each_event(256, |record| {
        if record.shape.contract == contract && record.event_id != 0 {
            found = Some(*record);
        }
    });
    match found {
        Some(record) => print_obs_event("last", &record),
        None => println!(
            "obs last: no event for contract {}",
            contract_name(contract)
        ),
    }
}

fn obs_trace(correlation_id: u64) {
    println!("Observability trace correlation_id={:#x}", correlation_id);
    let mut count = 0usize;
    crate::kds::for_each_event(128, |record| {
        if record.shape.correlation_id == correlation_id && record.event_id != 0 {
            count += 1;
            print_obs_event("trace", record);
        }
    });
    if count == 0 {
        println!("  gap: no events recorded for correlation id");
    }
}

fn obs_gaps() {
    println!("Observability required-event gaps");
    let required = [
        ("process.create", crate::kds::KdsEventType::TaskCreate),
        ("process.exit", crate::kds::KdsEventType::TaskExit),
        ("sched.switch", crate::kds::KdsEventType::ContextSwitch),
        ("memory.alloc", crate::kds::KdsEventType::PageAlloc),
        ("memory.free", crate::kds::KdsEventType::PageFree),
        ("as.map", crate::kds::KdsEventType::Mmap),
        ("as.unmap", crate::kds::KdsEventType::Munmap),
        ("irq.entry", crate::kds::KdsEventType::InterruptEnter),
        ("irq.exit", crate::kds::KdsEventType::InterruptExit),
        ("vfs.open", crate::kds::KdsEventType::FileOpen),
        ("vfs.read", crate::kds::KdsEventType::FileRead),
        ("vfs.write", crate::kds::KdsEventType::FileWrite),
    ];
    let mut missing = 0usize;
    for (name, event_type) in required {
        let count = crate::kds::count_events(event_type);
        if count == 0 {
            missing += 1;
            println!(
                "  missing {:<16} event={}",
                name,
                crate::kds::event_type_name(event_type)
            );
        } else {
            println!("  present {:<16} count={}", name, count);
        }
    }
    if missing == 0 {
        println!("  no required-event gaps detected in retained KDS window");
    }
}

fn print_obs_event(prefix: &str, record: &crate::kds::EventRecord) {
    println!(
        "  {} id={} ts={} contract={} outcome={} resource={} owner={:#x} cid={:#x} event={} payload={:#x},{:#x},{:#x},{:#x}",
        prefix,
        record.event_id,
        record.metadata.timestamp,
        contract_name(record.shape.contract),
        record.shape.outcome,
        record.shape.resource,
        record.shape.owner,
        record.shape.correlation_id,
        crate::kds::event_type_name(record.metadata.event_type),
        record.payload[0],
        record.payload[1],
        record.payload[2],
        record.payload[3]
    );
}

fn parse_u64_arg(value: &str) -> Option<u64> {
    value
        .strip_prefix("0x")
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .or_else(|| value.parse::<u64>().ok())
}

fn contract_id_from_name(name: &str) -> Option<u16> {
    Some(match name {
        "address-space" | "as" => crate::observability_contract::ContractId::AddressSpace as u16,
        "driver" => crate::observability_contract::ContractId::Driver as u16,
        "execution" => crate::observability_contract::ContractId::Execution as u16,
        "interrupt" | "irq" => crate::observability_contract::ContractId::Interrupt as u16,
        "memory" => crate::observability_contract::ContractId::Memory as u16,
        "network" => crate::observability_contract::ContractId::Network as u16,
        "process" => crate::observability_contract::ContractId::Process as u16,
        "scheduler" | "sched" => crate::observability_contract::ContractId::Scheduler as u16,
        "syscall" => crate::observability_contract::ContractId::Syscall as u16,
        "vfs" => crate::observability_contract::ContractId::Vfs as u16,
        "watchdog" => crate::observability_contract::ContractId::Watchdog as u16,
        _ => return None,
    })
}

fn contract_name(contract: u16) -> &'static str {
    match contract {
        x if x == crate::observability_contract::ContractId::AddressSpace as u16 => "address-space",
        x if x == crate::observability_contract::ContractId::Driver as u16 => "driver",
        x if x == crate::observability_contract::ContractId::Execution as u16 => "execution",
        x if x == crate::observability_contract::ContractId::Interrupt as u16 => "interrupt",
        x if x == crate::observability_contract::ContractId::Memory as u16 => "memory",
        x if x == crate::observability_contract::ContractId::Network as u16 => "network",
        x if x == crate::observability_contract::ContractId::Process as u16 => "process",
        x if x == crate::observability_contract::ContractId::Scheduler as u16 => "scheduler",
        x if x == crate::observability_contract::ContractId::Syscall as u16 => "syscall",
        x if x == crate::observability_contract::ContractId::Vfs as u16 => "vfs",
        x if x == crate::observability_contract::ContractId::Watchdog as u16 => "watchdog",
        _ => "unknown",
    }
}

fn kds_health() {
    let stats = crate::kds::stats();
    println!("KDS health");
    print_kds_stream("events", stats.events);
    print_kds_stream("metrics", stats.metrics);
    print_kds_stream("traces", stats.traces);
    print_kds_stream("objects", stats.objects);
    print_kds_stream("state", stats.state);
    println!("  aggregates active={}", stats.aggregates_used);
}

fn print_kds_stream(name: &str, stream: crate::kds::KdsStreamStats) {
    let utilization = (stream.records as usize)
        .saturating_mul(100)
        .checked_div(stream.capacity)
        .unwrap_or(0);
    let overflow = if stream.dropped == 0 {
        "ok"
    } else {
        "overflow"
    };
    println!(
        "  {:<7} stored={:<5} drops={:<5} cap={:<5} util={:>3}% overflow={} recsz={:<3} provider={} base={} file={}",
        name,
        stream.records,
        stream.dropped,
        stream.capacity,
        utilization,
        overflow,
        stream.record_size,
        stream.storage_provider.name(),
        stream.base_path,
        stream.filename
    );
}

fn kds_events() {
    println!("KDS events (recent)");
    crate::kds::for_each_event(32, |record| {
        println!(
            "  id={:<5} ts={:<12} cpu={} pid={} {:<10} {:<18} payload={:#x},{:#x},{:#x},{:#x}",
            record.event_id,
            record.metadata.timestamp,
            record.metadata.cpu_id,
            record.metadata.process_id,
            crate::kds::subsystem_name(record.metadata.subsystem),
            crate::kds::event_type_name(record.metadata.event_type),
            record.payload[0],
            record.payload[1],
            record.payload[2],
            record.payload[3]
        );
    });
}

fn kds_metrics() {
    println!("KDS metrics (recent)");
    crate::kds::flush_aggregates();
    crate::kds::for_each_metric(32, |record| {
        println!(
            "  ts={:<12} cpu={} pid={} {:<10} {:<20} value={} payload={},{}",
            record.metadata.timestamp,
            record.metadata.cpu_id,
            record.metadata.process_id,
            crate::kds::subsystem_name(record.metadata.subsystem),
            crate::kds::metric_name(record.metric_id),
            record.value,
            record.payload[0],
            record.payload[1]
        );
    });
}

fn kds_traces() {
    println!("KDS traces (recent)");
    crate::kds::for_each_trace(32, |record| {
        println!(
            "  trace={} parent={} {:<10} {:<11} start={} end={} duration={}",
            record.trace_id,
            record.parent_trace_id,
            crate::kds::subsystem_name(record.metadata.subsystem),
            crate::kds::event_type_name(record.metadata.event_type),
            record.start_time,
            record.end_time,
            record.duration
        );
    });
}

fn kds_objects() {
    println!("KDS objects (recent)");
    crate::kds::for_each_object(32, |record| {
        println!(
            "  object={} kind={:<8} parent={} cpu={} pid={} payload={:#x},{:#x}",
            record.object_id,
            crate::kds::object_kind_name(record.object_kind),
            record.parent_object_id,
            record.metadata.cpu_id,
            record.metadata.process_id,
            record.payload[0],
            record.payload[1]
        );
    });
}

fn kds_state() {
    println!("KDS state (recent)");
    crate::kds::for_each_state(32, |record| {
        println!(
            "  state={} ts={} cpu={} pid={} {:<10} value={} payload={:#x},{:#x}",
            record.state_id,
            record.metadata.timestamp,
            record.metadata.cpu_id,
            record.metadata.process_id,
            crate::kds::subsystem_name(record.metadata.subsystem),
            record.value,
            record.payload[0],
            record.payload[1]
        );
    });
}

pub fn verify(args: &str) {
    match args.trim() {
        "observability" => verify_observability(),
        _ => println!("usage: verify observability"),
    }
}

fn verify_observability() {
    crate::kds::flush_aggregates();
    println!("Observability Contract verification");
    let stats = crate::kds::stats();
    verify_line("KDS initialized", stats.events.records > 0, false);
    verify_line(
        "storage-independent KDS",
        crate::observability_contract::ObservabilityContract::validate_storage_independent_kds()
            .is_ok(),
        false,
    );
    verify_line(
        "heartbeat active",
        crate::kds::count_metrics(crate::kds::KdsMetricId::CpuHeartbeat) > 0
            || crate::kds::aggregate_exists(
                crate::kds::KdsSubsystem::Watchdog,
                crate::kds::KdsMetricId::CpuHeartbeat,
            ),
        false,
    );
    verify_line(
        "watchdog active",
        crate::kds::count_events_for_subsystem(crate::kds::KdsSubsystem::Watchdog) > 0
            || crate::kds::count_metrics(crate::kds::KdsMetricId::WatchdogStallMs) > 0
            || crate::kds::aggregate_exists(
                crate::kds::KdsSubsystem::Watchdog,
                crate::kds::KdsMetricId::CpuHeartbeat,
            ),
        true,
    );
    verify_line(
        "metrics flowing",
        stats.metrics.records > 0 || stats.aggregates_used > 0,
        false,
    );
    verify_line("event stream functioning", stats.events.records > 0, false);
    verify_line(
        "no buffer exhaustion",
        stats.events.dropped == 0
            && stats.metrics.dropped == 0
            && stats.traces.dropped == 0
            && stats.objects.dropped == 0
            && stats.state.dropped == 0,
        false,
    );
}

fn verify_line(name: &str, pass: bool, warn_if_missing: bool) {
    let status = if pass {
        "PASS"
    } else if warn_if_missing {
        "WARN"
    } else {
        "FAIL"
    };
    println!("  {:<24} {}", name, status);
}

pub fn gziptest() {
    const G: &[u8] = &[
        31, 139, 8, 0, 0, 0, 0, 0, 2, 3, 243, 72, 205, 201, 201, 215, 81, 8, 118, 244, 244, 15, 86,
        72, 73, 77, 203, 73, 44, 73, 85, 40, 73, 45, 46, 81, 84, 8, 201, 72, 85, 40, 44, 205, 76,
        206, 86, 72, 42, 202, 47, 207, 83, 72, 203, 175, 80, 200, 42, 205, 45, 40, 86, 200, 47, 75,
        45, 82, 40, 1, 74, 231, 36, 86, 85, 42, 164, 228, 167, 235, 1, 0, 21, 170, 193, 88, 71, 0,
        0, 0,
    ];
    match crate::compress::deflate::gzip_decompress(G) {
        Ok(d) => println!(
            "gziptest OK ({} bytes): {}",
            d.len(),
            String::from_utf8_lossy(&d)
        ),
        Err(e) => println!("gziptest FAIL: {}", e),
    }
}

pub fn cpus() {
    let n = crate::smp::cpu_count();
    let mask = crate::smp::online_mask();
    println!("CPU cores online: {}", n);
    println!("Online APIC mask: {:#x}", mask);
    println!("This CPU apic_id: {}", crate::smp::lapic_id());
    if n <= 1 {
        println!("(single-core - APs idle or not present)");
    }
}

pub fn cpuinfo() {
    let vendor = cpuid_vendor();
    let brand = cpuid_brand();
    let (family, model, stepping) = cpuid_family();
    let features = cpuid_features();
    println!("Vendor  : {}", vendor);
    println!("Brand   : {}", brand);
    println!(
        "Family  : {}  Model: {}  Stepping: {}",
        family, model, stepping
    );
    println!("Features: {}", features);
}

fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {0:e}, ebx",
            "pop rbx",
            out(reg) ebx,
            inlateout("eax") leaf => eax,
            out("ecx") ecx,
            out("edx") edx,
        );
    }
    (eax, ebx, ecx, edx)
}

fn cpuid_vendor() -> String {
    let (_, ebx, ecx, edx) = cpuid(0);
    let mut out = [0u8; 12];
    out[0..4].copy_from_slice(&ebx.to_le_bytes());
    out[4..8].copy_from_slice(&edx.to_le_bytes());
    out[8..12].copy_from_slice(&ecx.to_le_bytes());
    core::str::from_utf8(&out)
        .unwrap_or("Unknown")
        .trim_end_matches('\0')
        .into()
}

fn cpuid_brand() -> String {
    let mut brand = [0u8; 48];
    for (i, leaf) in [0x8000_0002u32, 0x8000_0003, 0x8000_0004]
        .iter()
        .enumerate()
    {
        let (eax, ebx, ecx, edx) = cpuid(*leaf);
        let off = i * 16;
        brand[off..off + 4].copy_from_slice(&eax.to_le_bytes());
        brand[off + 4..off + 8].copy_from_slice(&ebx.to_le_bytes());
        brand[off + 8..off + 12].copy_from_slice(&ecx.to_le_bytes());
        brand[off + 12..off + 16].copy_from_slice(&edx.to_le_bytes());
    }
    let s = core::str::from_utf8(&brand).unwrap_or("Unknown");
    s.trim_matches('\0').trim().into()
}

fn cpuid_family() -> (u32, u32, u32) {
    let (eax, _, _, _) = cpuid(1);
    let stepping = eax & 0xF;
    let model = ((eax >> 4) & 0xF) | (((eax >> 16) & 0xF) << 4);
    let family = ((eax >> 8) & 0xF) + ((eax >> 20) & 0xFF);
    (family, model, stepping)
}

fn cpuid_features() -> String {
    let (_, _, ecx, edx) = cpuid(1);
    let mut f = String::new();
    if edx & (1 << 25) != 0 {
        f.push_str("SSE ");
    }
    if edx & (1 << 26) != 0 {
        f.push_str("SSE2 ");
    }
    if ecx & (1 << 0) != 0 {
        f.push_str("SSE3 ");
    }
    if ecx & (1 << 9) != 0 {
        f.push_str("SSSE3 ");
    }
    if ecx & (1 << 19) != 0 {
        f.push_str("SSE4.1 ");
    }
    if ecx & (1 << 20) != 0 {
        f.push_str("SSE4.2 ");
    }
    if ecx & (1 << 28) != 0 {
        f.push_str("AVX ");
    }
    if ecx & (1 << 5) != 0 {
        f.push_str("VMX ");
    }
    if f.is_empty() {
        f.push_str("(none detected)");
    }
    f
}

pub fn meminfo() {
    let (total, free, used) = crate::memory::frame_stats();
    println!("Physical frames (4 KiB each):");
    println!("  Total : {:6}  ({} MiB)", total, total * 4 / 1024);
    println!("  Used  : {:6}  ({} MiB)", used, used * 4 / 1024);
    println!("  Free  : {:6}  ({} MiB)", free, free * 4 / 1024);
    println!();
    println!("Memory map (Multiboot2 regions):");
    let regions = crate::multiboot::CACHED_REGIONS.lock();
    let count = *crate::multiboot::CACHED_REGION_COUNT.lock();
    for r in &regions[..count] {
        let kind = match r.kind {
            1 => "Available",
            2 => "Reserved ",
            3 => "ACPI     ",
            4 => "NVS      ",
            5 => "BadRAM   ",
            _ => "Unknown  ",
        };
        println!(
            "  {:#010x}–{:#010x}  {} MiB  {}",
            r.base,
            r.base + r.len,
            r.len / (1024 * 1024),
            kind
        );
    }
    println!("Heap: 256 MiB static");
}

pub static BOOT_TICKS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn boot_ticks() -> u64 {
    BOOT_TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn tick() {
    BOOT_TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

pub fn uptime() {
    let t = boot_ticks();
    println!("Ticks since boot: {}", t);
    println!("(timer fires at PIT default rate ~18 Hz on bare metal)");
}

pub fn clear() {
    crate::vga_buffer::clear();
}

pub fn disktest(args: &str) {
    let dev = match crate::block::get() {
        Some(d) => d,
        None => {
            println!("disktest: no block device found");
            return;
        }
    };
    const LBA: u64 = 1024;
    if args.trim() == "check" {
        let mut buf = [0u8; 512];
        match dev.read_sectors(LBA, &mut buf) {
            Ok(()) => {
                if &buf[..8] == b"SAIOSDSK" {
                    let marker = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
                    println!(
                        "disktest: PERSISTED ✓  magic found, marker={} - AHCI writes survive reboot.",
                        marker
                    );
                } else {
                    println!(
                        "disktest: NOT persisted ✗  magic absent (got {:02x} {:02x} {:02x} {:02x} ...).",
                        buf[0], buf[1], buf[2], buf[3]
                    );
                    println!("          AHCI writes are not reaching the VDI across a reboot.");
                }
            }
            Err(e) => println!("disktest: read failed: {}", e),
        }
    } else {
        let marker = boot_ticks() as u32 ^ 0xC0DE;
        let mut buf = [0u8; 512];
        buf[..8].copy_from_slice(b"SAIOSDSK");
        buf[8..12].copy_from_slice(&marker.to_le_bytes());
        let mut i = 16;
        while i < 512 {
            buf[i] = (i as u8) ^ 0x5A;
            i += 1;
        }
        match dev.write_sectors(LBA, &buf) {
            Ok(()) => {
                println!("disktest: wrote marker {} to LBA {}.", marker, LBA);
                println!(
                    "          Now reboot the VM (your normal reboot), then run: disktest check"
                );
            }
            Err(e) => println!("disktest: write failed: {}", e),
        }
    }
}

pub fn storage(args: &str) {
    match args.trim() {
        "" | "diagnose" => storage_diagnose(),
        "daignose" | "diagnostics" => {
            println!("Did you mean: storage diagnose");
            storage_diagnose();
        }
        "disks" => storage_disks(),
        "partitions" => storage_partitions(),
        "filesystems" => storage_filesystems(),
        "operating-systems" | "oses" => storage_operating_systems(),
        "analyze" => storage_analyze(),
        "analyse" => {
            println!("Did you mean: storage analyze");
            storage_analyze();
        }
        "graph" => storage_graph(),
        "snapshots" | "snapshot-store" => storage_snapshot_store(),
        "risk" => storage_risk(),
        "plan" | "plan install" | "install" => {
            storage_execution_plan(crate::saios::storage_platform::StorageIntent::Install)
        }
        "plan update" | "update" => {
            storage_execution_plan(crate::saios::storage_platform::StorageIntent::Update)
        }
        "plan recover" | "recover" | "repair" => {
            storage_execution_plan(crate::saios::storage_platform::StorageIntent::Recover)
        }
        "plan rollback" | "rollback" => {
            storage_execution_plan(crate::saios::storage_platform::StorageIntent::Rollback)
        }
        "plan diagnose" => {
            storage_execution_plan(crate::saios::storage_platform::StorageIntent::Diagnose)
        }
        "validate" => storage_validate(),
        "simulate" => storage_simulate(),
        "recommend" | "advice" => storage_recommend(),
        "resize" => storage_resize(),
        "recovery" => storage_recovery(),
        _ => println!(
            "usage: storage [diagnose|graph|snapshots|disks|partitions|filesystems|operating-systems|analyze|risk|plan [install|update|recover|rollback|diagnose]|repair|validate|simulate|recommend|resize|recovery]"
        ),
    }
}

fn storage_diagnose() {
    let report = crate::block::diagnose();
    println!("Storage diagnostics");
    if !report.disk_detected {
        println!("  Controller: none");
        println!("  Disk: not detected");
        if let Some(reason) = report.root_mount_failure {
            println!("  Root Mount: failed ({})", reason);
        }
        return;
    }

    if let Some(device) = report.device {
        let mib = device
            .sector_count
            .saturating_mul(device.sector_size as u64)
            .checked_div(1024 * 1024)
            .unwrap_or(0);
        println!(
            "  Controller: {}",
            crate::block::controller_name(device.controller)
        );
        match device.port {
            Some(port) => println!("  Port: {}", port),
            None => println!("  Port: n/a"),
        }
        println!(
            "  Disk: {} MiB (sectors={} sector_size={})",
            mib, device.sector_count, device.sector_size
        );
    }

    println!(
        "  Partition Table: MBR={} GPT={}",
        validity(report.mbr_valid),
        validity(report.gpt_valid)
    );
    println!("  Partitions: {}", report.partitions.len());
    for partition in &report.partitions {
        println!(
            "    Partition {}: table={} type=0x{:02x} start_lba={} size_lba={}",
            partition.index,
            crate::block::partition_table_name(partition.table),
            partition.type_code,
            partition.start_lba,
            partition.size_lba
        );
    }

    println!("  Filesystem Probes:");
    if report.probes.is_empty() {
        println!("    none");
    }
    for probe in &report.probes {
        let source = match probe.partition_index {
            Some(index) => alloc::format!("partition {}", index),
            None => String::from("fallback"),
        };
        println!(
            "    ext4 {}: probe_target_lba={} superblock_lba={} superblock_offset={} expected_magic=0x{:04x} actual_magic=0x{:04x} result={}",
            source,
            probe.probe_target_lba,
            probe.superblock_lba,
            probe.superblock_offset,
            probe.expected_magic,
            probe.actual_magic,
            probe.result
        );
    }

    if report.root_mount_success {
        println!("  Root Mount: success");
    } else if let Some(reason) = report.root_mount_failure {
        println!("  Root Mount: failed ({})", reason);
    } else {
        println!("  Root Mount: not attempted");
    }
}

fn storage_disks() {
    let report = crate::saios::storage_platform::scan_storage();
    println!("Storage Manager disks");
    match report.disk {
        Some(disk) => {
            println!("  transport     : {}", disk.transport);
            println!(
                "  controller    : {}",
                crate::block::controller_name(disk.controller)
            );
            println!("  vendor        : {}", disk.vendor);
            println!("  model         : {}", disk.model);
            println!("  serial        : {}", disk.serial);
            println!("  capacity      : {} MiB", disk.capacity_mib);
            println!("  sectors       : {}", disk.sector_count);
            println!("  sector_size   : {}", disk.sector_size);
        }
        None => println!("  no supported disk discovered"),
    }
}

fn storage_partitions() {
    let report = crate::saios::storage_platform::scan_storage();
    println!("Storage Manager partitions");
    println!(
        "  MBR={} GPT={}",
        validity(report.mbr_valid),
        validity(report.gpt_valid)
    );
    if report.partitions.is_empty() {
        println!("  none");
    }
    for partition in &report.partitions {
        println!(
            "  {} table={} type=0x{:02x} start_lba={} size_lba={} fs={}",
            partition.index,
            crate::block::partition_table_name(partition.table),
            partition.type_code,
            partition.start_lba,
            partition.size_lba,
            partition.filesystem.label()
        );
    }
}

fn storage_filesystems() {
    let report = crate::saios::storage_platform::scan_storage();
    println!("Storage Manager filesystems");
    if report.filesystems.is_empty() {
        println!("  none detected read-only");
    }
    for fs in &report.filesystems {
        println!(
            "  partition={} kind={} confidence={} evidence={}",
            fs.partition_index,
            fs.kind.label(),
            fs.confidence,
            fs.evidence
        );
    }
}

fn storage_operating_systems() {
    let report = crate::saios::storage_platform::scan_storage();
    println!("Storage Manager operating systems");
    if report.operating_systems.is_empty() {
        println!("  none detected");
    }
    for os in &report.operating_systems {
        println!(
            "  kind={} confidence={} evidence={}",
            os.kind.label(),
            os.confidence,
            os.evidence
        );
    }
}

fn storage_analyze() {
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let compatibility = &snapshot.compatibility;
    let target = &snapshot.target;
    let risk = &snapshot.risk;
    println!("Storage Platform analysis");
    println!("  operation_id          : {}", target.operation_id);
    println!("  compatibility_score   : {}", compatibility.score);
    println!(
        "  critical_failures     : {}",
        compatibility.critical_failures
    );
    println!("  warnings              : {}", compatibility.warnings);
    println!("  cpu_pass              : {}", compatibility.cpu_pass);
    println!("  memory_pass           : {}", compatibility.memory_pass);
    println!("  storage_pass          : {}", compatibility.storage_pass);
    println!("  boot_pass             : {}", compatibility.boot_pass);
    println!(
        "  filesystem_pass       : {}",
        compatibility.filesystem_pass
    );
    println!("  device_pass           : {}", compatibility.device_pass);
    println!("  target                : {}", target.classification);
    println!("  risk                  : {}", target.risk.label());
    println!("  dual_boot_required    : {}", target.dual_boot_required);
    println!("  risk_level            : {}", risk.level.label());
    println!("  risk_score            : {}", risk.score);
    if let Some(reason) = target.blocked_reason {
        println!("  advisory_reason       : {}", reason);
    }
    println!("  summary               : {}", compatibility.summary);
}

fn storage_risk() {
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let risk = &snapshot.risk;
    println!("Storage Platform install risk");
    println!("  operation_id       : {}", risk.operation_id);
    println!("  completed          : {}", risk.completed);
    println!("  level              : {}", risk.level.label());
    println!("  score              : {}", risk.score);
    if risk.factors.is_empty() {
        println!("  factors            : none");
    } else {
        println!("  factors:");
        for factor in &risk.factors {
            println!("    level            : {}", factor.level.label());
            println!("    reason           : {}", factor.reason);
            println!("    evidence         : {}", factor.evidence);
        }
    }
    if risk.recommendations.is_empty() {
        println!("  recommendations    : none");
    } else {
        println!("  recommendations:");
        for recommendation in &risk.recommendations {
            println!("    action           : {}", recommendation.action);
            println!("    reason           : {}", recommendation.reason);
        }
    }
}

fn storage_graph() {
    let graph = crate::saios::storage_platform::storage_graph();
    println!("Storage Platform graph");
    println!("  operation_id       : {}", graph.operation_id);
    println!("  classification     : {}", graph.classification.label());
    println!("  partitions         : {}", graph.partitions.len());
    for partition in &graph.partitions {
        println!(
            "    #{} {} fs={} start={} size={} evidence={}",
            partition.index,
            partition.classification.label(),
            partition.filesystem.label(),
            partition.start_lba,
            partition.size_lba,
            partition.evidence
        );
    }
    println!("  slots              : {}", graph.slots.len());
    for slot in &graph.slots {
        println!(
            "    {} state={} bootable={} partition={:?} evidence={}",
            slot.slot.label(),
            slot.state.label(),
            slot.bootable,
            slot.partition_index,
            slot.evidence
        );
    }
    println!("  boot_entries       : {}", graph.boot_entries.len());
    for entry in &graph.boot_entries {
        println!(
            "    {} preferred={} partition={:?} evidence={}",
            entry.name, entry.preferred, entry.partition_index, entry.evidence
        );
    }
}

fn storage_execution_plan(intent: crate::saios::storage_platform::StorageIntent) {
    let plan = crate::saios::storage_platform::execution_plan(intent);
    println!("Storage Platform Contract execution plan");
    println!("  plan_id            : {}", plan.plan_id);
    println!("  intent             : {}", plan.intent.label());
    println!(
        "  graph              : {}",
        plan.graph.classification.label()
    );
    println!("  target_available   : {}", plan.execution_enabled);
    println!("  approval_required  : {}", plan.approval_required);
    println!("  risk               : {}", plan.risk.label());
    println!("  advisory_checks:");
    for gate in &plan.gates {
        println!(
            "    {}: {} ({})",
            gate.gate.label(),
            if gate.passed { "pass" } else { "fail" },
            gate.evidence
        );
        if let Some(reason) = gate.blocking_reason {
            println!("      concern: {}", reason);
        }
    }
    println!("  steps:");
    for step in &plan.steps {
        println!("    {}. {}", step.step, step.action);
        println!("       affected      : {}", step.affected);
        println!("       verification  : {}", step.verification);
        println!("       rollback      : {}", step.rollback_action);
    }
    if let Some(reason) = plan.refusal_reason {
        println!("  target_issue       : {}", reason);
    }
}

fn storage_snapshot_store() {
    let report = crate::saios::storage_platform::snapshot_store_report();
    println!("Storage Platform snapshot store");
    println!("  operation_id         : {}", report.operation_id);
    println!("  metadata_dir         : {}", report.metadata_dir);
    println!("  slot_metadata        : {}", report.slot_metadata_path);
    println!("  latest_snapshot      : {}", report.latest_snapshot_path);
    println!("  available            : {}", report.available);
    println!("  latest_snapshot_id   : {:?}", report.latest_snapshot_id);
    println!("  evidence             : {}", report.evidence);
}

fn storage_validate() {
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let validation = &snapshot.validation;
    println!("Storage Platform install validation");
    println!("  operation_id       : {}", validation.operation_id);
    println!("  status             : {}", validation.status.label());
    println!("  checks:");
    for check in &validation.checks {
        println!(
            "    {}: {} ({})",
            check.name,
            if check.passed { "pass" } else { "fail" },
            check.evidence
        );
    }
    if validation.failures.is_empty() {
        println!("  concerns           : none");
    } else {
        println!("  concerns:");
        for failure in &validation.failures {
            println!("    reason           : {}", failure.reason);
            println!("    evidence         : {}", failure.evidence);
            println!("    recommendation   : {}", failure.suggested_fix);
        }
    }
    if validation.suggested_fixes.is_empty() {
        println!("  suggested_fixes    : none");
    } else {
        println!("  suggested_fixes:");
        for fix in &validation.suggested_fixes {
            println!("    - {}", fix);
        }
    }
}

fn storage_simulate() {
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let simulation = &snapshot.simulation;
    println!("Storage Platform install simulation");
    println!("  operation_id       : {}", simulation.operation_id);
    println!("  target_available   : {}", !simulation.blocked);
    if let Some(reason) = simulation.blocked_reason {
        println!("  target_issue       : {}", reason);
    }
    if simulation.actions.is_empty() {
        println!("  actions            : none");
    } else {
        for action in &simulation.actions {
            println!("ACTION {}:", action.step);
            println!("  {}", action.action);
            println!("  {}", action.detail);
        }
    }
    if simulation.no_changes_made {
        println!("NO CHANGES MADE");
    }
}

fn storage_recommend() {
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let recommendation = &snapshot.recommendation;
    println!("Storage Platform recommendation");
    println!("  operation_id       : {}", recommendation.operation_id);
    println!("  recommended_mode   : {}", recommendation.mode.label());
    println!("  confidence         : {}%", recommendation.confidence);
    println!("  reasons:");
    for reason in &recommendation.reasons {
        println!("    - {}", reason);
    }
    println!("  evidence:");
    for evidence in &recommendation.evidence {
        println!(
            "    {}: {} ({})",
            evidence.subject, evidence.detail, evidence.confidence
        );
    }
}

fn storage_resize() {
    let report = crate::saios::storage_platform::resize_analysis();
    println!("Safe resize analysis");
    println!("  safe              : {}", report.safe);
    println!("  implementation    : {}", if report.execution_enabled { "available" } else { "analysis-only" });
    println!("  reason            : {}", report.reason);
}

fn storage_recovery() {
    let report = crate::saios::storage_platform::recovery_report();
    println!("Storage Platform Contract recovery report");
    println!("  operation_id             : {}", report.operation_id);
    println!("  disk_diagnostics         : {}", report.disk_diagnostics);
    println!(
        "  partition_diagnostics    : {}",
        report.partition_diagnostics
    );
    println!(
        "  filesystem_diagnostics   : {}",
        report.filesystem_diagnostics
    );
    println!(
        "  efi_repair_available     : {}",
        report.efi_repair_available
    );
    println!(
        "  boot_repair_available    : {}",
        report.boot_repair_available
    );
    println!(
        "  rootfs_repair_available  : {}",
        report.rootfs_repair_available
    );
    println!("  summary                  : {}", report.summary);
}

fn validity(valid: bool) -> &'static str {
    if valid { "valid" } else { "invalid" }
}

pub fn journal(args: &str) {
    let a = args.trim();
    if a.is_empty() {
        crate::journal::dump(50, None);
    } else if a == "all" {
        crate::journal::dump(0, None);
    } else if let Some(n) = a.strip_prefix("-n") {
        crate::journal::dump(n.trim().parse().unwrap_or(50), None);
    } else {
        crate::journal::dump(0, Some(a));
    }
}

pub fn color(args: &str) {
    use crate::vga_buffer::{Color, ColorCode, WRITER};
    let cc = match args.trim() {
        "green" => ColorCode::new(Color::LightGreen, Color::Black),
        "cyan" => ColorCode::new(Color::LightCyan, Color::Black),
        "white" => ColorCode::new(Color::White, Color::Black),
        "yellow" => ColorCode::new(Color::Yellow, Color::Black),
        "red" => ColorCode::new(Color::LightRed, Color::Black),
        "blue" => ColorCode::new(Color::LightBlue, Color::Black),
        "pink" => ColorCode::new(Color::Pink, Color::Black),
        _ => {
            println!("usage: color <green|cyan|white|yellow|red|blue|pink>");
            return;
        }
    };
    WRITER.lock().color = cc;
    println!("Color set to {}.", args.trim());
}

pub fn lspci() {
    let devices = crate::driver::pci::scan();
    if devices.is_empty() {
        println!("No PCI devices found.");
        return;
    }
    for d in &devices {
        println!(
            "{:02x}:{:02x}.{} {:04x}:{:04x}  {}",
            d.bus,
            d.dev,
            d.func,
            d.vendor,
            d.device,
            d.class_name()
        );
    }
    println!("--- {} device(s)", devices.len());
}

pub fn reboot() -> ! {
    println!("Rebooting...");
    crate::power_contract::PowerContract::reboot();
}

pub fn halt() -> ! {
    println!("SAIOS shutting down via ACPI S5...");
    crate::power_contract::PowerContract::shutdown();
}

pub fn reload_cmd(args: &str) {
    let module = args.trim();
    if module.is_empty() {
        println!("usage: reload <module>");
        println!("modules: ai, network, packages, config");
        return;
    }

    match module {
        "ai" => {
            crate::configuration_contract::ConfigurationContract::reload_ai();
            println!("[reload] {} reloaded", module);
        }
        "network" => {
            crate::configuration_contract::ConfigurationContract::reload_network();
            println!("[reload] {} reloaded", module);
        }
        "packages" | "apt" => {
            crate::configuration_contract::ConfigurationContract::reload_packages();
            println!("[reload] {} reloaded", module);
        }
        "config" | "all" => {
            crate::configuration_contract::ConfigurationContract::reload();
            println!("[reload] {} reloaded", module);
        }
        _ => {
            println!("reload: unknown module '{}'", module);
            println!("known modules: ai, network, packages, config");
        }
    }
}

pub fn gfx(args: &str) {
    let sub = args.trim();
    if !crate::graphics::available() {
        println!("gfx: no framebuffer - reboot with graphics mode enabled.");
        println!("     The GRUB menu must set: set gfxpayload=1024x768x32");
        return;
    }
    match sub {
        "off" | "text" => {
            crate::graphics::clear(crate::graphics::BLACK);
            println!("gfx: framebuffer cleared.");
        }
        "info" => {
            let (w, h) = crate::graphics::dimensions();
            println!("Framebuffer: {}x{} (graphics mode active)", w, h);
        }
        "ui" => {
            crate::graphics::ui::demo();
        }
        _ => {
            crate::graphics::draw_desktop();
            loop {
                x86_64::instructions::hlt();
                if crate::interrupts::next_scancode().is_some() {
                    break;
                }
            }
            crate::vga_buffer::clear();
            println!("Returned to text mode.");
        }
    }
}

pub fn beep(args: &str) {
    let mut parts = args.split_whitespace();
    let freq = parts.next().and_then(|s| s.parse().ok()).unwrap_or(880u32);
    let ms = parts.next().and_then(|s| s.parse().ok()).unwrap_or(200u32);
    crate::driver::hda::beep(freq, ms);
}

pub fn jobs() {
    let running = crate::shell::IN_BG.load(core::sync::atomic::Ordering::Relaxed);
    if let Some((label, done, total)) = crate::shell::PROGRESS.lock().clone() {
        let pct = done
            .checked_mul(100)
            .and_then(|n| n.checked_div(total))
            .unwrap_or(0)
            .min(100);
        println!(
            "[running] {} - {}% ({}/{} KB)",
            label,
            pct,
            done / 1024,
            total / 1024
        );
    } else if running {
        println!("[running] background job in progress");
    }
    let q = crate::shell::BG_QUEUE.lock();
    if q.is_empty() && !running {
        println!("no background jobs (run a command with '&' to background it)");
    } else {
        for (i, j) in q.iter().enumerate() {
            println!("  [{}] queued: {}", i + 1, j);
        }
    }
}

pub fn help_system() {
    println!("  System:");
    println!("    uname              system info");
    println!("    cpuinfo            CPU details (CPUID)");
    println!("    meminfo            memory regions");
    println!("    lspci              list PCI devices");
    println!("    storage diagnose   explain disk, partition and root mount state");
    println!("    storage graph      constitutional StorageGraph view");
    println!("    storage snapshots  SPC snapshot metadata store status");
    println!("    storage analyze    compatibility and install-target analysis");
    println!("    storage recommend  risk recommendation and evidence");
    println!("    storage plan       SPC execution plan (install/update/recover/rollback)");
    println!("    storage resize     resize risk analysis");
    println!("    storage recovery   SAIOS recovery report");
    println!("    uptime             ticks since boot");
    println!("    kds [view]         KDS health/events/metrics/traces/objects/state");
    println!("    verify observability  check Observability Contract components");
    println!("    clear              clear screen");
    println!("    color <scheme>     set color (green/cyan/white/yellow/red)");
    println!("    reload <module>    reload module config/state");
    println!("    reboot / halt");
}
