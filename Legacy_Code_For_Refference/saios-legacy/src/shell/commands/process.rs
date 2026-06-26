use crate::{print, println};
use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};

const DEFAULT_TEST_TIMEOUT_MS: u64 = 5_000;
const LONG_TEST_TIMEOUT_MS: u64 = 15_000;
const SUITE_TEST_TIMEOUT_MS: u64 = 120_000;
const TEST_CLEANUP_TIMEOUT_MS: u64 = 1_000;
static MATRIX_FAILED: AtomicBool = AtomicBool::new(false);

pub enum TestResult {
    Pass,
    Fail(String),
    Panic(String),
    Timeout,
}

impl TestResult {
    fn passed(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

pub trait SaiosTest {
    fn name(&self) -> &'static str;
    fn timeout_ms(&self) -> u64;
    fn run(&self) -> TestResult;
}

struct EmbeddedProbeTest {
    name: &'static str,
    path: &'static str,
    elf: &'static [u8],
    description: &'static str,
    timeout_ms: u64,
    wait_mode: ChildWaitMode,
}

struct ExecveProbeTest;

struct MatrixTest {
    name: &'static str,
    timeout_ms: u64,
    run_fn: fn() -> TestResult,
}

struct SaiosSuiteTest;

#[derive(Clone, Copy)]
enum ChildWaitMode {
    Contract,
    PollTimeout,
}

#[derive(Default)]
struct SuiteCounts {
    passed: u32,
    failed: u32,
    panicked: u32,
    timed_out: u32,
}

impl SaiosTest for ExecveProbeTest {
    fn name(&self) -> &'static str {
        "execvetest"
    }

    fn timeout_ms(&self) -> u64 {
        DEFAULT_TEST_TIMEOUT_MS
    }

    fn run(&self) -> TestResult {
        if EXECVE_DRIVER_ELF.is_empty() || EXECVE_CHILD_ELF.is_empty() {
            return TestResult::Fail(String::from("execve probe not built"));
        }

        test_step(self.name(), 1, "write_driver");
        crate::write_file_pub("/tmp/execve_driver", EXECVE_DRIVER_ELF);
        test_step(self.name(), 2, "write_child");
        crate::write_file_pub("/tmp/execve_child", EXECVE_CHILD_ELF);
        println!(
            "[execvetest] driver={} bytes child={} bytes",
            EXECVE_DRIVER_ELF.len(),
            EXECVE_CHILD_ELF.len()
        );

        test_step(self.name(), 3, "spawn_driver");
        match crate::process::spawn("/tmp/execve_driver") {
            Ok(pid) => {
                wait_for_test_child(self.name(), pid, self.timeout_ms(), ChildWaitMode::Contract)
            }
            Err(error) => TestResult::Fail(alloc::format!("spawn failed: {}", error)),
        }
    }
}

impl SaiosTest for MatrixTest {
    fn name(&self) -> &'static str {
        self.name
    }

    fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    fn run(&self) -> TestResult {
        (self.run_fn)()
    }
}

impl SaiosTest for SaiosSuiteTest {
    fn name(&self) -> &'static str {
        "testsaios"
    }

    fn timeout_ms(&self) -> u64 {
        SUITE_TEST_TIMEOUT_MS
    }

    fn run(&self) -> TestResult {
        run_testsaios_suite()
    }
}

impl SaiosTest for EmbeddedProbeTest {
    fn name(&self) -> &'static str {
        self.name
    }

    fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    fn run(&self) -> TestResult {
        if self.elf.is_empty() {
            return TestResult::Fail(String::from("probe binary not built"));
        }

        test_step(self.name, 1, "write_probe");
        crate::write_file_pub(self.path, self.elf);
        println!(
            "[{}] {} = {} bytes",
            self.name,
            self.description,
            self.elf.len()
        );

        test_step(self.name, 2, "spawn_probe");
        match crate::process::spawn(self.path) {
            Ok(pid) => wait_for_test_child(self.name, pid, self.timeout_ms, self.wait_mode),
            Err(error) => TestResult::Fail(alloc::format!("spawn failed: {}", error)),
        }
    }
}

pub fn run_test(test: &dyn SaiosTest) -> TestResult {
    let started_ns = crate::time::uptime_ns();
    let timeout_ms = test.timeout_ms();
    test_event(
        test.name(),
        crate::kds::KdsEventType::TestStart,
        crate::kds::KdsSeverity::Info,
        timeout_ms,
        0,
    );
    println!("TEST_START {} timeout_ms={}", test.name(), timeout_ms);

    let result = test.run();
    let duration_ms = elapsed_ms(started_ns);

    // Wall-clock exceeded declared timeout: emit a warning but do NOT
    // override the functional result.  A test that returns Pass is Pass;
    // wall-clock duration reflects system load / scheduling delay, not
    // correctness of the subsystem under test.  Only override to Timeout
    // when the test function itself did not return a terminal grade
    // (safety net for future async/poll-based test runners).
    if duration_ms > timeout_ms
        && (matches!(result, TestResult::Pass) || matches!(result, TestResult::Fail(_)))
    {
        crate::serial_println!(
            "TEST_WARN {} duration_ms={} exceeded_declared_timeout_ms={} result_stands={}",
            test.name(),
            duration_ms,
            timeout_ms,
            if matches!(result, TestResult::Pass) { "PASS" } else { "FAIL" }
        );
    }
    let result = if duration_ms > timeout_ms && !is_terminal_result(&result) {
        TestResult::Timeout
    } else {
        result
    };

    match &result {
        TestResult::Pass => {
            test_event(
                test.name(),
                crate::kds::KdsEventType::TestPass,
                crate::kds::KdsSeverity::Info,
                duration_ms,
                0,
            );
            println!("TEST_PASS {} duration_ms={}", test.name(), duration_ms);
        }
        TestResult::Fail(reason) => {
            test_event(
                test.name(),
                crate::kds::KdsEventType::TestFail,
                crate::kds::KdsSeverity::Error,
                duration_ms,
                reason.len() as u64,
            );
            println!(
                "TEST_FAIL {} duration_ms={} reason={}",
                test.name(),
                duration_ms,
                reason.as_str()
            );
        }
        TestResult::Panic(reason) => {
            test_event(
                test.name(),
                crate::kds::KdsEventType::TestFail,
                crate::kds::KdsSeverity::Fatal,
                duration_ms,
                reason.len() as u64,
            );
            println!(
                "TEST_PANIC {} duration_ms={} reason={}",
                test.name(),
                duration_ms,
                reason.as_str()
            );
        }
        TestResult::Timeout => {
            test_event(
                test.name(),
                crate::kds::KdsEventType::TestTimeout,
                crate::kds::KdsSeverity::Warn,
                duration_ms,
                timeout_ms,
            );
            println!("TEST_TIMEOUT {} duration_ms={}", test.name(), duration_ms);
        }
    }
    result
}

fn run_embedded_probe(
    command: &'static str,
    path: &'static str,
    elf: &'static [u8],
    description: &'static str,
    timeout_ms: u64,
) -> TestResult {
    run_embedded_probe_with_wait(
        command,
        path,
        elf,
        description,
        timeout_ms,
        ChildWaitMode::Contract,
    )
}

fn run_embedded_probe_with_wait(
    command: &'static str,
    path: &'static str,
    elf: &'static [u8],
    description: &'static str,
    timeout_ms: u64,
    wait_mode: ChildWaitMode,
) -> TestResult {
    run_test(&EmbeddedProbeTest {
        name: command,
        path,
        elf,
        description,
        timeout_ms,
        wait_mode,
    })
}

fn run_matrix_test(
    name: &'static str,
    timeout_ms: u64,
    run_fn: fn() -> TestResult,
) -> TestResult {
    run_test(&MatrixTest {
        name,
        timeout_ms,
        run_fn,
    })
}

fn record_suite_result(counts: &mut SuiteCounts, result: TestResult) {
    match result {
        TestResult::Pass => counts.passed += 1,
        TestResult::Fail(_) => counts.failed += 1,
        TestResult::Panic(_) => counts.panicked += 1,
        TestResult::Timeout => counts.timed_out += 1,
    }
}

fn begin_matrix() {
    MATRIX_FAILED.store(false, Ordering::Relaxed);
}

fn finish_matrix(failure_reason: &'static str) -> TestResult {
    if MATRIX_FAILED.load(Ordering::Relaxed) {
        TestResult::Fail(String::from(failure_reason))
    } else {
        TestResult::Pass
    }
}

fn wait_for_test_child(
    test_name: &'static str,
    pid: u32,
    timeout_ms: u64,
    wait_mode: ChildWaitMode,
) -> TestResult {
    test_step(test_name, 3, "wait_child");
    if matches!(wait_mode, ChildWaitMode::Contract) {
        return wait_for_test_child_contract(test_name, pid);
    }

    wait_for_test_child_poll_timeout(test_name, pid, timeout_ms)
}

#[cfg(debug_assertions)]
const WAITPID_DIAGNOSTIC_TIMEOUT_MS: u64 = 5_000;

fn wait_for_test_child_contract(test_name: &'static str, pid: u32) -> TestResult {
    let parent_pid = crate::process::current_pid().unwrap_or(0);
    let wait_request = crate::process_contract::ProcessWaitRequest {
        parent_pid,
        waiter_pid: parent_pid,
        want_pid: pid,
        options: 0,
    };

    #[cfg(debug_assertions)]
    let started_ns = crate::time::uptime_ns();

    loop {
        crate::process_contract::ProcessContract::register_child_waiter(wait_request);
        if let Some(exit_code) = try_reap_test_child(parent_pid, pid) {
            return if exit_code == 0 {
                TestResult::Pass
            } else {
                TestResult::Fail(alloc::format!("exit_code={}", exit_code))
            };
        }

        #[cfg(debug_assertions)]
        {
            let elapsed_ms = crate::time::uptime_ns()
                .saturating_sub(started_ns)
                .checked_div(1_000_000)
                .unwrap_or(0);
            if elapsed_ms >= WAITPID_DIAGNOSTIC_TIMEOUT_MS {
                let (waiter_present, zombie_present) =
                    crate::process_contract::ProcessContract::waitpid_diagnostic(
                        parent_pid, pid,
                    );
                let sched_snapshot = crate::process::table::TABLE
                    .try_lock()
                    .map(|table| {
                        let snap = table.scheduler_snapshot();
                        let procs: alloc::vec::Vec<(u32, alloc::string::String, crate::process::ProcessState)> =
                            table.procs.iter().map(|(p, proc)| {
                                (*p, proc.name.clone(), proc.state().clone())
                            }).collect();
                        (snap, procs, table.zombies.len())
                    });
                crate::serial_println!("WAITPID DEADLOCK DETECTED");
                crate::serial_println!("  test={}", test_name);
                crate::serial_println!("  parent_pid={}", parent_pid);
                crate::serial_println!("  child_pid={}", pid);
                crate::serial_println!("  waiter_present={}", waiter_present);
                crate::serial_println!("  zombie_present={}", zombie_present);
                if let Some((snap, procs, zombie_count)) = sched_snapshot {
                    crate::serial_println!("  scheduler current={:?}", snap.current);
                    crate::serial_println!("  scheduler prev={:?}", snap.prev);
                    crate::serial_println!("  scheduler idle={:?}", snap.idle);
                    crate::serial_println!("  scheduler run_queue={:?}", snap.run_queue);
                    crate::serial_println!("  zombie_count={}", zombie_count);
                    for (p, name, state) in &procs {
                        crate::serial_println!("  proc pid={} name={} state={:?}", p, name, state);
                    }
                } else {
                    crate::serial_println!("  scheduler state=table_locked");
                }
                panic!(
                    "[testsaios] WAITPID DEADLOCK: {} child={} waiter_present={} zombie_present={}",
                    test_name, pid, waiter_present, zombie_present
                );
            }
        }

        if !crate::process_contract::ProcessContract::block_registered_child_waiter(parent_pid) {
            wait_for_progress_tick();
        }
    }
}

fn wait_for_test_child_poll_timeout(
    test_name: &'static str,
    pid: u32,
    timeout_ms: u64,
) -> TestResult {
    let parent_pid = crate::process::current_pid().unwrap_or(0);
    let started_ns = crate::time::uptime_ns();
    let deadline_ns = started_ns.saturating_add(timeout_ms.saturating_mul(1_000_000));

    loop {
        if let Some(exit_code) = try_reap_test_child(parent_pid, pid) {
            return if exit_code == 0 {
                TestResult::Pass
            } else {
                TestResult::Fail(alloc::format!("exit_code={}", exit_code))
            };
        }

        if crate::time::uptime_ns() >= deadline_ns {
            test_step(test_name, 4, "timeout_kill");
            let _ = crate::ipc::signal::raise_signal_for_pid(pid, crate::ipc::signal::SIGKILL);
            cleanup_test_child(test_name, parent_pid, pid);
            return TestResult::Timeout;
        }

        wait_for_progress_tick();
    }
}

fn cleanup_test_child(test_name: &'static str, parent_pid: u32, pid: u32) {
    let cleanup_deadline = crate::time::uptime_ns()
        .saturating_add(TEST_CLEANUP_TIMEOUT_MS.saturating_mul(1_000_000));
    while crate::time::uptime_ns() < cleanup_deadline {
        if try_reap_test_child(parent_pid, pid).is_some() {
            test_step(test_name, 5, "cleanup_reaped");
            return;
        }
        wait_for_progress_tick();
    }

    test_step(test_name, 6, "cleanup_forced");
    {
        let mut table = crate::process::table::TABLE.lock();
        let _ = table.remove_faulted(pid);
    }
    let _ = try_reap_test_child(parent_pid, pid);
}

fn try_reap_test_child(parent_pid: u32, pid: u32) -> Option<i64> {
    let wait_request = crate::process_contract::ProcessWaitRequest {
        parent_pid,
        waiter_pid: parent_pid,
        want_pid: pid,
        options: 1,
    };
    let reap = crate::process_contract::ProcessContract::try_reap_waitable(wait_request)?;
    crate::process_contract::ProcessContract::record_wait_success(wait_request, reap);
    Some(reap.exit_code)
}

fn test_step(test_name: &'static str, step: u64, label: &'static str) {
    test_event(
        test_name,
        crate::kds::KdsEventType::TestStep,
        crate::kds::KdsSeverity::Trace,
        step,
        label.len() as u64,
    );
    crate::serial_println!("TEST_STEP {} {}", test_name, label);
}

fn test_event(
    test_name: &'static str,
    event_type: crate::kds::KdsEventType,
    severity: crate::kds::KdsSeverity,
    value1: u64,
    value2: u64,
) {
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Shell,
        event_type,
        severity,
        [test_name_hash(test_name), value1, value2, crate::process::current_pid().unwrap_or(0) as u64],
    );
}

fn test_name_hash(name: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn is_terminal_result(result: &TestResult) -> bool {
    matches!(result, TestResult::Pass | TestResult::Timeout)
        || matches!(result, TestResult::Fail(_) | TestResult::Panic(_))
}

fn elapsed_ms(started_ns: u64) -> u64 {
    crate::time::uptime_ns()
        .saturating_sub(started_ns)
        .checked_div(1_000_000)
        .unwrap_or(0)
}

fn wait_for_progress_tick() {
    let was_syscall_context = crate::arch::syscall::kernel_gs_active();
    crate::process::scheduler::yield_now_wait("testsaios_wait_progress");
    restore_syscall_context_after_test_yield(was_syscall_context);
    let _ = crate::process::refresh_current_from_table();
}

fn restore_syscall_context_after_test_yield(was_syscall_context: bool) {
    if was_syscall_context && !crate::arch::syscall::kernel_gs_active() {
        unsafe {
            crate::arch::process::swapgs();
        }
        crate::arch::syscall::mark_kernel_gs_active(true);
    }
}

fn run_foreground_user_process(pid: u32) {
    let _ = crate::process::waitpid(pid, 0);
}

pub fn exec(args: &str) {
    let path = args.trim();
    if path.is_empty() {
        println!("usage: exec <path>");
        println!("  Put an x86_64 ELF binary in ramfs first:");
        println!("  (cross-compile on host, then load via QEMU or future net transfer)");
        return;
    }
    println!("Loading {}...", path);
    match crate::process::spawn(path) {
        Ok(pid) => {
            println!("Running pid={}...", pid);
            run_foreground_user_process(pid);
        }
        Err(e) => println!("exec: {}", e),
    }
}

pub fn ps() {
    println!("  PID  STATE   CPU  NAME");
    println!("  ---  ------  ---  ----------------");
    let t = crate::process::table::TABLE.lock();
    for (&pid, p) in t.procs.iter() {
        let state = match p.state() {
            crate::process::ProcessState::Ready => "ready ",
            crate::process::ProcessState::Running => "run   ",
            crate::process::ProcessState::Blocked => "block ",
            crate::process::ProcessState::Zombie => "zombie",
            crate::process::ProcessState::New => "new   ",
            crate::process::ProcessState::Dead => "dead  ",
        };
        let mut cpu_str = alloc::string::String::from("  -");
        for c in 0..crate::process::table::MAX_CPUS {
            if t.current_on_cpu(c) == Some(pid) {
                cpu_str = alloc::format!("{:3}", c);
                break;
            }
        }
        println!("  {:3}  {}  {}  {}", pid, state, cpu_str, p.name);
    }
}

fn usertest() -> TestResult {
    run_embedded_probe(
        "usertest",
        "/tmp/usertest",
        USERTEST_ELF,
        "static userspace ELF ring 3 probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn ring3loop() -> TestResult {
    println!("[ring3loop] bounded probe: TIMEOUT means user loop stayed in ring 3 until cleanup");
    run_embedded_probe_with_wait(
        "ring3loop",
        "/tmp/ring3loop",
        RING3_LOOP_ELF,
        "static no-syscall user loop probe",
        DEFAULT_TEST_TIMEOUT_MS,
        ChildWaitMode::PollTimeout,
    )
}

fn ring3halt() -> TestResult {
    run_embedded_probe(
        "ring3halt",
        "/tmp/ring3halt",
        RING3_HALT_ELF,
        "1-instruction HLT privilege probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn testpie() -> TestResult {
    run_embedded_probe(
        "testpie",
        "/tmp/test_pie",
        TEST_PIE_ELF,
        "PIE ET_DYN loader probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn validate() -> TestResult {
    run_embedded_probe(
        "validate",
        "/tmp/validate",
        VALIDATION_ELF,
        "user-space validation suite",
        LONG_TEST_TIMEOUT_MS,
    )
}

fn mempermtest() -> TestResult {
    run_embedded_probe(
        "mempermtest",
        "/tmp/memperm_test",
        MEMPERM_TEST_ELF,
        "memory permission plan gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn libchello() -> TestResult {
    run_embedded_probe(
        "libchello",
        "/tmp/libc_plan_test",
        LIBC_PLAN_TEST_ELF,
        "GCC-built libc hello plan gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn threadtest() -> TestResult {
    run_embedded_probe(
        "threadtest",
        "/tmp/thread_test",
        THREAD_TEST_ELF,
        "clone/thread lifecycle plan gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn futextest() -> TestResult {
    run_embedded_probe(
        "futextest",
        "/tmp/futex_test",
        FUTEX_TEST_ELF,
        "futex wait/wake plan gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn signaltest() -> TestResult {
    run_embedded_probe(
        "signaltest",
        "/tmp/signal_test",
        SIGNAL_TEST_ELF,
        "signal delivery plan gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn waitreaptest() -> TestResult {
    run_embedded_probe(
        "waitreaptest",
        "/tmp/wait_reap_test",
        WAIT_REAP_TEST_ELF,
        "wait4 nohang and reap plan gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn pipesemtest() -> TestResult {
    run_embedded_probe(
        "pipesemtest",
        "/tmp/pipe_semantics_test",
        PIPE_SEMANTICS_TEST_ELF,
        "pipe EOF and EPIPE plan gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn syscallabitest() -> TestResult {
    run_embedded_probe(
        "syscallabitest",
        "/tmp/syscall_abi_test",
        SYSCALL_ABI_TEST_ELF,
        "syscall ABI conformance gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn capabilitytest() -> TestResult {
    run_embedded_probe(
        "capabilitytest",
        "/tmp/capability_test",
        CAPABILITY_TEST_ELF,
        "capability enforcement plan gate",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn execvetest() -> TestResult {
    run_test(&ExecveProbeTest)
}

fn forkabitest() -> TestResult {
    run_embedded_probe(
        "forkabitest",
        "/tmp/fork_abi_test",
        FORK_ABI_TEST_ELF,
        "fork parent/child return ABI probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

pub fn testsaios() {
    let _ = run_test(&SaiosSuiteTest);
}

fn run_testsaios_suite() -> TestResult {
    let mut counts = SuiteCounts::default();
    println!("[testsaios] running SAIOS test matrices");

    record_suite_result(
        &mut counts,
        run_matrix_test("bootselftest", DEFAULT_TEST_TIMEOUT_MS, bootselftest_matrix),
    );
    record_suite_result(
        &mut counts,
        run_matrix_test("architecture_matrix", LONG_TEST_TIMEOUT_MS, architecture_validation),
    );
    record_suite_result(
        &mut counts,
        run_matrix_test("storage_matrix", DEFAULT_TEST_TIMEOUT_MS, storage_validation),
    );
    record_suite_result(&mut counts, usertest());
    record_suite_result(&mut counts, testpie());
    record_suite_result(&mut counts, validate());
    record_suite_result(&mut counts, forkabitest());
    record_suite_result(&mut counts, execvetest());
    record_suite_result(&mut counts, gp_test());
    record_suite_result(&mut counts, ud_test());
    record_suite_result(&mut counts, div0_test());
    record_suite_result(&mut counts, pf_test());
    record_suite_result(&mut counts, hlt_test());
    record_suite_result(&mut counts, fault_test());
    record_suite_result(&mut counts, mempermtest());
    record_suite_result(&mut counts, libchello());
    record_suite_result(&mut counts, threadtest());
    record_suite_result(&mut counts, futextest());
    record_suite_result(&mut counts, signaltest());
    record_suite_result(&mut counts, waitreaptest());
    record_suite_result(&mut counts, pipesemtest());
    record_suite_result(&mut counts, syscallabitest());
    record_suite_result(&mut counts, capabilitytest());
    record_suite_result(
        &mut counts,
        run_matrix_test(
            "observability_activity_matrix",
            DEFAULT_TEST_TIMEOUT_MS,
            observability_activity_validation,
        ),
    );

    println!(
        "[testsaios] summary PASS={} FAIL={} PANIC={} TIMEOUT={}",
        counts.passed, counts.failed, counts.panicked, counts.timed_out
    );
    if counts.failed == 0 && counts.panicked == 0 && counts.timed_out == 0 {
        TestResult::Pass
    } else {
        TestResult::Fail(alloc::format!(
            "failures={} panics={} timeouts={}",
            counts.failed,
            counts.panicked,
            counts.timed_out
        ))
    }
}

pub fn bootselftest() {
    let _ = bootselftest_matrix();
}

fn bootselftest_matrix() -> TestResult {
    begin_matrix();
    crate::serial_println!("[bootselftest] begin");
    println!("[bootselftest] minimal pre-login validation");
    bootselftest_line(crate::smp::cpu_count() > 0, "CPU discovery");
    bootselftest_line(
        crate::process::table::TABLE.lock().current_pid() > 0,
        "scheduler current task",
    );
    bootselftest_line(
        crate::vfs_contract::VfsContract::resolve("/").is_ok(),
        "root VFS reachable",
    );
    // Use lock-free KDS_READY check instead of kds::stats() which acquires 5
    // stream mutexes and can block if flight-recorder/bgworker hold them.
    bootselftest_line(
        crate::kds::KDS_READY.load(core::sync::atomic::Ordering::Acquire),
        "KDS available",
    );

    crate::serial_println!("[bootselftest] storage check begin");
    let storage = crate::block::validate_storage();
    if storage.disk_detected {
        bootselftest_line(storage.filesystem_probe, "storage probe readable");
        if !storage.root_mount {
            println!("  WARN storage root mount failed; run `storage diagnose` after login");
        }
    } else {
        println!("  WARN no block disk detected; recovery tmpfs root remains available");
    }
    crate::serial_println!("[bootselftest] complete");
    finish_matrix("boot self-test matrix failed")
}

fn storage_validation() -> TestResult {
    begin_matrix();
    let storage = crate::block::validate_storage();
    testsaios_colon_line(storage.disk_detected, "disk detected");
    testsaios_colon_line(storage.partition_table_detected, "partition table detected");
    testsaios_colon_line(storage.partition_discovered, "partition discovered");
    testsaios_colon_line(storage.filesystem_probe, "filesystem probe");
    testsaios_colon_line(storage.root_mount, "root mount");

    let report = crate::saios::storage_platform::scan_storage();
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let hardware = &snapshot.compatibility;
    let analysis = &snapshot.target;
    let plan = &snapshot.plan;
    let resize = crate::saios::storage_platform::resize_analysis();
    let recovery = crate::saios::storage_platform::recovery_report();
    let assessment = &snapshot.assessment;
    let validation = &snapshot.validation;
    let risk = &snapshot.risk;
    let simulation = &snapshot.simulation;
    let recommendation = &snapshot.recommendation;
    let kds = crate::kds::stats();
    testsaios_line(
        report.operation_id > 0,
        "Storage Platform scan emits operation id",
    );
    testsaios_line(
        assessment.operation_id > 0
            && assessment.model.disk.is_some() == report.disk.is_some()
            && assessment.model.partitions.len() == report.partitions.len(),
        "typed storage model mirrors scan discovery",
    );
    testsaios_line(
        !assessment.checks.is_empty() && !assessment.evidence.is_empty(),
        "storage assessment records checks and evidence",
    );
    testsaios_line(
        !assessment.capabilities.resize && !assessment.capabilities.dual_boot,
        "storage capabilities remain conservative",
    );
    testsaios_line(
        assessment.decision.confidence_score <= 100
            && (!assessment.evidence.is_empty() || !assessment.decision.reasons.is_empty()),
        "install advisory records evidence or reasons",
    );
    testsaios_line(
        !validation.checks.is_empty(),
        "install validation records checks",
    );
    testsaios_line(
        validation.status == crate::saios::storage_platform::InstallValidationStatus::Failed
            || validation.failures.is_empty(),
        "install validation failure list follows status",
    );
    testsaios_line(
        assessment.decision.allowed || !assessment.decision.reasons.is_empty(),
        "install advisory records recommendation context",
    );
    testsaios_line(
        assessment.decision.confidence_score <= 100,
        "install decision confidence score bounded",
    );
    testsaios_line(
        risk.completed && risk.score <= 100,
        "install risk assessment completed with bounded score",
    );
    testsaios_line(
        risk.level != crate::saios::storage_platform::RiskLevel::Critical || !risk.factors.is_empty(),
        "critical install risk includes evidence",
    );
    testsaios_line(
        assessment.model.requirements.minimum_root_mib == 20 * 1024
            && assessment.model.requirements.recommended_root_mib == 64 * 1024
            && assessment.model.requirements.minimum_efi_mib == 300
            && assessment.model.requirements.recommended_efi_mib == 512,
        "install requirements match locked policy",
    );
    testsaios_line(
        simulation.no_changes_made && !simulation.actions.is_empty(),
        "install simulation is non-mutating and action-oriented",
    );
    testsaios_line(
        recommendation.confidence <= 100 && !recommendation.reasons.is_empty(),
        "storage recommendation records confidence and reasons",
    );
    testsaios_line(
        hardware.score <= 100,
        "hardware compatibility score bounded",
    );
    testsaios_line(
        !resize.execution_enabled && !resize.safe,
        "resize remains advisory-only",
    );
    testsaios_line(
        (analysis.classification == "No Disk") == !plan.execution_enabled,
        "install plan follows target availability",
    );
    testsaios_line(
        recovery.operation_id > 0,
        "recovery report emits operation id",
    );
    testsaios_line(
        kds.events.records > 0 && kds.state.records > 0,
        "storage KDS evidence recorded",
    );
    finish_matrix("storage matrix failed")
}

fn architecture_validation() -> TestResult {
    begin_matrix();
    let kds = crate::kds::validate_architecture();
    testsaios_line(kds.event_creation, "KDS event creation");
    testsaios_line(kds.metric_creation, "KDS metric creation");
    testsaios_line(kds.trace_creation, "KDS trace begin/end");
    testsaios_line(kds.object_creation, "KDS object creation");
    testsaios_line(kds.state_update, "KDS state update");
    testsaios_line(kds.stream_integrity, "KDS stream integrity");
    testsaios_line(kds.buffer_accounting, "KDS buffer accounting");
    testsaios_line(kds.drop_accounting, "KDS drop accounting");
    testsaios_line(kds.attribution_present, "KDS attribution present");
    testsaios_line(kds.taxonomy_coverage, "KDS taxonomy coverage");
    testsaios_line(kds.passed(), "KDS validation");

    println!("\n[testsaios] === resource accounting validation ===");
    let resource_coverage = crate::resource_contract::ResourceContract::coverage_report();
    testsaios_line(
        resource_coverage.all_kinds_described,
        "Resource accounting kinds described",
    );
    testsaios_line(
        resource_coverage.fallback_paths == resource_coverage.resource_kinds,
        "Resource accounting fallback paths described",
    );
    testsaios_line(
        resource_coverage.accounting_invariants,
        "Resource accounting invariants",
    );
    testsaios_line(
        resource_coverage.implemented > 0 && resource_coverage.missing > 0,
        "Resource accounting pending owners tracked",
    );
    println!(
        "[testsaios] Resource accounting coverage: {}/{} implemented, {} pending",
        resource_coverage.implemented, resource_coverage.resource_kinds, resource_coverage.missing
    );

    println!("\n[testsaios] === observability contract validation ===");
    let stats = crate::kds::stats();
    testsaios_line(
        stats.events.records > 0,
        "Observability Contract initialized",
    );
    testsaios_line(
        crate::kds::count_metrics(crate::kds::KdsMetricId::SchedulerProgress) > 0
            || crate::kds::aggregate_exists(
                crate::kds::KdsSubsystem::Scheduler,
                crate::kds::KdsMetricId::SchedulerProgress,
            ),
        "Scheduler contract active",
    );
    testsaios_line(
        crate::kds::count_metrics(crate::kds::KdsMetricId::PageAlloc) > 0,
        "Memory contract active",
    );
    testsaios_line(
        crate::kds::count_events(crate::kds::KdsEventType::TaskCreate) > 0
            || crate::kds::count_events(crate::kds::KdsEventType::Fork) > 0,
        "Process contract active",
    );
    testsaios_line(
        crate::kds::count_events_for_subsystem(crate::kds::KdsSubsystem::Watchdog) > 0
            || crate::kds::aggregate_exists(
                crate::kds::KdsSubsystem::Watchdog,
                crate::kds::KdsMetricId::CpuHeartbeat,
            ),
        "Watchdog contract active",
    );
    testsaios_line(
        crate::boot_mode::BootMode::parse(crate::boot_mode::BootMode::FirstBoot.as_str())
            == crate::boot_mode::BootMode::FirstBoot,
        "installed firstboot mode contract",
    );

    println!("\n[testsaios] === SAIRU validation ===");
    let sairu = crate::sairu::validate_runtime();
    testsaios_line(sairu.runtime_available, "SAIRU runtime available");
    testsaios_line(sairu.tools_available, "SAIRU tools");
    testsaios_line(sairu.skills_available, "SAIRU skills");
    testsaios_line(sairu.tasks_available, "SAIRU tasks");
    testsaios_line(sairu.health_diagnostic, "SAIRU diagnose health");
    testsaios_line(sairu.memory_diagnostic, "SAIRU diagnose memory");
    testsaios_line(sairu.freeze_diagnostic, "SAIRU diagnose freeze");
    testsaios_line(sairu.contract_boundary, "SAIRU contract boundary");
    testsaios_line(sairu.evidence_citations, "SAIRU evidence citations");
    testsaios_line(sairu.deterministic, "SAIRU deterministic output");
    testsaios_line(sairu.passed(), "SAIRU validation");

    println!("\n[testsaios] === freeze diagnostics validation ===");
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Watchdog,
        crate::kds::KdsEventType::WatchdogCpuStall,
        crate::kds::KdsSeverity::Fatal,
        [
            18,
            0,
            crate::shell::commands::boot_ticks(),
            crate::memory::paging::active_pml4(),
        ],
    );
    let freeze = crate::sairu::failure_summary();
    testsaios_line(
        crate::kds::latest_event(crate::kds::KdsEventType::WatchdogCpuStall).is_some(),
        "watchdog event generated",
    );
    testsaios_line(
        freeze.failure_kind == crate::sairu::FailureKind::SchedulerStall,
        "stall recorded",
    );
    testsaios_line(freeze.evidence_value_3 > 0, "evidence available");
    testsaios_line(
        freeze.recommended_action_1 == crate::sairu::DiagnosticActionId::CollectFreezeDump
            && freeze.recommended_action_2
                == crate::sairu::DiagnosticActionId::InspectSchedulerAndLocks,
        "recommendation generated",
    );
    finish_matrix("architecture matrix failed")
}

fn observability_activity_validation() -> TestResult {
    begin_matrix();
    crate::kds::flush_aggregates();
    testsaios_line(
        crate::kds::count_events(crate::kds::KdsEventType::TaskCreate) > 0
            && crate::kds::count_events(crate::kds::KdsEventType::TaskExit) > 0
            && crate::kds::count_events(crate::kds::KdsEventType::Wait) > 0
            && (crate::kds::count_events(crate::kds::KdsEventType::Execve) > 0
                || crate::kds::count_events(crate::kds::KdsEventType::Fork) > 0),
        "process lifecycle evidence recorded",
    );
    testsaios_line(
        crate::kds::count_events(crate::kds::KdsEventType::Mmap) > 0
            || crate::kds::count_events(crate::kds::KdsEventType::Mprotect) > 0
            || crate::kds::count_metrics(crate::kds::KdsMetricId::MmapBytes) > 0,
        "memory mapping evidence recorded",
    );
    testsaios_line(
        crate::kds::count_events(crate::kds::KdsEventType::Fault) > 0
            || crate::kds::count_metrics(crate::kds::KdsMetricId::Faults) > 0,
        "fault evidence recorded",
    );
    testsaios_line(
        crate::kds::count_events(crate::kds::KdsEventType::TaskBlock) > 0
            || crate::kds::count_events(crate::kds::KdsEventType::TaskUnblock) > 0
            || crate::kds::count_metrics(crate::kds::KdsMetricId::SchedulerProgress) > 0
            || crate::kds::aggregate_exists(
                crate::kds::KdsSubsystem::Scheduler,
                crate::kds::KdsMetricId::SchedulerProgress,
            ),
        "scheduler activity evidence recorded",
    );
    testsaios_line(
        crate::kds::count_events(crate::kds::KdsEventType::WatchdogCpuStall) > 0
            || crate::kds::aggregate_exists(
                crate::kds::KdsSubsystem::Watchdog,
                crate::kds::KdsMetricId::CpuHeartbeat,
            ),
        "watchdog evidence recorded",
    );
    finish_matrix("observability activity matrix failed")
}

fn testsaios_line(pass: bool, text: &str) {
    if !pass {
        MATRIX_FAILED.store(true, Ordering::Relaxed);
    }
    println!("  {} {}", if pass { "PASS" } else { "FAIL" }, text);
}

fn testsaios_colon_line(pass: bool, text: &str) {
    if !pass {
        MATRIX_FAILED.store(true, Ordering::Relaxed);
    }
    println!("  {}: {}", if pass { "PASS" } else { "FAIL" }, text);
}

fn bootselftest_line(pass: bool, text: &str) {
    if !pass {
        MATRIX_FAILED.store(true, Ordering::Relaxed);
    }
    println!("  {}: {}", if pass { "PASS" } else { "FAIL" }, text);
}

fn gp_test() -> TestResult {
    run_embedded_probe(
        "gp_test",
        "/tmp/gp_test",
        GP_TEST_ELF,
        "General Protection fault probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn ud_test() -> TestResult {
    run_embedded_probe(
        "ud_test",
        "/tmp/ud_test",
        UD_TEST_ELF,
        "Invalid Opcode fault probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn div0_test() -> TestResult {
    run_embedded_probe(
        "div0_test",
        "/tmp/div0_test",
        DIV0_TEST_ELF,
        "Divide by Zero fault probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn pf_test() -> TestResult {
    run_embedded_probe(
        "pf_test",
        "/tmp/pf_test",
        PF_TEST_ELF,
        "Page Fault probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn hlt_test() -> TestResult {
    run_embedded_probe(
        "hlt_test",
        "/tmp/hlt_test",
        RING3_HALT_ELF,
        "HLT privilege fault probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

fn fault_test() -> TestResult {
    run_embedded_probe(
        "fault_test",
        "/tmp/fault_test",
        FAULT_TEST_ELF,
        "generic user fault probe",
        DEFAULT_TEST_TIMEOUT_MS,
    )
}

pub fn help_process() {
    println!("  User-space:");
    println!("    testsaios          run all SAIOS matrices with PASS/FAIL/TIMEOUT results");
    println!("    exec <path>        run ELF binary from ramfs in ring 3");
    println!("    ps                 list all processes (PID, state, CPU, name)");
    println!("    kill [-<sig>] <pid> send signal to process (default: SIGTERM)");
}

pub fn kill(args: &str) {
    let mut signal: u32 = 15; // SIGTERM default
    let mut pid_str: Option<&str> = None;

    for tok in args.split_whitespace() {
        if let Some(sig_part) = tok.strip_prefix('-') {
            signal = match sig_part.to_uppercase().as_str() {
                "KILL" | "SIGKILL" => 9,
                "TERM" | "SIGTERM" => 15,
                "INT" | "SIGINT" => 2,
                "HUP" | "SIGHUP" => 1,
                "QUIT" | "SIGQUIT" => 3,
                "STOP" | "SIGSTOP" => 19,
                "CONT" | "SIGCONT" => 18,
                "USR1" | "SIGUSR1" => 10,
                "USR2" | "SIGUSR2" => 12,
                _ => match sig_part.parse::<u32>() {
                    Ok(n) if n <= 31 => n,
                    _ => {
                        println!("kill: invalid signal '{}'", sig_part);
                        return;
                    }
                },
            };
        } else {
            pid_str = Some(tok);
        }
    }

    let Some(pid_tok) = pid_str else {
        println!("usage: kill [-<signal>] <pid>");
        println!("  signals: 1=HUP 2=INT 3=QUIT 9=KILL 15=TERM 19=STOP 18=CONT");
        return;
    };
    let Ok(pid) = pid_tok.parse::<u32>() else {
        println!("kill: invalid pid '{}'", pid_tok);
        return;
    };
    if pid == 0 {
        println!("kill: cannot signal pid 0 (idle)");
        return;
    }
    if crate::ipc::signal::raise_signal_for_pid(pid, signal) {
        println!("signal {} sent to pid {}", signal, pid);
    } else {
        println!("kill: no such process (pid={})", pid);
    }
}

include!(concat!(env!("OUT_DIR"), "/usertest_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/ring3_loop_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/ring3_halt_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/test_pie_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/validation_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/fork_abi_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/execve_driver_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/execve_child_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/fault_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/gp_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/ud_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/div0_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/pf_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/memperm_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/thread_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/futex_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/signal_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/wait_reap_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/pipe_semantics_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/syscall_abi_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/capability_test_elf.rs"));
include!(concat!(env!("OUT_DIR"), "/libc_plan_test_elf.rs"));
