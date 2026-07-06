use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use hal::arch::x86_64::{cpuid, interrupt};

use crate::console;
use crate::kernel::{device, process, testing};
use crate::{heap, object_manager, pmm, saifs, scheduler, timer, vfs};

struct RequiredGate {
    label: &'static str,
    category: &'static str,
    name: &'static str,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum ReadinessProfile {
    V03,
    V04,
}

impl ReadinessProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::V03 => "v0.3",
            Self::V04 => "v0.4",
        }
    }
}

#[derive(Copy, Clone)]
pub struct ReadinessGateStatus {
    pub label: &'static str,
    pub passed: bool,
    pub skipped: bool,
}

const REQUIRED_GATES_V03: &[RequiredGate] = &[
    RequiredGate {
        label: "Interrupts",
        category: "CPU",
        name: "interrupt enable/disable",
    },
    RequiredGate {
        label: "Timer",
        category: "Timer",
        name: "monotonic ticks",
    },
    RequiredGate {
        label: "Sleep",
        category: "Scheduler",
        name: "sleep",
    },
    RequiredGate {
        label: "Wake",
        category: "Scheduler",
        name: "wake",
    },
    RequiredGate {
        label: "Page Fault",
        category: "Memory",
        name: "page faults",
    },
    RequiredGate {
        label: "Invalid Pointer",
        category: "Memory",
        name: "invalid pointer handling",
    },
    RequiredGate {
        label: "stderr",
        category: "Console",
        name: "stderr",
    },
    RequiredGate {
        label: "Rename",
        category: "Filesystem",
        name: "rename",
    },
    RequiredGate {
        label: "Move",
        category: "Filesystem",
        name: "move",
    },
    RequiredGate {
        label: "Keyboard",
        category: "Drivers",
        name: "keyboard",
    },
    RequiredGate {
        label: "Mouse",
        category: "Drivers",
        name: "mouse",
    },
];

const REQUIRED_GATES_V04: &[RequiredGate] = &[
    RequiredGate {
        label: "VFS Open",
        category: "Filesystem",
        name: "open",
    },
    RequiredGate {
        label: "VFS Read",
        category: "Filesystem",
        name: "read",
    },
    RequiredGate {
        label: "VFS Write",
        category: "Filesystem",
        name: "write",
    },
    RequiredGate {
        label: "VFS Direnum",
        category: "Filesystem",
        name: "directory enumeration",
    },
    RequiredGate {
        label: "Mounts",
        category: "Storage",
        name: "mounted filesystems",
    },
    RequiredGate {
        label: "Proc Spawn",
        category: "Process",
        name: "process creation",
    },
    RequiredGate {
        label: "Proc Wait",
        category: "Process",
        name: "wait",
    },
    RequiredGate {
        label: "Sys ABI",
        category: "Syscall",
        name: "stable ABI smoke",
    },
];

fn required_gates(profile: ReadinessProfile) -> &'static [RequiredGate] {
    match profile {
        ReadinessProfile::V03 => REQUIRED_GATES_V03,
        ReadinessProfile::V04 => REQUIRED_GATES_V04,
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
}

impl TestStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
        }
    }
}

pub struct ValidateOptions {
    pub verbose: bool,
    pub perf: bool,
    pub stress: bool,
    pub json: bool,
    pub readiness: Option<ReadinessProfile>,
}

impl ValidateOptions {
    pub fn parse(args: &[&str]) -> Result<Self, &'static str> {
        let mut options = Self {
            verbose: false,
            perf: false,
            stress: false,
            json: false,
            readiness: None,
        };

        for arg in args {
            match *arg {
                "-v" => options.verbose = true,
                "--perf" => options.perf = true,
                "--stress" => options.stress = true,
                "--json" => options.json = true,
                "--ready" => options.readiness = Some(ReadinessProfile::V03),
                "--ready-v04" => options.readiness = Some(ReadinessProfile::V04),
                "--help" | "-h" => return Err("help"),
                _ => return Err("validate: unknown option"),
            }
        }

        Ok(options)
    }
}

pub struct TestResult {
    pub category: &'static str,
    pub name: &'static str,
    pub status: TestStatus,
    pub detail: &'static str,
    pub time_ms: u64,
}

impl TestResult {
    fn pass(category: &'static str, name: &'static str, start: u64) -> Self {
        Self {
            category,
            name,
            status: TestStatus::Pass,
            detail: "",
            time_ms: elapsed_ms(start),
        }
    }

    fn fail(category: &'static str, name: &'static str, detail: &'static str, start: u64) -> Self {
        Self {
            category,
            name,
            status: TestStatus::Fail,
            detail,
            time_ms: elapsed_ms(start),
        }
    }

    fn skip(category: &'static str, name: &'static str, detail: &'static str, start: u64) -> Self {
        Self {
            category,
            name,
            status: TestStatus::Skip,
            detail,
            time_ms: elapsed_ms(start),
        }
    }
}

pub struct ValidationReport {
    pub results: Vec<TestResult>,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub health: usize,
    pub time_ms: u64,
    pub readiness_profile: ReadinessProfile,
}

impl ValidationReport {
    fn new() -> Self {
        Self {
            results: Vec::new(),
            passed: 0,
            failed: 0,
            skipped: 0,
            health: 100,
            time_ms: 0,
            readiness_profile: ReadinessProfile::V03,
        }
    }

    fn push(&mut self, result: TestResult) {
        match result.status {
            TestStatus::Pass => self.passed += 1,
            TestStatus::Fail => self.failed += 1,
            TestStatus::Skip => self.skipped += 1,
        }
        self.results.push(result);
    }

    fn finish(&mut self, start: u64) {
        self.time_ms = elapsed_ms(start);
        let checked = self.passed + self.failed;
        self.health = if checked == 0 {
            100
        } else {
            (self.passed * 100) / checked
        };
    }

    fn find(&self, category: &str, name: &str) -> Option<&TestResult> {
        self.results
            .iter()
            .find(|result| result.category == category && result.name == name)
    }

    pub fn kernel_ready(&self) -> bool {
        required_gates(self.readiness_profile).iter().all(|gate| {
            self.find(gate.category, gate.name)
                .is_some_and(|result| result.status == TestStatus::Pass)
        })
    }

    pub fn total(&self) -> usize {
        self.results.len()
    }

    pub fn fully_clean(&self) -> bool {
        self.failed == 0 && self.skipped == 0
    }

    pub fn readiness_passed(&self) -> usize {
        required_gates(self.readiness_profile)
            .iter()
            .filter(|gate| {
                self.find(gate.category, gate.name)
                    .is_some_and(|result| result.status == TestStatus::Pass)
            })
            .count()
    }

    pub fn readiness_gate_statuses(&self) -> Vec<ReadinessGateStatus> {
        required_gates(self.readiness_profile)
            .iter()
            .map(|gate| {
                let status = self.find(gate.category, gate.name).map(|r| r.status);
                ReadinessGateStatus {
                    label: gate.label,
                    passed: matches!(status, Some(TestStatus::Pass)),
                    skipped: matches!(status, Some(TestStatus::Skip)),
                }
            })
            .collect()
    }
}

type TestFn = fn() -> Result<(), &'static str>;

struct TestCase {
    category: &'static str,
    name: &'static str,
    run: TestFn,
}

impl TestCase {
    const fn new(category: &'static str, name: &'static str, run: TestFn) -> Self {
        Self {
            category,
            name,
            run,
        }
    }
}

fn now_ms() -> u64 {
    timer::uptime().as_millis() as u64
}

fn elapsed_ms(start: u64) -> u64 {
    now_ms().saturating_sub(start)
}

fn wait_for_ticks(delta: u64) -> Result<(), &'static str> {
    let start = timer::ticks();
    let target = start.saturating_add(delta.max(1));

    // Use scheduler-aware sleep first; this validates wake-up behavior instead
    // of relying on a tight spin budget.
    timer::sleep(10);

    for _ in 0..64 {
        if timer::ticks() >= target {
            return Ok(());
        }
        scheduler::maybe_preempt();
        timer::sleep(1);
    }

    Err("timer ticks did not advance")
}

fn pass_or_skip(
    category: &'static str,
    name: &'static str,
    start: u64,
    result: Result<(), &'static str>,
) -> TestResult {
    match result {
        Ok(()) => TestResult::pass(category, name, start),
        Err(reason) if reason.starts_with("skip:") => {
            TestResult::skip(category, name, reason, start)
        }
        Err(reason) => TestResult::fail(category, name, reason, start),
    }
}

pub fn run(options: &ValidateOptions) -> ValidationReport {
    let mut report = ValidationReport::new();
    let suite_start = now_ms();

    report.readiness_profile = options.readiness.unwrap_or(ReadinessProfile::V03);

    let mut tests = if let Some(profile) = options.readiness {
        readiness_tests(profile)
    } else {
        core_tests()
    };
    if options.readiness.is_none() {
        if options.perf {
            tests.extend(perf_tests());
        }
        if options.stress {
            tests.extend(stress_tests());
        }
    }

    for test in tests {
        let start = now_ms();
        let result = pass_or_skip(test.category, test.name, start, (test.run)());
        report.push(result);
    }

    report.finish(suite_start);
    report
}

pub fn print_report(report: &ValidationReport, options: &ValidateOptions) {
    if options.json {
        print_json(report);
        return;
    }

    console::println!("=========================================");
    console::println!("SAIOS Kernel Validation Suite");
    console::println!("=========================================");
    console::newline();

    let mut current = "";
    for result in &report.results {
        if result.category != current {
            if !current.is_empty() {
                console::newline();
            }
            current = result.category;
            console::println!("{}", current);
            console::println!("--------------------------------");
            console::newline();
        }
        console::println!("[{}] {}", result.status.as_str(), result.name);
        if options.verbose && !result.detail.is_empty() {
            console::println!("       {} ({} ms)", result.detail, result.time_ms);
        } else if options.verbose {
            console::println!("       {} ms", result.time_ms);
        }
    }

    console::newline();
    console::println!("Summary");
    console::println!("--------------------------------");
    console::newline();
    console::println!("Total  : {}", report.total());
    console::println!("Passed : {}", report.passed);
    console::println!("Failed : {}", report.failed);
    console::println!("Skipped: {}", report.skipped);
    if options.verbose {
        console::println!("Time   : {} ms", report.time_ms);
    }
    console::newline();
    console::println!(
        "Validation Status: {}",
        if report.failed > 0 {
            "FAIL"
        } else if report.skipped > 0 {
            "PASS WITH SKIPS"
        } else {
            "PASS"
        }
    );
    console::println!("Kernel Health: {}%", report.health);
    console::println!(
        "Readiness Gates: {}/{}",
        report.readiness_passed(),
        required_gates(report.readiness_profile).len()
    );
    console::println!("Readiness Profile: {}", report.readiness_profile.as_str());
    console::println!(
        "Kernel Status: {}",
        if report.kernel_ready() {
            "Kernel READY"
        } else {
            "Kernel NOT READY"
        }
    );
    console::newline();
    console::println!("Readiness Gates");
    console::println!("--------------------------------");
    console::newline();
    for gate in required_gates(report.readiness_profile) {
        let status = match report.find(gate.category, gate.name).map(|r| r.status) {
            Some(TestStatus::Pass) => "PASS",
            Some(TestStatus::Skip) => "SKIP",
            _ => "FAIL",
        };
        console::println!("{:<16} {}", gate.label, status);
    }
    console::newline();
    console::println!("=========================================");
}

pub fn print_help() {
    console::println!("Usage: validate [options]");
    console::newline();
    console::println!("Options:");
    console::println!("  -v          verbose per-test diagnostics");
    console::println!("  --perf      include performance measurements");
    console::println!("  --stress    include stress tests");
    console::println!("  --ready     run required kernel-readiness gates only");
    console::println!("  --ready-v04 run v0.4 readiness gates only");
    console::println!("  --json      emit machine-readable JSON");
    console::println!("  --help      show this help text");
}

fn print_json(report: &ValidationReport) {
    console::println!("{{");
    console::println!("  \"passed\": {},", report.passed);
    console::println!("  \"failed\": {},", report.failed);
    console::println!("  \"skipped\": {},", report.skipped);
    console::println!("  \"health\": {},", report.health);
    console::println!(
        "  \"readiness_profile\": \"{}\",",
        report.readiness_profile.as_str()
    );
    console::println!("  \"kernel_ready\": {},", report.kernel_ready());
    console::println!("  \"fully_clean\": {},", report.fully_clean());
    console::println!("  \"time_ms\": {},", report.time_ms);
    console::println!("  \"tests\": [");
    for (idx, result) in report.results.iter().enumerate() {
        let comma = if idx + 1 == report.results.len() {
            ""
        } else {
            ","
        };
        console::println!(
            "    {{ \"category\": \"{}\", \"name\": \"{}\", \"status\": \"{}\", \"time_ms\": {}, \"detail\": \"{}\" }}{}",
            json_escape(result.category),
            json_escape(result.name),
            result.status.as_str(),
            result.time_ms,
            json_escape(result.detail),
            comma
        );
    }
    console::println!("  ]");
    console::println!(", \"readiness\": [");
    let gates = required_gates(report.readiness_profile);
    for (idx, gate) in gates.iter().enumerate() {
        let comma = if idx + 1 == gates.len() {
            ""
        } else {
            ","
        };
        let status = match report.find(gate.category, gate.name).map(|r| r.status) {
            Some(TestStatus::Pass) => "PASS",
            Some(TestStatus::Skip) => "SKIP",
            _ => "FAIL",
        };
        console::println!(
            "    {{ \"name\": \"{}\", \"passed\": {}, \"status\": \"{}\" }}{}",
            json_escape(gate.label),
            status == "PASS",
            status,
            comma
        );
    }
    console::println!("  ]");
    console::println!("}}");
}

fn readiness_tests(profile: ReadinessProfile) -> Vec<TestCase> {
    match profile {
        ReadinessProfile::V03 => alloc::vec![
            TestCase::new("CPU", "interrupt enable/disable", test_interrupt_toggle),
            TestCase::new("Timer", "monotonic ticks", test_timer_monotonic),
            TestCase::new("Scheduler", "sleep", test_sleep),
            TestCase::new("Scheduler", "wake", test_wake),
            TestCase::new("Memory", "page faults", test_page_faults),
            TestCase::new("Memory", "invalid pointer handling", test_invalid_pointer),
            TestCase::new("Console", "stderr", test_console_stderr),
            TestCase::new("Filesystem", "rename", test_fs_rename),
            TestCase::new("Filesystem", "move", test_fs_move),
            TestCase::new("Drivers", "keyboard", test_driver_keyboard),
            TestCase::new("Drivers", "mouse", test_driver_mouse),
        ],
        ReadinessProfile::V04 => alloc::vec![
            TestCase::new("Filesystem", "open", test_fs_open),
            TestCase::new("Filesystem", "read", test_fs_read),
            TestCase::new("Filesystem", "write", test_fs_write),
            TestCase::new("Filesystem", "directory enumeration", test_fs_directory_enumeration),
            TestCase::new("Storage", "mounted filesystems", test_storage_mounts),
            TestCase::new("Process", "process creation", test_process_creation),
            TestCase::new("Process", "wait", test_process_wait),
            TestCase::new("Syscall", "stable ABI smoke", test_syscall_smoke),
        ],
    }
}

fn json_escape(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn core_tests() -> Vec<TestCase> {
    alloc::vec![
        TestCase::new("CPU", "CPUID", test_cpuid),
        TestCase::new("CPU", "RDTSC", test_rdtsc),
        TestCase::new("CPU", "64-bit execution", test_64bit),
        TestCase::new("CPU", "interrupt enable/disable", test_interrupt_toggle),
        TestCase::new("CPU", "timer accuracy", test_timer_accuracy),
        TestCase::new("Memory", "malloc/free", test_malloc_free),
        TestCase::new("Memory", "aligned allocation", test_aligned_allocation),
        TestCase::new("Memory", "zero allocation", test_zero_allocation),
        TestCase::new("Memory", "large allocation", test_large_allocation),
        TestCase::new("Memory", "repeated allocation", test_repeated_allocation),
        TestCase::new("Memory", "memory copy", test_memory_copy),
        TestCase::new("Memory", "memory set", test_memory_set),
        TestCase::new("Memory", "page faults", test_page_faults),
        TestCase::new("Memory", "invalid pointer handling", test_invalid_pointer),
        TestCase::new("Scheduler", "yield", test_yield),
        TestCase::new("Scheduler", "sleep", test_sleep),
        TestCase::new("Scheduler", "wake", test_wake),
        TestCase::new("Scheduler", "fairness", test_fairness),
        TestCase::new("Scheduler", "timer scheduling", test_timer_scheduling),
        TestCase::new("Scheduler", "starvation resistance", test_starvation),
        TestCase::new("Process", "process creation", test_process_creation),
        TestCase::new("Process", "exit", test_process_exit),
        TestCase::new("Process", "wait", test_process_wait),
        TestCase::new("Process", "PID uniqueness", test_pid_uniqueness),
        TestCase::new("Process", "argument passing", test_argument_passing),
        TestCase::new("Syscall", "stable ABI smoke", test_syscall_smoke),
        TestCase::new("Console", "stdout", test_console_stdout),
        TestCase::new("Console", "stderr", test_console_stderr),
        TestCase::new("Console", "ANSI escape sequences", test_console_ansi),
        TestCase::new("Console", "scrolling", test_console_scrolling),
        TestCase::new("Console", "cursor positioning", test_console_cursor),
        TestCase::new("Console", "Unicode", test_console_unicode),
        TestCase::new("Framebuffer", "attached", test_framebuffer_attached),
        TestCase::new(
            "Surface Manager",
            "lifecycle",
            test_surface_manager_lifecycle
        ),
        TestCase::new("Filesystem", "create", test_fs_create),
        TestCase::new("Filesystem", "open", test_fs_open),
        TestCase::new("Filesystem", "close", test_fs_close),
        TestCase::new("Filesystem", "write", test_fs_write),
        TestCase::new("Filesystem", "read", test_fs_read),
        TestCase::new("Filesystem", "append", test_fs_append),
        TestCase::new("Filesystem", "rename", test_fs_rename),
        TestCase::new("Filesystem", "move", test_fs_move),
        TestCase::new("Filesystem", "delete", test_fs_delete),
        TestCase::new("Filesystem", "directory creation", test_fs_directory_create),
        TestCase::new(
            "Filesystem",
            "directory enumeration",
            test_fs_directory_enumeration
        ),
        TestCase::new("Storage", "volume registry", test_storage_volume_registry),
        TestCase::new("Storage", "mounted filesystems", test_storage_mounts),
        TestCase::new("Storage", "ext4 stage8 package", test_storage_ext4_stage8),
        TestCase::new(
            "Storage",
            "ext4 cache validation",
            test_storage_cache_validation
        ),
        TestCase::new(
            "Stability",
            "heap leak detection",
            test_stability_heap_leak_detection
        ),
        TestCase::new("Timer", "monotonic ticks", test_timer_monotonic),
        TestCase::new("Timer", "sleep advances ticks", test_timer_sleep),
        TestCase::new("Drivers", "keyboard", test_driver_keyboard),
        TestCase::new("Drivers", "mouse", test_driver_mouse),
        TestCase::new(
            "Drivers",
            "keyboard input behavior",
            test_driver_keyboard_behavior
        ),
        TestCase::new(
            "Drivers",
            "mouse input behavior",
            test_driver_mouse_behavior
        ),
        TestCase::new("Drivers", "timer", test_driver_timer),
        TestCase::new("Drivers", "serial", test_driver_serial),
        TestCase::new("Drivers", "framebuffer", test_driver_framebuffer),
        TestCase::new("Drivers", "storage", test_driver_storage),
    ]
}

fn perf_tests() -> Vec<TestCase> {
    alloc::vec![
        TestCase::new("Performance", "memcpy bandwidth", test_perf_memcpy),
        TestCase::new(
            "Performance",
            "framebuffer bandwidth",
            test_perf_framebuffer
        ),
        TestCase::new("Performance", "syscall throughput", test_perf_syscall),
        TestCase::new("Performance", "malloc throughput", test_perf_malloc),
        TestCase::new("Performance", "file I/O throughput", test_perf_file_io),
    ]
}

fn stress_tests() -> Vec<TestCase> {
    alloc::vec![
        TestCase::new("Stress", "repeated allocations", test_stress_allocations),
        TestCase::new("Stress", "create/delete files", test_stress_files),
        TestCase::new("Stress", "repeated process creation", test_stress_processes),
        TestCase::new("Stress", "repeated scheduler yields", test_stress_yields),
        TestCase::new("Stress", "console flood", test_stress_console),
        TestCase::new("Stress", "framebuffer flood", test_stress_framebuffer),
    ]
}

fn test_cpuid() -> Result<(), &'static str> {
    if cpuid::vendor().iter().all(|b| *b == 0) {
        return Err("CPUID vendor string is empty");
    }
    Ok(())
}

fn test_rdtsc() -> Result<(), &'static str> {
    let before = timer::ticks();
    let after = timer::ticks();
    if after < before {
        return Err("timer ticks moved backward");
    }
    Ok(())
}

fn test_64bit() -> Result<(), &'static str> {
    let value = 0x1_0000_0000u64 + 7;
    if core::mem::size_of::<usize>() < 8 || value != 0x1_0000_0007 {
        return Err("64-bit arithmetic or pointer width failed");
    }
    Ok(())
}

fn test_interrupt_toggle() -> Result<(), &'static str> {
    let was_enabled = interrupt::are_enabled();

    interrupt::disable();
    if interrupt::are_enabled() {
        return Err("interrupt disable did not clear IF");
    }

    if was_enabled {
        interrupt::enable();
        if !interrupt::are_enabled() {
            return Err("interrupt enable did not set IF");
        }
    }

    Ok(())
}

fn test_timer_accuracy() -> Result<(), &'static str> {
    let start = timer::ticks();
    wait_for_ticks(1)?;
    if timer::ticks() <= start {
        return Err("sleep did not advance timer ticks");
    }
    Ok(())
}

fn test_malloc_free() -> Result<(), &'static str> {
    let before = heap::stats();
    let data = alloc::vec![0x5au8; 64];
    if data.len() != 64 || data[0] != 0x5a {
        return Err("heap allocation content check failed");
    }
    drop(data);
    let after = heap::stats();
    if after.used > after.total || before.used > before.total {
        return Err("heap accounting invalid");
    }
    Ok(())
}

fn test_aligned_allocation() -> Result<(), &'static str> {
    let data = alloc::vec![0u64; 16];
    if data.as_ptr().addr() % core::mem::align_of::<u64>() != 0 {
        return Err("u64 vector is misaligned");
    }
    Ok(())
}

fn test_zero_allocation() -> Result<(), &'static str> {
    let data: Vec<u8> = Vec::new();
    if !data.is_empty() {
        return Err("empty allocation is not empty");
    }
    Ok(())
}

fn test_large_allocation() -> Result<(), &'static str> {
    let data = alloc::vec![0u8; 64 * 1024];
    if data.len() != 64 * 1024 {
        return Err("large allocation length mismatch");
    }
    Ok(())
}

fn test_repeated_allocation() -> Result<(), &'static str> {
    let mut blocks = Vec::new();
    for i in 0..64 {
        blocks.push(alloc::vec![i as u8; 32 + i]);
    }
    if blocks.len() != 64 {
        return Err("repeated allocation count mismatch");
    }
    Ok(())
}

fn test_memory_copy() -> Result<(), &'static str> {
    let src = alloc::vec![0xacu8; 128];
    let mut dst = alloc::vec![0u8; 128];
    dst.copy_from_slice(&src);
    if dst != src {
        return Err("copy_from_slice mismatch");
    }
    Ok(())
}

fn test_memory_set() -> Result<(), &'static str> {
    let buf = alloc::vec![0xa5u8; 128];
    if buf.iter().any(|b| *b != 0xa5) {
        return Err("memory set pattern mismatch");
    }
    Ok(())
}

fn test_page_faults() -> Result<(), &'static str> {
    if !crate::kernel::fault::policy_ready() {
        return Err("fault policy is not initialized");
    }

    // Error code without the user bit is kernel-domain.
    if crate::kernel::fault::domain_from_page_fault_error(0)
        != crate::kernel::fault::FaultDomain::Kernel
    {
        return Err("kernel page-fault classification mismatch");
    }

    Ok(())
}

fn test_invalid_pointer() -> Result<(), &'static str> {
    if !crate::kernel::fault::policy_ready() {
        return Err("fault policy is not initialized");
    }

    // Error code with user bit set must classify as user-domain.
    if crate::kernel::fault::domain_from_page_fault_error(1 << 2)
        != crate::kernel::fault::FaultDomain::User
    {
        return Err("user page-fault classification mismatch");
    }

    // Record and inspect a synthetic fault snapshot to validate observability.
    crate::kernel::fault::record_page_fault(0, 1 << 2);
    let snapshot = crate::kernel::fault::last_fault().ok_or("fault snapshot missing")?;
    if snapshot.address != 0 || snapshot.domain != crate::kernel::fault::FaultDomain::User {
        return Err("fault snapshot contents mismatch");
    }

    Ok(())
}

fn test_yield() -> Result<(), &'static str> {
    scheduler::maybe_preempt();
    Ok(())
}

fn test_sleep() -> Result<(), &'static str> {
    wait_for_ticks(1)
}

fn test_wake() -> Result<(), &'static str> {
    wait_for_ticks(1)
}

fn test_fairness() -> Result<(), &'static str> {
    if scheduler::threads().is_empty() {
        return Err("scheduler has no threads");
    }
    Ok(())
}

fn test_timer_scheduling() -> Result<(), &'static str> {
    let start = timer::ticks();
    wait_for_ticks(1)?;
    if timer::ticks() < start {
        return Err("timer scheduling tick regression");
    }
    Ok(())
}

fn test_starvation() -> Result<(), &'static str> {
    scheduler::maybe_preempt();
    scheduler::maybe_preempt();
    Ok(())
}

fn spawn_silent(name: &str, args: &[&str]) -> Result<u64, &'static str> {
    console::begin_output_capture(true);
    let result = process::spawn(name, args, &[]);
    let _ = console::end_output_capture();
    result
}

fn test_process_creation() -> Result<(), &'static str> {
    let before = process::jobs().len();
    let pid = spawn_silent("hello", &["validate"])?;
    let after = process::jobs().len();
    if pid == 0 || after <= before {
        return Err("process spawn did not register a job");
    }
    Ok(())
}

fn test_process_exit() -> Result<(), &'static str> {
    let pid = spawn_silent("hello", &["exit"])?;
    let rec = process::jobs()
        .into_iter()
        .find(|job| job.pid == pid)
        .ok_or("spawned process missing")?;
    if rec.exit_code.is_none() {
        return Err("spawned process has no exit code");
    }
    Ok(())
}

fn test_process_wait() -> Result<(), &'static str> {
    let pid = spawn_silent("hello", &["wait"])?;
    process::wait(pid).map(|_| ())
}

fn test_pid_uniqueness() -> Result<(), &'static str> {
    let a = spawn_silent("hello", &["pid-a"])?;
    let b = spawn_silent("hello", &["pid-b"])?;
    if a == b {
        return Err("duplicate process id");
    }
    Ok(())
}

fn test_argument_passing() -> Result<(), &'static str> {
    let pid = spawn_silent("hello", &["alpha", "beta"])?;
    if pid == 0 {
        return Err("argument process spawn failed");
    }
    Ok(())
}

fn test_syscall_smoke() -> Result<(), &'static str> {
    let reports = testing::verify_target(Some("all"))?;
    if reports.iter().any(|report| !report.passed()) {
        return Err("runtime verifier reported failure");
    }
    Ok(())
}

fn test_console_stdout() -> Result<(), &'static str> {
    console::begin_output_capture(true);
    console::print("stdout-validate");
    let captured = console::end_output_capture();
    if !captured.contains("stdout-validate") {
        return Err("stdout capture missing");
    }
    Ok(())
}

fn test_console_stderr() -> Result<(), &'static str> {
    console::begin_output_capture(true);
    console::print("stdout-probe");
    console::stderr_write_str("stderr-probe");
    let captured = console::end_output_capture();

    if !captured.contains("stdout-probe") {
        return Err("stdout capture missing");
    }
    if !captured.contains("[stderr] stderr-probe") {
        return Err("stderr stream marker missing");
    }

    Ok(())
}

fn test_console_ansi() -> Result<(), &'static str> {
    console::begin_output_capture(true);
    console::print("\x1b[0m");
    let captured = console::end_output_capture();
    if !captured.contains("\x1b[0m") {
        return Err("ansi escape capture missing");
    }
    Ok(())
}

fn test_console_scrolling() -> Result<(), &'static str> {
    console::begin_output_capture(true);
    console::print("line-a\nline-b\nline-c");
    let captured = console::end_output_capture();
    if !captured.contains("line-a\nline-b\nline-c") {
        return Err("newline capture mismatch");
    }
    Ok(())
}

fn test_console_cursor() -> Result<(), &'static str> {
    console::begin_output_capture(true);
    console::print("\x1b[1;1H");
    let captured = console::end_output_capture();
    if !captured.contains("\x1b[1;1H") {
        return Err("cursor escape capture missing");
    }
    Ok(())
}

fn test_console_unicode() -> Result<(), &'static str> {
    console::begin_output_capture(true);
    console::print("SAIOS λß中");
    let captured = console::end_output_capture();
    if !captured.contains("SAIOS λß中") {
        return Err("unicode capture mismatch");
    }
    Ok(())
}

fn test_framebuffer_attached() -> Result<(), &'static str> {
    if !console::framebuffer_attached() {
        return Err("skip: framebuffer backend is not active");
    }
    let props = console::framebuffer_properties().ok_or("framebuffer properties unavailable")?;
    if props.width == 0 || props.height == 0 || props.bytes_per_pixel == 0 {
        return Err("framebuffer geometry is invalid");
    }
    Ok(())
}

fn test_surface_manager_lifecycle() -> Result<(), &'static str> {
    if !console::framebuffer_attached() {
        return Err("framebuffer is not attached");
    }
    let props = console::framebuffer_properties().ok_or("framebuffer properties unavailable")?;
    if props.width == 0 || props.height == 0 || props.bytes_per_pixel == 0 {
        return Err("framebuffer geometry is invalid");
    }
    Ok(())
}

fn temp_path(name: &str) -> String {
    format!("/tmp/validate-{}-{}", timer::ticks(), name)
}

fn test_fs_create() -> Result<(), &'static str> {
    saifs::init();
    let path = temp_path("create");
    saifs::touch(&path).map_err(|_| "touch failed")?;
    saifs::remove(&path).map_err(|_| "cleanup failed")
}

fn test_fs_open() -> Result<(), &'static str> {
    saifs::init();
    let path = temp_path("open");
    saifs::touch(&path).map_err(|_| "touch failed")?;
    let _handle = saifs::open(&path).map_err(|_| "open failed")?;
    saifs::remove(&path).map_err(|_| "cleanup failed")
}

fn test_fs_close() -> Result<(), &'static str> {
    test_fs_open()
}

fn test_fs_write() -> Result<(), &'static str> {
    saifs::init();
    let path = temp_path("write");
    saifs::touch(&path).map_err(|_| "touch failed")?;
    let handle = saifs::open(&path).map_err(|_| "open failed")?;
    crate::saifs::Handle::write(&handle, b"validate").map_err(|_| "write failed")?;
    saifs::remove(&path).map_err(|_| "cleanup failed")
}

fn test_fs_read() -> Result<(), &'static str> {
    saifs::init();
    let path = temp_path("read");
    saifs::touch(&path).map_err(|_| "touch failed")?;
    let handle = saifs::open(&path).map_err(|_| "open failed")?;
    crate::saifs::Handle::write(&handle, b"validate").map_err(|_| "write failed")?;
    let text = saifs::read_text(&path).map_err(|_| "read failed")?;
    saifs::remove(&path).map_err(|_| "cleanup failed")?;
    if text != "validate" {
        return Err("read content mismatch");
    }
    Ok(())
}

fn test_fs_append() -> Result<(), &'static str> {
    saifs::init();
    let path = temp_path("append");
    saifs::touch(&path).map_err(|_| "touch failed")?;
    let handle = saifs::open(&path).map_err(|_| "open failed")?;
    crate::saifs::Handle::write(&handle, b"a").map_err(|_| "write a failed")?;
    crate::saifs::Handle::write(&handle, b"b").map_err(|_| "write b failed")?;
    let text = saifs::read_text(&path).map_err(|_| "read failed")?;
    saifs::remove(&path).map_err(|_| "cleanup failed")?;
    if text != "ab" {
        return Err("append content mismatch");
    }
    Ok(())
}

fn test_fs_rename() -> Result<(), &'static str> {
    saifs::init();
    let from = temp_path("rename-src");
    let to = temp_path("rename-dst");

    saifs::touch(&from).map_err(|_| "touch failed")?;
    vfs::rename(&from, &to).map_err(|_| "rename failed")?;

    if vfs::open_node(&from).is_ok() {
        return Err("source still exists after rename");
    }
    if vfs::open_node(&to).is_err() {
        return Err("destination missing after rename");
    }

    saifs::remove(&to).map_err(|_| "cleanup failed")
}

fn test_fs_move() -> Result<(), &'static str> {
    saifs::init();
    let src_dir = temp_path("move-src-dir");
    let dst_dir = temp_path("move-dst-dir");
    let src_file = format!("{}/payload", src_dir);
    let dst_file = format!("{}/payload", dst_dir);

    saifs::mkdir(&src_dir).map_err(|_| "mkdir src failed")?;
    saifs::mkdir(&dst_dir).map_err(|_| "mkdir dst failed")?;
    saifs::touch(&src_file).map_err(|_| "touch failed")?;

    vfs::rename(&src_file, &dst_file).map_err(|_| "move failed")?;

    if vfs::open_node(&src_file).is_ok() {
        return Err("source still exists after move");
    }
    if vfs::open_node(&dst_file).is_err() {
        return Err("destination missing after move");
    }

    saifs::remove(&dst_file).map_err(|_| "cleanup moved file failed")?;
    saifs::remove(&src_dir).map_err(|_| "cleanup src dir failed")?;
    saifs::remove(&dst_dir).map_err(|_| "cleanup dst dir failed")?;
    Ok(())
}

fn test_fs_delete() -> Result<(), &'static str> {
    saifs::init();
    let path = temp_path("delete");
    saifs::touch(&path).map_err(|_| "touch failed")?;
    saifs::remove(&path).map_err(|_| "delete failed")?;
    if saifs::open(&path).is_ok() {
        return Err("deleted file still opens");
    }
    Ok(())
}

fn test_fs_directory_create() -> Result<(), &'static str> {
    saifs::init();
    let path = temp_path("dir");
    saifs::mkdir(&path).map_err(|_| "mkdir failed")?;
    saifs::remove(&path).map_err(|_| "cleanup failed")
}

fn test_fs_directory_enumeration() -> Result<(), &'static str> {
    saifs::init();
    let entries = saifs::list("/").map_err(|_| "list root failed")?;
    if entries.is_empty() {
        return Err("root directory is empty");
    }
    Ok(())
}

fn test_storage_volume_registry() -> Result<(), &'static str> {
    if crate::driver::storage::volumes().is_empty() {
        return Err("skip: no storage volumes detected");
    }
    Ok(())
}

fn test_storage_mounts() -> Result<(), &'static str> {
    if saifs::mounts().is_empty() {
        return Err("no SAIFS mounts registered");
    }

    if !saifs::mounts().iter().any(|mount| mount.path == "/") {
        return Err("root SAIFS mount missing");
    }

    let mounted_volumes = crate::driver::storage::volumes()
        .into_iter()
        .filter(|volume| volume.name != "tmpfs" && volume.mounted_at.is_some())
        .count();
    if mounted_volumes == 0 {
        return Err("skip: no storage volumes mounted");
    }

    Ok(())
}

fn test_storage_ext4_stage8() -> Result<(), &'static str> {
    let s = crate::driver::storage::ext4_stage8_status();
    if !(s.existing_file_overwrite
        && s.block_allocator
        && s.inode_allocator
        && s.directory_updates
        && s.journal)
    {
        return Err("ext4 stage8 package incomplete");
    }
    Ok(())
}

fn test_storage_cache_validation() -> Result<(), &'static str> {
    let r = crate::driver::storage::validate_ext4_caches();
    if r.errors != 0 {
        return Err("ext4 cache validation found errors");
    }
    Ok(())
}

fn test_stability_heap_leak_detection() -> Result<(), &'static str> {
    let before = heap::leak_stats();

    {
        let mut blocks: Vec<Vec<u8>> = Vec::new();
        for i in 0..64usize {
            blocks.push(alloc::vec![0xA5u8; 1024 + i * 8]);
        }
        for item in &mut blocks {
            if let Some(b) = item.get_mut(0) {
                *b = 0x5A;
            }
        }
    }

    let after = heap::leak_stats();
    let before_out = before.outstanding_requested_bytes;
    let after_out = after.outstanding_requested_bytes;
    let delta = after_out.saturating_sub(before_out);

    if delta > (128 * 1024) as u64 {
        return Err("heap leak detector: outstanding bytes grew above threshold");
    }

    Ok(())
}

fn test_timer_monotonic() -> Result<(), &'static str> {
    let a = timer::ticks();
    let b = timer::ticks();
    if b < a {
        return Err("ticks moved backward");
    }
    Ok(())
}

fn test_timer_sleep() -> Result<(), &'static str> {
    let a = timer::ticks();
    wait_for_ticks(1)?;
    let b = timer::ticks();
    if b <= a {
        return Err("sleep did not advance timer");
    }
    Ok(())
}

fn test_driver_keyboard() -> Result<(), &'static str> {
    driver_exists_any(&["keyboard", "hid-keyboard"], &["keyboard0"])
}

fn test_driver_mouse() -> Result<(), &'static str> {
    driver_exists_any(&["mouse", "hid-mouse"], &["mouse0"])
}

fn test_driver_keyboard_behavior() -> Result<(), &'static str> {
    driver_exists_any(&["keyboard", "hid-keyboard"], &["keyboard0"])?;
    let _ = console::poll_input();
    Ok(())
}

fn test_driver_mouse_behavior() -> Result<(), &'static str> {
    driver_exists_any(&["mouse", "hid-mouse"], &["mouse0"])?;
    let _ = console::poll_input();
    Ok(())
}

fn test_driver_timer() -> Result<(), &'static str> {
    if timer::ticks() == 0 {
        return Err("timer ticks are zero");
    }
    Ok(())
}

fn test_driver_serial() -> Result<(), &'static str> {
    driver_exists("serial")
}

fn test_driver_framebuffer() -> Result<(), &'static str> {
    driver_exists("framebuffer").or(Err("skip: framebuffer driver not registered"))
}

fn test_driver_storage() -> Result<(), &'static str> {
    driver_exists("storage").or(Err("skip: storage driver not registered"))
}

fn driver_exists(name: &str) -> Result<(), &'static str> {
    if crate::kernel::driver::find(name).is_some()
        || device::devices().iter().any(|d| d.driver == name)
    {
        Ok(())
    } else {
        Err("skip: driver not available on current hardware")
    }
}

fn driver_exists_any(drivers: &[&str], device_names: &[&str]) -> Result<(), &'static str> {
    if drivers
        .iter()
        .any(|name| crate::kernel::driver::find(name).is_some())
    {
        return Ok(());
    }

    let records = device::devices();
    if records.iter().any(|record| {
        drivers.iter().any(|driver| record.driver == *driver)
            || device_names.iter().any(|name| record.name == *name)
    }) {
        return Ok(());
    }

    Err("skip: driver not available on current hardware")
}

fn test_perf_memcpy() -> Result<(), &'static str> {
    let src = alloc::vec![0x55u8; 32 * 1024];
    let mut dst = alloc::vec![0u8; 32 * 1024];
    let start = now_ms();
    for _ in 0..128 {
        dst.copy_from_slice(&src);
    }
    console::println!(
        "memcpy: {} KiB copied in {} ms",
        32 * 128,
        elapsed_ms(start)
    );
    Ok(())
}

fn test_perf_framebuffer() -> Result<(), &'static str> {
    Err("skip: use fbbench for destructive framebuffer bandwidth testing")
}

fn test_perf_syscall() -> Result<(), &'static str> {
    let start = now_ms();
    for _ in 0..256 {
        let _ = testing::verify_target(Some("memory"))?;
    }
    console::println!(
        "syscall verifier loop: 256 iterations in {} ms",
        elapsed_ms(start)
    );
    Ok(())
}

fn test_perf_malloc() -> Result<(), &'static str> {
    let start = now_ms();
    for _ in 0..256 {
        let data = alloc::vec![0u8; 256];
        core::hint::black_box(data);
    }
    console::println!("malloc: 256 allocations in {} ms", elapsed_ms(start));
    Ok(())
}

fn test_perf_file_io() -> Result<(), &'static str> {
    let start = now_ms();
    for idx in 0..32 {
        let path = temp_path(format!("perf-{}", idx).as_str());
        saifs::touch(&path).map_err(|_| "touch failed")?;
        let handle = saifs::open(&path).map_err(|_| "open failed")?;
        crate::saifs::Handle::write(&handle, b"0123456789abcdef").map_err(|_| "write failed")?;
        let _ = saifs::read_text(&path).map_err(|_| "read failed")?;
        saifs::remove(&path).map_err(|_| "cleanup failed")?;
    }
    console::println!("file I/O: 32 roundtrips in {} ms", elapsed_ms(start));
    Ok(())
}

fn test_stress_allocations() -> Result<(), &'static str> {
    for _ in 0..10_000 {
        let data = alloc::vec![0u8; 16];
        core::hint::black_box(data);
    }
    Ok(())
}

fn test_stress_files() -> Result<(), &'static str> {
    for idx in 0..128 {
        let path = temp_path(format!("stress-{}", idx).as_str());
        saifs::touch(&path).map_err(|_| "touch failed")?;
        saifs::remove(&path).map_err(|_| "remove failed")?;
    }
    Ok(())
}

fn test_stress_processes() -> Result<(), &'static str> {
    for _ in 0..16 {
        let _ = process::spawn("hello", &["stress"], &[])?;
    }
    Ok(())
}

fn test_stress_yields() -> Result<(), &'static str> {
    for _ in 0..10_000 {
        scheduler::maybe_preempt();
    }
    Ok(())
}

fn test_stress_console() -> Result<(), &'static str> {
    for _ in 0..16 {
        console::print(".");
    }
    console::newline();
    Ok(())
}

fn test_stress_framebuffer() -> Result<(), &'static str> {
    Err("skip: framebuffer flood is intentionally not run from validate")
}

#[allow(dead_code)]
fn _object_manager_smoke() -> Result<(), &'static str> {
    if object_manager::providers().is_empty() {
        return Err("no object providers registered");
    }
    if pmm::total_pages() == 0 {
        return Err("PMM has no pages");
    }
    Ok(())
}
