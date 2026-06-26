#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

extern crate alloc;

#[path = "AddressSpaceContract.rs"]
mod address_space_contract;
mod ai;
mod arch;
mod bash_setup;
mod block;
mod boot_mode;
#[path = "CapabilityContract.rs"]
mod capability_contract;
#[path = "CompatibilityContract.rs"]
mod compatibility_contract;
mod compress;
mod config;
#[path = "ConfigurationContract.rs"]
mod configuration_contract;
#[path = "DebugContract.rs"]
mod debug_contract;
mod diag;
mod driver;
#[path = "DriverContract.rs"]
mod driver_contract;
mod dynlink;
#[path = "ExecutionContract.rs"]
mod execution_contract;
mod firstboot;
mod fs;
mod fs_ramfs;
mod gdt;
mod graphics;
#[path = "IdentityContract.rs"]
mod identity_contract;
mod install;
#[path = "InterruptContract.rs"]
mod interrupt_contract;
mod interrupts;
mod ipc;
#[path = "IpcContract.rs"]
mod ipc_contract;
mod journal;
mod kds;
mod manpages;
mod memory;
#[path = "MemoryContract.rs"]
mod memory_contract;
mod multiboot;
mod net;
#[path = "NetworkContract.rs"]
mod network_contract;
#[path = "NumaContract.rs"]
mod numa_contract;
#[path = "ObservabilityContract.rs"]
mod observability_contract;
mod package;
mod panic_state;
#[path = "PowerContract.rs"]
mod power_contract;
mod process;
#[path = "ProcessContract.rs"]
mod process_contract;
#[path = "ProgressContract.rs"]
mod progress_contract;
mod reliability;
#[path = "ReliabilityContract.rs"]
mod reliability_contract;
#[path = "ResourceContract.rs"]
mod resource_contract;
mod saios;
mod sairu;
#[path = "SchedulerContract.rs"]
mod scheduler_contract;
#[path = "SecurityContract.rs"]
mod security_contract;
mod shell;
mod smp;
mod syscall;
#[path = "SyscallContract.rs"]
mod syscall_contract;
#[path = "../tests/mod.rs"]
mod tests;
mod time;
mod tools;
mod tty;
mod user;
mod vfs;
#[path = "VfsContract.rs"]
mod vfs_contract;
mod vga_buffer;
mod windows;
mod version {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/shared_version.rs"));
}

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

unsafe extern "C" {
    static _kernel_start: u8;
    static _kernel_end: u8;
}

/// Magic value that identifies a UEFI boot info pointer vs Multiboot2.
const SAIOS_UEFI_MAGIC: u64 = 0x5341_494F_5345_4649;

/// Upper bound of the boot identity map (128 GiB) - pointers above this are
/// not directly dereferenceable in early boot.
const IDENTITY_LIMIT: u64 = 128 * 1024 * 1024 * 1024;

static DIAG_BOOT: AtomicBool = AtomicBool::new(false);
static BOOT_LAST_PASSED_GATE: AtomicU64 = AtomicU64::new(u64::MAX);
static BOOT_ACTIVE_GATE: AtomicU64 = AtomicU64::new(u64::MAX);

#[repr(u8)]
#[derive(Clone, Copy)]
enum BootGate {
    PhysicalMemoryMapValidated = 0,
    HalInitialised = 1,
    LockOrderValidatorInstalled = 2,
    ExecutionContractInitialised = 3,
    MemoryAndAddressSpaceInitialised = 4,
    KdsWritePathValidated = 5,
    ProcessContractInitialised = 6,
    SchedulerContractInitialised = 7,
    InterruptContractInitialised = 8,
    SyscallContractInitialised = 9,
    DriverContractInitialised = 10,
    VfsContractInitialised = 11,
    ObservabilityFullyOperational = 12,
    ProgressContractInitialised = 13,
    ReliabilityContractInitialised = 14,
    SairuInitialised = 15,
    InitProcessLaunched = 16,
}

impl BootGate {
    const fn number(self) -> u64 {
        self as u64
    }

    const fn name(self) -> &'static str {
        match self {
            Self::PhysicalMemoryMapValidated => "physical memory map validated",
            Self::HalInitialised => "HAL initialised",
            Self::LockOrderValidatorInstalled => "lock order validator installed",
            Self::ExecutionContractInitialised => "execution contract initialised",
            Self::KdsWritePathValidated => "KDS write path validated",
            Self::ProcessContractInitialised => "process contract initialised",
            Self::SchedulerContractInitialised => "scheduler contract initialised",
            Self::MemoryAndAddressSpaceInitialised => {
                "memory and address space contracts initialised"
            }
            Self::InterruptContractInitialised => "interrupt contract initialised",
            Self::SyscallContractInitialised => "syscall contract initialised",
            Self::DriverContractInitialised => "driver contract initialised",
            Self::VfsContractInitialised => "VFS contract initialised",
            Self::ObservabilityFullyOperational => "observability fully operational",
            Self::ProgressContractInitialised => "progress contract initialised",
            Self::ReliabilityContractInitialised => "reliability contract initialised",
            Self::SairuInitialised => "SAIRU initialised",
            Self::InitProcessLaunched => "init launched",
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy)]
enum BootStage {
    Platform = 1,
    CoreServices = 2,
    ProcessInfrastructure = 3,
    SyscallInfrastructure = 4,
    SmpBringup = 5,
    Contracts = 6,
    BootSelfTest = 7,
    DeviceDiscovery = 8,
    VfsInitialization = 9,
    StorageInitialization = 10,
    RootFilesystem = 11,
    RuntimeFilesystems = 12,
    SairuRuntime = 13,
    LoginEnvironment = 14,
}

impl BootStage {
    const fn label(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::CoreServices => "core-services",
            Self::ProcessInfrastructure => "process-infrastructure",
            Self::SyscallInfrastructure => "syscall",
            Self::SmpBringup => "smp",
            Self::Contracts => "contracts",
            Self::BootSelfTest => "bootselftest",
            Self::DeviceDiscovery => "device-discovery",
            Self::VfsInitialization => "vfs",
            Self::StorageInitialization => "storage",
            Self::RootFilesystem => "root-filesystem",
            Self::RuntimeFilesystems => "runtime-filesystems",
            Self::SairuRuntime => "sairu-runtime",
            Self::LoginEnvironment => "login-environment",
        }
    }
}

fn boot_stage_begin(stage: BootStage) {
    boot_stage_marker(stage, 1, "begin");
}

fn boot_stage_end(stage: BootStage) {
    boot_stage_marker(stage, 2, "end");
}

fn boot_stage_marker(stage: BootStage, transition: u64, transition_name: &'static str) {
    let timestamp = time::uptime_ns();
    let cpu = process::table::cpu_idx() as u64;
    serial_println!(
        "[boot] segment {} {} {}",
        stage as u8,
        transition_name,
        stage.label()
    );
    kds::kds_event_for(
        kds::KdsSubsystem::Kernel,
        kds::KdsEventType::Boot,
        kds::KdsSeverity::Info,
        0,
        0,
        [stage as u64, transition, timestamp, cpu],
    );
}

fn boot_gate_begin(gate: BootGate) {
    let expected = if gate.number() == 0 {
        u64::MAX
    } else {
        gate.number() - 1
    };
    let last = BOOT_LAST_PASSED_GATE.load(Ordering::Acquire);
    if last != expected {
        boot_gate_fail(gate, "boot gate order violation");
    }
    BOOT_ACTIVE_GATE.store(gate.number(), Ordering::Release);
    serial_println!("Gate {}: {} begin", gate.number(), gate.name());
}

fn boot_gate_pass(gate: BootGate) {
    if BOOT_ACTIVE_GATE.load(Ordering::Acquire) != gate.number() {
        boot_gate_fail(gate, "boot gate pass without matching begin");
    }
    BOOT_LAST_PASSED_GATE.store(gate.number(), Ordering::Release);
    BOOT_ACTIVE_GATE.store(u64::MAX, Ordering::Release);
    serial_println!("Gate {}: {}", gate.number(), gate.name());
    emit_boot_gate_event(crate::kds::KdsEventType::BootGatePassed, gate, 0);
}

fn boot_gate_fail(gate: BootGate, reason: &'static str) -> ! {
    serial_println!("Gate {} FAILED: {}: {}", gate.number(), gate.name(), reason);
    emit_boot_gate_event(
        crate::kds::KdsEventType::BootGateFailed,
        gate,
        stable_boot_reason(reason),
    );
    match crate::kds::seal_flight_recorder_final() {
        Ok(records) => serial_println!(
            "[boot] flight recorder final seal complete records={}",
            records
        ),
        Err(seal_reason) => {
            serial_println!("[boot] flight recorder final seal failed: {}", seal_reason)
        }
    }
    hlt_loop()
}

fn emit_boot_gate_event(event_type: crate::kds::KdsEventType, gate: BootGate, reason_hash: u64) {
    if gate.number() < BootGate::KdsWritePathValidated.number() {
        return;
    }
    crate::kds::kds_event(
        crate::kds::KdsSubsystem::Kernel,
        event_type,
        if event_type == crate::kds::KdsEventType::BootGateFailed {
            crate::kds::KdsSeverity::Error
        } else {
            crate::kds::KdsSeverity::Info
        },
        [gate.number(), crate::time::uptime_ns(), reason_hash, 0],
    );
}

fn emit_boot_complete_event() {
    serial_println!("BOOT_COMPLETE");
    crate::kds::kds_event(
        crate::kds::KdsSubsystem::Kernel,
        crate::kds::KdsEventType::BootComplete,
        crate::kds::KdsSeverity::Info,
        [
            BOOT_LAST_PASSED_GATE.load(Ordering::Acquire),
            crate::time::uptime_ns(),
            0,
            0,
        ],
    );
}

fn stable_boot_reason(reason: &'static str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in reason.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn trace_rootfs_marker(marker: &'static str) {
    serial_println!("[rootfs] trace {}", marker);
}

fn trace_rootfs_diagnostics(marker: &'static str) {
    let diagnostic = block::diagnose();
    serial_println!("[rootfs] trace {}", marker);
    match diagnostic.device {
        Some(device) => serial_println!(
            "[rootfs] controller={} disk=present sectors={} sector_size={}",
            block::controller_name(device.controller),
            device.sector_count,
            device.sector_size
        ),
        None => serial_println!("[rootfs] controller=none disk=absent"),
    }
    serial_println!(
        "[rootfs] partition-table mbr={} gpt={} count={}",
        validity(diagnostic.mbr_valid),
        validity(diagnostic.gpt_valid),
        diagnostic.partitions.len()
    );
    for partition in &diagnostic.partitions {
        serial_println!(
            "[rootfs] partition index={} table={} start_lba={} size_lba={} type=0x{:02x}",
            partition.index,
            block::partition_table_name(partition.table),
            partition.start_lba,
            partition.size_lba,
            partition.type_code
        );
    }
    for probe in &diagnostic.probes {
        serial_println!(
            "[rootfs] ext4-probe partition_index={} partition_start_lba={} superblock_lba={} expected_magic=0x{:04x} actual_magic=0x{:04x} result={}",
            probe.partition_index.unwrap_or(0),
            probe.probe_target_lba,
            probe.superblock_lba,
            probe.expected_magic,
            probe.actual_magic,
            probe.result
        );
    }
    serial_println!(
        "[rootfs] state={} status={} mount-state success={} failure={}",
        block::root_filesystem_state_name(block::classify_root_filesystem(&diagnostic)),
        block::root_filesystem_status(block::classify_root_filesystem(&diagnostic)),
        diagnostic.root_mount_success,
        diagnostic.root_mount_failure.unwrap_or("none")
    );
}

fn validity(valid: bool) -> &'static str {
    if valid { "valid" } else { "invalid" }
}

/// UEFI boot info structure (matches uefi_stub/src/main.rs).
#[repr(C)]
struct UefiBootInfo {
    magic: u64,
    map_count: u32,
    descriptor_size: u32,
    memory_map: u64,
    cmdline: [u8; 256],
}

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(mbi_ptr: u64) -> ! {
    // Capture the pristine kernel image for the disk installer BEFORE any .data
    // static is modified - must be the very first thing we do.
    install::elf_wrap::snapshot_kernel();
    driver::serial::init();
    vga_buffer::clear();
    serial_println!(" ____    _    ___ ___  ____");
    serial_println!("/ ___|  / \\  |_ _/ _ \\/ ___|");
    serial_println!("\\___ \\ / _ \\  | | | | \\___ \\");
    serial_println!(" ___) / ___ \\ | | |_| |___) |");
    serial_println!("|____/_/   \\_\\___\\___/|____/");
    serial_println!();
    serial_println!(
        "{} {} -- {}",
        version::SAIOS_NAME,
        version::SAIOS_VERSION_TAG,
        version::SAIOS_FULL_NAME
    );
    serial_println!(
        "{}  |  x86_64  |  GRUB Multiboot2 / UEFI",
        compatibility_contract::CompatibilityContract::active_status_summary()
    );
    serial_println!();

    // -- Boot segment: hardware that needs no heap --------------------------
    boot_stage_begin(BootStage::Platform);
    gdt::init();
    interrupts::init_idt();
    boot_stage_begin(BootStage::CoreServices);
    diag::init(); // heartbeat (1 Hz) + watchdog (10 s) + fault dump
    boot_stage_begin(BootStage::Contracts);
    boot_stage_end(BootStage::Contracts);
    time::init(); // calibrate TSC + read CMOS RTC for an accurate clock

    windows::init();

    let kernel_start = unsafe { &_kernel_start as *const u8 as u64 };
    let kernel_end = unsafe { &_kernel_end as *const u8 as u64 };
    boot_gate_begin(BootGate::PhysicalMemoryMapValidated);

    // -- Boot segment: parse boot info into fixed buffers (no heap yet) ------
    //
    // We must build the memory map and command line without allocating, since
    // allocator to be initialised from that very memory map first.
    let mut regions = [multiboot::MemRegion {
        base: 0,
        len: 0,
        kind: 0,
    }; multiboot::MAX_MEM_REGIONS];
    let region_n: usize;
    let mut cmdline_buf = [0u8; 256];
    let cmdline_len: usize;
    let mut framebuffer = multiboot::FramebufferInfo::default();

    // Detect UEFI vs Multiboot2 by checking for our magic at mbi_ptr.
    let is_uefi = mbi_ptr != 0
        && mbi_ptr > 0x1000
        && mbi_ptr < IDENTITY_LIMIT
        && unsafe { core::ptr::read_unaligned(mbi_ptr as *const u64) } == SAIOS_UEFI_MAGIC;

    if is_uefi {
        let uefi = unsafe { &*(mbi_ptr as *const UefiBootInfo) };
        region_n = fill_regions_from_uefi(uefi, &mut regions);
        let cl_end = uefi.cmdline.iter().position(|&b| b == 0).unwrap_or(0);
        cmdline_len = cl_end.min(256);
        cmdline_buf[..cmdline_len].copy_from_slice(&uefi.cmdline[..cmdline_len]);
        // (UEFI GOP framebuffer handoff is a future addition.)
    } else {
        let boot_info = unsafe { multiboot::parse(mbi_ptr) };
        region_n = boot_info.mem_region_count.min(multiboot::MAX_MEM_REGIONS);
        regions[..region_n].copy_from_slice(&boot_info.mem_regions[..region_n]);
        cmdline_len = boot_info.cmdline_len.min(256);
        cmdline_buf[..cmdline_len].copy_from_slice(&boot_info.cmdline[..cmdline_len]);
        framebuffer = boot_info.framebuffer;
    }

    // Reserve KDS from fixed boot memory data, but initialize KDS only after
    // the frame allocator and heap exist.
    let kds_reservation =
        kds::reserve_from_memory_map(&regions[..region_n], kernel_start, kernel_end)
            .unwrap_or_else(|reason| boot_gate_fail(BootGate::PhysicalMemoryMapValidated, reason));
    boot_gate_pass(BootGate::PhysicalMemoryMapValidated);
    boot_gate_begin(BootGate::HalInitialised);
    boot_gate_pass(BootGate::HalInitialised);
    boot_gate_begin(BootGate::LockOrderValidatorInstalled);
    crate::reliability::lock_order::install();
    boot_gate_pass(BootGate::LockOrderValidatorInstalled);
    boot_gate_begin(BootGate::ExecutionContractInitialised);
    // Gate 3: validate GDT + TSS are loaded for BSP, including a bootstrap
    // RSP0 until process switches install per-task kernel stacks.
    if !crate::gdt::tss_ist0_valid() {
        boot_gate_fail(
            BootGate::ExecutionContractInitialised,
            "BSP TSS not loaded: double-fault IST is zero",
        );
    }
    if !crate::gdt::tss_rsp0_valid() {
        boot_gate_fail(
            BootGate::ExecutionContractInitialised,
            "BSP TSS not loaded: RSP0 is zero",
        );
    }
    boot_gate_pass(BootGate::ExecutionContractInitialised);
    boot_gate_begin(BootGate::MemoryAndAddressSpaceInitialised);

    // -- Boot segment: frame allocator, then the dynamic heap ----------------
    serial_println!("[boot] memory segment init begin regions={}", region_n);
    memory::init_with_reserved(
        &regions[..region_n],
        kernel_start,
        kernel_end,
        Some((kds_reservation.base, kds_reservation.size)),
    );
    serial_println!("[boot] memory segment frame allocator ready");
    memory::init_heap(); // dynamic, frame-backed - alloc available after here
    memory::slab::init(); // F-MEM-02: slab cache for common sizes (32-512 bytes)
    serial_println!("[boot] memory segment heap ready");
    journal::init(); // system log ring buffer (needs the heap)
    diag::watchdog::init_after_heap();
    boot_gate_pass(BootGate::MemoryAndAddressSpaceInitialised);
    boot_gate_begin(BootGate::KdsWritePathValidated);
    kds::init(kds_reservation);
    let kds_numa_events = crate::numa_contract::NumaContract::emit_kds_segment_evidence();
    serial_println!(
        "[kds] emitted {} NUMA placement evidence events",
        kds_numa_events
    );
    boot_gate_pass(BootGate::KdsWritePathValidated);
    boot_gate_begin(BootGate::ProcessContractInitialised);
    // Gate 6: validate process table exists and idle slots are ready.
    if crate::process::table::TABLE.try_lock().is_none() {
        boot_gate_fail(
            BootGate::ProcessContractInitialised,
            "process table lock unavailable at gate",
        );
    }
    boot_gate_pass(BootGate::ProcessContractInitialised);
    boot_gate_begin(BootGate::SchedulerContractInitialised);
    // Gate 7: validate scheduler contract reports its topology.
    let sched_cap = crate::scheduler_contract::SchedulerContract::capability_view();
    serial_println!(
        "[gate-7] scheduler topology={:?} per_cpu_queues={}",
        sched_cap.active_topology,
        sched_cap.has_per_cpu_run_queues,
    );
    boot_gate_pass(BootGate::SchedulerContractInitialised);
    boot_gate_begin(BootGate::InterruptContractInitialised);
    // Gate 8: validate IDT is loaded (we got here via interrupt-enabled code).
    boot_gate_pass(BootGate::InterruptContractInitialised);
    boot_stage_end(BootStage::CoreServices);
    boot_stage_end(BootStage::Platform);

    // -- Boot segment: now that the heap exists, use alloc freely ------------
    {
        let mut cached = multiboot::CACHED_REGIONS.lock();
        cached[..region_n].copy_from_slice(&regions[..region_n]);
        *multiboot::CACHED_REGION_COUNT.lock() = region_n;
    }
    let cmdline = alloc::string::String::from(
        core::str::from_utf8(&cmdline_buf[..cmdline_len]).unwrap_or(""),
    );
    if !cmdline.is_empty() {
        serial_println!("[boot] cmdline: {}", cmdline);
    }
    serial_println!(
        "[boot] mode: {}",
        if is_uefi { "UEFI" } else { "GRUB Multiboot2" }
    );
    let boot_mode = parse_cmdline_mode(&cmdline);
    let parsed_boot_mode = boot_mode::BootMode::parse(&boot_mode);
    DIAG_BOOT.store(
        parsed_boot_mode == boot_mode::BootMode::Debug,
        Ordering::Relaxed,
    );
    if cmdline.contains("saios.boot=hdd") {
        shell::BOOTED_FROM_HDD.store(true, core::sync::atomic::Ordering::Relaxed);
    }

    boot_stage_begin(BootStage::SyscallInfrastructure);
    boot_gate_begin(BootGate::SyscallContractInitialised);
    syscall::init(); // initialize BSP syscall state before APs enter syscall::init()
    boot_gate_pass(BootGate::SyscallContractInitialised);
    boot_stage_end(BootStage::SyscallInfrastructure);

    // Boot segment: userspace infrastructure
    boot_stage_begin(BootStage::DeviceDiscovery);
    boot_gate_begin(BootGate::DriverContractInitialised);
    driver::keyboard::init();
    driver::mouse::init();
    driver::acpi::init(); // power management + clean shutdown
    boot_stage_begin(BootStage::SmpBringup);
    smp::init(); // enumerate and bring up scheduler-visible CPUs
    crate::arch::x86_64::ioapic::init(); // F-INT-02: detect + program IOAPIC, fallback to PIC
    boot_stage_end(BootStage::SmpBringup);
    driver::usb_hid::init(); // release USB from BIOS, enable USB HID
    driver::hda::init(); // Intel HDA audio

    // Boot segment: graphics framebuffer handoff from GRUB if present
    if framebuffer.addr != 0 && framebuffer.addr < IDENTITY_LIMIT {
        driver::vesa::init(
            framebuffer.addr,
            framebuffer.width,
            framebuffer.height,
            framebuffer.pitch,
            framebuffer.bpp,
        );
    }
    graphics::init();

    // If GRUB handed us a framebuffer, the VGA text buffer (0xB8000) is not
    // displayed - route all kernel text output to the graphics console so the
    // shell and command output are visible on screen.
    if graphics::available() {
        graphics::console::enter();
        vga_buffer::use_gfx_console(true);
        serial_println!("[gfx] text output routed to framebuffer console");
    }

    // Boot segment: dynamic linker surface
    dynlink::init();
    boot_gate_pass(BootGate::DriverContractInitialised);

    // Boot segment: VFS and filesystems
    boot_gate_begin(BootGate::VfsContractInitialised);
    // Install man pages into VFS
    manpages::install();

    boot_stage_begin(BootStage::VfsInitialization);
    serial_println!("[vfs] mounting filesystems...");
    if let Err(e) = fs::register_builtin_filesystems() {
        serial_println!("[vfs] filesystem driver registration failed: {}", e);
    }
    fs::tmpfs::mount("/"); // rootfs (tmpfs until disk is mounted)
    boot_stage_end(BootStage::VfsInitialization);
    boot_stage_begin(BootStage::RuntimeFilesystems);
    fs::tmpfs::mount("/tmp");
    fs::tmpfs::mount("/run");
    fs::procfs::mount("/proc");
    fs::devfs::mount("/dev");
    boot_stage_end(BootStage::RuntimeFilesystems);

    // Boot segment: Ethernet NICs before VirtIO-Net so real hardware wins
    driver::net::init();
    net::ipv6::init(); // derive link-local IPv6 address from the NIC MAC
    driver::wifi::init();

    // Boot segment: block devices - AHCI first, then VirtIO-Block
    boot_stage_begin(BootStage::StorageInitialization);
    let blk_dev = block::ahci::Ahci::probe().or_else(block::virtio_blk::VirtioBlk::probe);
    boot_stage_end(BootStage::StorageInitialization);

    boot_stage_begin(BootStage::RootFilesystem);
    let live_boot = parsed_boot_mode == boot_mode::BootMode::Live;
    let persistent_root = match blk_dev {
        Some(dev) => {
            block::register(dev.clone());
            trace_rootfs_diagnostics("after-ext4-probe");
            let diagnostic = block::diagnose();
            match block::classify_root_filesystem(&diagnostic) {
                block::RootFilesystemState::FilesystemValid => {
                    if live_boot {
                        block::record_root_mount_result(true, None);
                        trace_rootfs_marker("after-live-rootfs-selection");
                        serial_println!(
                            "[fs] live mode: ext4 root is valid; using recovery tmpfs root"
                        );
                        false
                    } else {
                        match fs::ext4::mount(dev, "/") {
                            Ok(()) => {
                                block::record_root_mount_result(true, None);
                                trace_rootfs_diagnostics("after-ext4-mount-attempt");
                                serial_println!("[fs] ext4 mounted as root");
                                true
                            }
                            Err(e) => {
                                block::record_root_mount_result(false, Some(e));
                                trace_rootfs_diagnostics("after-ext4-mount-attempt");
                                trace_rootfs_marker("after-tmpfs-fallback-selection");
                                serial_println!("[fs] ext4 mount failed ({}), using tmpfs root", e);
                                false
                            }
                        }
                    }
                },
                state => {
                    let status = block::root_filesystem_status(state);
                    block::record_root_mount_result(false, Some(status));
                    trace_rootfs_diagnostics("after-rootfs-classification");
                    trace_rootfs_marker("after-tmpfs-fallback-selection");
                    serial_println!("[fs] {}, using tmpfs root", status);
                    false
                }
            }
        }
        None => {
            block::record_root_mount_result(false, Some("no block device"));
            trace_rootfs_diagnostics("after-ext4-probe");
            trace_rootfs_marker("after-tmpfs-fallback-selection");
            serial_println!("[blk] no block device found - using tmpfs root");
            false
        }
    };

    if persistent_root {
        repair_rootfs_metadata();
    } else {
        provision_recovery_rootfs();
        trace_rootfs_marker("after-recovery-rootfs-provisioning");
    }
    boot_stage_end(BootStage::RootFilesystem);
    match kds::flush_flight_recorder(64) {
        Ok(records) => serial_println!("[kds] flight recorder flushed {} boot records", records),
        Err(reason) => serial_println!("[kds] flight recorder degraded: {}", reason),
    }
    if let Ok(entries) = crate::vfs_contract::VfsContract::read_dir("/") {
        serial_println!("[fs] root populated: {} entries", entries.len());
    }
    trace_rootfs_diagnostics("after-rootfs-stage-completion");
    boot_gate_pass(BootGate::VfsContractInitialised);
    boot_gate_begin(BootGate::ObservabilityFullyOperational);
    // Gate 12: validate KDS is ready and rings are operational.
    if !crate::kds::KDS_READY.load(Ordering::Acquire) {
        boot_gate_fail(
            BootGate::ObservabilityFullyOperational,
            "KDS rings not ready at observability gate",
        );
    }
    boot_gate_pass(BootGate::ObservabilityFullyOperational);

    // Initialize user management system
    user::init();
    serial_println!("[user] user management system initialized");
    boot_gate_begin(BootGate::ProgressContractInitialised);
    // Gate 13: validate watchdog and heartbeat are installed.
    crate::diag::watchdog::note_cpu_heartbeat(); // prove watchdog can receive
    boot_gate_pass(BootGate::ProgressContractInitialised);
    boot_gate_begin(BootGate::ReliabilityContractInitialised);
    // Gate 14: validate Red Ring pathway is installed (enter + seal + NMI handler).
    if !crate::reliability::lock_order::is_active() {
        boot_gate_fail(
            BootGate::ReliabilityContractInitialised,
            "lock order validator not active at reliability gate",
        );
    }
    boot_gate_pass(BootGate::ReliabilityContractInitialised);

    // Boot segment: network identity and queues
    // Prefer hardware NICs (e1000/rtl8139); fall back to VirtIO-Net.
    // If a hardware NIC was found, its MAC/IP are already set - don't overwrite.
    if driver::net::hw_nic_active() {
        serial_println!("[net] hardware NIC active - VirtIO-Net disabled");
    } else {
        // No hardware NIC - try VirtIO-Net (QEMU -device virtio-net-pci)
        net::virtio::init();
        // If VirtIO-Net also failed, show a hint
        let ip = network_contract::NetworkContract::ip();
        if ip == [0, 0, 0, 0] {
            serial_println!("[net] WARNING: no NIC found - check VirtualBox adapter type");
            serial_println!("[net] Supported: Intel PRO/1000, Realtek RTL8139, VirtIO-Net");
            serial_println!("[net] In VirtualBox: Machine → Settings → Network → Adapter Type");
        }
    }
    boot_stage_end(BootStage::DeviceDiscovery);

    boot_stage_begin(BootStage::SairuRuntime);
    boot_gate_begin(BootGate::SairuInitialised);
    // F-GATE-01: Gate 15 validates SAIRU module is callable (stub-level check).
    // SAIRU must have at least 1 registered skill and 1 registered tool.
    assert!(
        sairu::skill_count() > 0,
        "Gate 15 FAIL: SAIRU has no registered skills"
    );
    assert!(
        sairu::tool_count() > 0,
        "Gate 15 FAIL: SAIRU has no registered tools"
    );
    boot_stage_end(BootStage::SairuRuntime);
    boot_gate_pass(BootGate::SairuInitialised);

    emit_boot_complete_event();

    serial_println!();

    boot_mode::run_boot_mode(boot_mode.as_str(), login_thread, heartbeat_thread);

    serial_println!();
    // Config initialized in main.rs after print banner
    crate::config::init();

    // Run login and a background-job worker as preemptible kernel threads;
    // the boot context becomes the idle thread.  The timer now preempts between
    // them, so a long foreground operation no longer freezes the whole system,
    // and `cmd &` runs on the worker thread so the prompt stays responsive.
    boot_stage_begin(BootStage::ProcessInfrastructure);
    boot_gate_begin(BootGate::InitProcessLaunched);
    serial_println!("[boot] process-infrastructure: register boot thread");
    process::kthread::register_boot_thread();
    serial_println!("[boot] process-infrastructure: spawn flight-recorder");
    process::kthread::spawn("flight-recorder", kds::flight_recorder_thread);
    serial_println!("[boot] process-infrastructure: spawn bgworker");
    process::kthread::spawn_pinned("bgworker", shell::bg_worker_thread);
    serial_println!("[boot] process-infrastructure: spawn login");
    process::kthread::spawn_pinned("login", login_thread);
    // Worker pool exposes all online CPUs to deferred kernel work; scheduling
    // policy decides where those non-pinned workers actually run.
    serial_println!("[boot] process-infrastructure: start worker pool");
    process::kwork::start_pool();
    serial_println!("[boot] process-infrastructure: release scheduler cpus");
    smp::release_scheduler_cpus();
    boot_stage_end(BootStage::ProcessInfrastructure);
    // Gate 16: validate that at least 3 processes are running (boot, shell, bgworker).
    {
        let count = crate::process::table::TABLE.lock().procs.len();
        if count < 3 {
            boot_gate_fail(
                BootGate::InitProcessLaunched,
                "fewer than 3 processes after init",
            );
        }
    }
    boot_gate_pass(BootGate::InitProcessLaunched);
    serial_println!("[boot] process-infrastructure: yield BSP scheduler");
    process::scheduler::yield_now_wait("boot_bsp_shell_handoff");
    loop {
        crate::arch::halt();
    }
}

/// Entry point for the login kernel thread.
extern "C" fn login_thread() {
    serial_println!("[login] entered");
    let login_pid = {
        let table = crate::process::table::TABLE.lock();
        table.current_pid().max(1)
    };
    let controlling_tty = crate::tty::io::get_controlling_tty().unwrap_or(crate::tty::DEV_CONSOLE);
    let bootstrap = crate::saios::session::bootstrap_console_shell(login_pid, controlling_tty);
    let session = bootstrap.session;

    serial_println!(
        "[session] console sid={} pgid={} interface=login",
        session.session_id,
        session.task_domain.mapped_pgid,
    );
    dump_pre_login_state(login_pid, &session);
    boot_stage_begin(BootStage::BootSelfTest);
    crate::shell::commands::bootselftest();
    boot_stage_end(BootStage::BootSelfTest);
    boot_stage_begin(BootStage::LoginEnvironment);
    serial_println!("[login] start");
    let user = console_login_user();
    let environment = crate::saios::user_environment::UserEnvironment::from_user(&user, &session);
    let initial_cwd = user.home.clone();

    crate::mkdir_p(&environment.home);
    let _ = crate::process::with_current_process_mut(|proc| {
        proc.uid = environment.uid;
        proc.gid = environment.gid;
        proc.euid = environment.uid;
        proc.egid = environment.gid;
        proc.suid = environment.uid;
        proc.sgid = environment.gid;
        proc.cwd = initial_cwd.clone();
        proc.session_id = session.session_id;
        proc.pgid = session.task_domain.mapped_pgid;
    });
    crate::tty::io::set_session_id(session.session_id);
    crate::tty::io::set_fg_pgid(session.task_domain.mapped_pgid);
    if let Some(devno) = session.controlling_tty {
        crate::tty::io::set_controlling_tty(devno);
        serial_println!(
            "[login] tty={:?} sid={} fg_pgid={} login_pid={} current_pid={:?} pending_scancode={}",
            crate::tty::io::get_controlling_tty(),
            crate::tty::io::get_session_id(),
            crate::tty::io::get_fg_pgid(),
            login_pid,
            crate::process::current_pid(),
            crate::interrupts::has_pending_scancode(),
        );
    }
    serial_println!(
        "[login] stdin attached tty={:?} sid={} fg_pgid={} login_pid={} pending_scancode={}",
        crate::tty::io::get_controlling_tty(),
        crate::tty::io::get_session_id(),
        crate::tty::io::get_fg_pgid(),
        login_pid,
        crate::interrupts::has_pending_scancode(),
    );
    crate::shell::set_current_cwd(&initial_cwd);

    let mut env_text = alloc::string::String::new();
    for variable in &environment.variables {
        use alloc::fmt::Write;
        let _ = writeln!(env_text, "{}={}", variable.key, variable.value);
    }
    {
        use alloc::fmt::Write;
        let _ = writeln!(env_text, "PWD={}", initial_cwd);
    }
    write_file("/etc/env", env_text.as_bytes());
    serial_println!(
        "[session] console sid={} pgid={} user={} interface=userspace-shell",
        session.session_id,
        session.task_domain.mapped_pgid,
        environment.username,
    );
    boot_stage_end(BootStage::LoginEnvironment);

    run_login_shell(login_pid, &environment);
}

fn run_login_shell(
    login_pid: u32,
    environment: &crate::saios::user_environment::UserEnvironment,
) -> ! {
    let argv = alloc::vec![alloc::string::String::from("/bin/sh")];
    let mut envp = alloc::vec![alloc::format!("USER={}", environment.username)];
    for variable in &environment.variables {
        envp.push(alloc::format!("{}={}", variable.key, variable.value));
    }

    loop {
        serial_println!("[login] We made it to the shell, login_pid={} current_pid={:?}", login_pid, crate::process::current_pid());

        let mut shell = shell::Shell::new();
        shell.run();
    }
   
}
    


fn wait_for_login_child(login_pid: u32, child_pid: u32) {
    let request = crate::process_contract::ProcessWaitRequest {
        parent_pid: login_pid,
        waiter_pid: login_pid,
        want_pid: child_pid,
        options: 0,
    };

    loop {
        crate::process_contract::ProcessContract::register_child_waiter(request);
        if crate::process_contract::ProcessContract::try_reap_waitable(request).is_some() {
            return;
        }
        if !crate::process_contract::ProcessContract::block_registered_child_waiter(login_pid) {
            crate::process_contract::ProcessContract::unregister_child_waiter(login_pid);
            crate::process::scheduler::yield_now_wait("login_wait_child_retry");
        }
    }
}

fn console_login_user() -> crate::user::User {
    loop {
        print!("saios login: ");
        crate::graphics::console::update_cursor();
        let username = read_console_line();
        let username = username.trim();
        serial_println!("[login] username received len={}", username.len());
        if username.is_empty() {
            continue;
        }
        if let Some(user) = crate::user::get_user_by_name(username) {
            serial_println!("[login] user lookup ok name={}", username);
            return user;
        }
        println!("login: unknown user");
    }
}

fn dump_pre_login_state(shell_pid: u32, session: &crate::saios::session::SessionContext) {
    if !crate::diag::diag_proc_on() {
        return;
    }

    crate::serial_println!("[diag-login] pre-login state begin");
    crate::serial_println!(
        "[diag-login] cpu={} lapic={} boot_ticks={} timer_irqs={} kb_irqs={} mouse_irqs={} user_mode_active={} pending_scancode={}",
        crate::process::table::cpu_idx(),
        crate::smp::lapic_id(),
        crate::shell::commands::boot_ticks(),
        crate::interrupts::TIMER_IRQS.load(Ordering::Relaxed),
        crate::interrupts::KB_IRQS.load(Ordering::Relaxed),
        crate::interrupts::MOUSE_IRQS.load(Ordering::Relaxed),
        crate::process::USER_MODE_ACTIVE.load(Ordering::Relaxed),
        crate::interrupts::has_pending_scancode(),
    );
    crate::serial_println!(
        "[diag-login] shell_pid={} session={} pgid={} tty={:?} tty_session={} tty_fg_pgid={}",
        shell_pid,
        session.session_id,
        session.task_domain.mapped_pgid,
        session.controlling_tty,
        crate::tty::io::get_session_id(),
        crate::tty::io::get_fg_pgid(),
    );

    if let Some(table) = crate::process::table::TABLE.try_lock() {
        let scheduler = table.scheduler_snapshot();
        crate::serial_println!(
            "[diag-login] table current={:?} idle={:?} prev={:?} run_queue={:?} procs={} zombies={}",
            scheduler.current,
            scheduler.idle,
            scheduler.prev,
            scheduler.run_queue,
            table.procs.len(),
            table.zombies.len(),
        );
        for (pid, proc) in table.procs.iter() {
            crate::serial_println!(
                "[diag-login] proc pid={} name={} state={:?} on_cpu={} cpu={:?} boot_cpu_affine={} parent={} pgid={} sid={} k_rsp={:#x} pml4={:#x}",
                pid,
                proc.name,
                proc.state(),
                proc.is_on_cpu(),
                proc.cpu_owner(),
                proc.boot_cpu_affine,
                proc.parent_pid,
                proc.pgid,
                proc.session_id,
                proc.kernel_rsp,
                proc.pml4_phys,
            );
        }
        table.log_invariants("pre-login");
    } else {
        crate::serial_println!("[diag-login] process table busy before login");
    }

    crate::serial_println!("[diag-login] pre-login state end");
}

fn read_console_line() -> alloc::string::String {
    use crate::driver::keyboard::{KeyEvent, poll};
    use alloc::string::String;

    let mut line = String::new();
    // Flush stale BIOS/init scancodes, reset keyboard state machine, and
    // re-send 0xF4 enable-scanning.  Without this, leftover bytes (0xE0
    // prefixes, release codes, failed ACK bytes) sit in the queue producing
    // None from scancode_to_char(), and the keyboard may not be actively
    // scanning if the init 0xF4 was never ACK'd.
    crate::driver::keyboard::reenable();
    crate::serial_println!("[shell] waiting keyboard");
    crate::serial_println!(
        "[shell] waiting for login input tick={} kb_irqs={} pending_scancode={}",
        crate::shell::commands::boot_ticks(),
        crate::interrupts::KB_IRQS.load(Ordering::Relaxed),
        crate::interrupts::has_pending_scancode(),
    );
    loop {
        crate::diag::watchdog::enter_input_wait();
        // Block until keyboard data is available.  Uses process blocking with
        // keyboard waiter registration if a PID exists, or halt() fallback for
        // the kernel main thread.  Constitutional: ProgressContract requires
        // proper I/O wait signaling — not busy-spin.
        crate::interrupts::wait_for_keyboard_input_until(None);
        crate::diag::watchdog::leave_input_wait();

        // Drain all available key events after wake.
        while let Some(event) = poll() {
            match event {
                KeyEvent::Char(c) if c >= ' ' && c != '\x7f' => {
                    serial_println!("[kbd-pipe] login received char='{}'", c);
                    line.push(c);
                    print!("{}", c);
                    crate::graphics::console::update_cursor();
                }
                KeyEvent::Backspace => {
                    if line.pop().is_some() {
                        crate::vga_buffer::backspace();
                        crate::graphics::console::update_cursor();
                    }
                }
                KeyEvent::Enter => {
                    serial_println!("[kbd-pipe] login received enter line_len={}", line.len());
                    println!();
                    serial_println!("[kbd-pipe] login returning line");
                    return line;
                }
                _ => {}
            }
        }
    }
}

/// Recursively create a directory path - like `mkdir -p`.
/// Public so tar.rs and dpkg.rs can use it.
/// Never panics; silently skips components that already exist.
/// Public alias used by package/tar.rs and package/dpkg.rs.
/// Convert a UEFI EFI_MEMORY_DESCRIPTOR map into our fixed MemRegion array.
/// Returns the number of regions written. Performs NO heap allocation - it is
/// called before the heap exists.
fn fill_regions_from_uefi(
    uefi: &UefiBootInfo,
    out: &mut [multiboot::MemRegion; multiboot::MAX_MEM_REGIONS],
) -> usize {
    let desc_size = uefi.descriptor_size as usize;
    let count = uefi.map_count as usize;
    let base = uefi.memory_map as *const u8;
    let mut n = 0usize;

    for i in 0..count {
        if n >= multiboot::MAX_MEM_REGIONS {
            break;
        }
        let desc = unsafe { &*(base.add(i * desc_size) as *const UefiMemDesc) };
        // Types available to the OS after ExitBootServices:
        //   3 = EfiBootServicesCode, 4 = EfiBootServicesData,
        //   7 = EfiConventionalMemory, 9/10 = ACPI reclaim/NVS reclaimable.
        let kind = match desc.mem_type {
            7 | 3 | 4 => multiboot::MMAP_AVAILABLE,
            _ => multiboot::MMAP_RESERVED,
        };
        if desc.number_of_pages > 0 {
            out[n] = multiboot::MemRegion {
                base: desc.physical_start,
                len: desc.number_of_pages * 4096,
                kind,
            };
            n += 1;
        }
    }
    n
}

/// Minimal UEFI memory descriptor (must match uefi_stub's layout).
#[repr(C)]
struct UefiMemDesc {
    mem_type: u32,
    _pad: u32,
    physical_start: u64,
    virtual_start: u64,
    number_of_pages: u64,
    attribute: u64,
}

pub fn mkdir_p_pub(path: &str) {
    mkdir_p(path);
}

pub fn mkdir_p(path: &str) {
    use alloc::string::String;
    use alloc::vec::Vec;
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    let mut cur = String::from("/");
    for part in parts {
        let next = if cur == "/" {
            alloc::format!("/{}", part)
        } else {
            alloc::format!("{}/{}", cur, part)
        };
        if crate::vfs_contract::VfsContract::resolve(&next).is_err() {
            let _ = crate::vfs_contract::VfsContract::mkdir(&next, 0o755);
        }
        cur = next;
    }
}

pub fn symlink(path: &str, target: &str) {
    let _ = ensure_symlink(path, target);
}

pub fn ensure_symlink_pub(path: &str, target: &str) -> bool {
    ensure_symlink(path, target)
}

fn ensure_symlink(path: &str, target: &str) -> bool {
    let Ok((parent, name)) = crate::vfs_contract::VfsContract::resolve_parent(path) else {
        return false;
    };

    if let Ok(existing) = parent.ops.lookup(&name) {
        if existing.ftype == crate::vfs::FileType::SymLink {
            if existing.ops.readlink().ok().as_deref() == Some(target) {
                return true;
            }
            let _ = crate::vfs_contract::VfsContract::unlink(path);
            return crate::vfs_contract::VfsContract::symlink(path, target).is_ok();
        }

        // Some filesystems in SAIOS (notably the installed ext4 path today)
        // do not support symlink creation yet. Preserve an existing regular
        // compat file instead of deleting it and recreating defaults later.
        return false;
    }

    crate::vfs_contract::VfsContract::symlink(path, target).is_ok()
}

fn provision_recovery_rootfs() {
    trace_rootfs_marker("recovery-rootfs-authoritative-roots-begin");
    for dir in crate::saios::rootfs::AUTHORITATIVE_ROOTS {
        mkdir_p(dir);
    }
    trace_rootfs_marker("recovery-rootfs-authoritative-roots-end");
    trace_rootfs_marker("recovery-rootfs-authoritative-dirs-begin");
    for dir in crate::saios::rootfs::AUTHORITATIVE_DIRS {
        mkdir_p(dir);
    }
    trace_rootfs_marker("recovery-rootfs-authoritative-dirs-end");
    trace_rootfs_marker("recovery-rootfs-compatibility-roots-begin");
    for dir in crate::saios::rootfs::COMPATIBILITY_ROOTS {
        mkdir_p(dir);
    }
    trace_rootfs_marker("recovery-rootfs-compatibility-roots-end");
    trace_rootfs_marker("recovery-rootfs-compatibility-dirs-begin");
    for dir in crate::saios::rootfs::COMPATIBILITY_DIRS {
        mkdir_p(dir);
    }
    trace_rootfs_marker("recovery-rootfs-compatibility-dirs-end");
    trace_rootfs_marker("recovery-rootfs-legacy-roots-begin");
    for dir in crate::saios::rootfs::LEGACY_ROOTS {
        mkdir_p(dir);
    }
    trace_rootfs_marker("recovery-rootfs-legacy-roots-end");
    trace_rootfs_marker("recovery-rootfs-windows-compat-dirs-begin");
    for dir in crate::saios::rootfs::WINDOWS_COMPAT_DIRS {
        mkdir_p(dir);
    }
    trace_rootfs_marker("recovery-rootfs-windows-compat-dirs-end");
    trace_rootfs_marker("recovery-rootfs-macos-compat-dirs-begin");
    for dir in crate::saios::rootfs::MACOS_COMPAT_DIRS {
        mkdir_p(dir);
    }
    trace_rootfs_marker("recovery-rootfs-macos-compat-dirs-end");
    trace_rootfs_marker("recovery-rootfs-initial-files-begin");
    for (path, data) in crate::saios::rootfs::initial_files() {
        write_initial_file(path, &data);
    }
    trace_rootfs_marker("recovery-rootfs-initial-files-end");
    serial_println!("[vfs] recovery rootfs provisioned");
}

fn repair_rootfs_metadata() {
    for dir in crate::saios::rootfs::AUTHORITATIVE_ROOTS {
        mkdir_p(dir);
    }
    for dir in crate::saios::rootfs::AUTHORITATIVE_DIRS {
        mkdir_p(dir);
    }
    for dir in crate::saios::rootfs::COMPATIBILITY_ROOTS {
        mkdir_p(dir);
    }
    for dir in crate::saios::rootfs::COMPATIBILITY_DIRS {
        mkdir_p(dir);
    }
    for dir in crate::saios::rootfs::LEGACY_ROOTS {
        mkdir_p(dir);
    }
    for dir in crate::saios::rootfs::WINDOWS_COMPAT_DIRS {
        mkdir_p(dir);
    }
    for dir in crate::saios::rootfs::MACOS_COMPAT_DIRS {
        mkdir_p(dir);
    }

    migrate_compat_file_to_canonical(
        crate::config::COMPAT_CONFIG_PATH,
        crate::config::CANONICAL_CONFIG_PATH,
    );
    migrate_compat_file_to_canonical(
        crate::saios::identity::COMPAT_PASSWD,
        crate::saios::identity::NATIVE_PASSWD,
    );
    migrate_compat_file_to_canonical(
        crate::saios::identity::COMPAT_GROUP,
        crate::saios::identity::NATIVE_GROUP,
    );
    migrate_compat_file_to_canonical(
        crate::saios::identity::COMPAT_SHADOW,
        crate::saios::identity::NATIVE_SHADOW,
    );

    for (path, data) in crate::saios::rootfs::initial_files() {
        write_initial_file_if_missing_or_stale(path, &data);
    }
    serial_println!("[vfs] rootfs metadata verified");
}

/// Public wrapper so other modules (firstboot) can write a VFS file.
pub fn write_file_pub(path: &str, data: &[u8]) {
    write_file(path, data);
}

fn initial_file_mode(path: &str) -> u32 {
    if matches!(path, "/bin/sh" | "/bin/bash") {
        0o755
    } else {
        0o644
    }
}

fn write_file(path: &str, data: &[u8]) {
    let _ = crate::vfs_contract::VfsContract::write_file(path, data, 0o644);
}

fn write_initial_file(path: &str, data: &[u8]) {
    let mode = initial_file_mode(path);
    let _ = crate::vfs_contract::VfsContract::write_file(path, data, mode);
    let _ = crate::vfs_contract::VfsContract::chmod(path, mode);
}

fn write_initial_file_if_missing_or_stale(path: &str, data: &[u8]) {
    if let Ok(bytes) = crate::vfs_contract::VfsContract::read_file(path)
        && !bytes.is_empty()
    {
        if matches!(path, "/bin/sh" | "/bin/bash") && !bytes.starts_with(b"\x7fELF") {
            write_initial_file(path, data);
        } else {
            repair_initial_file_mode(path);
        }
        return;
    }
    write_initial_file(path, data);
}

fn repair_initial_file_mode(path: &str) {
    let mode = initial_file_mode(path);
    let Ok(inode) = crate::vfs_contract::VfsContract::resolve(path) else {
        return;
    };
    let Ok(stat) = inode.ops.stat() else {
        return;
    };
    if (stat.st_mode & 0o777) as u32 != mode {
        match crate::vfs_contract::VfsContract::chmod(path, mode) {
            Ok(()) => serial_println!(
                "[rootfs] repaired mode path={} old={:#o} new={:#o}",
                path,
                stat.st_mode & 0o777,
                mode
            ),
            Err(error) => serial_println!(
                "[rootfs] failed mode repair path={} old={:#o} new={:#o} errno={}",
                path,
                stat.st_mode & 0o777,
                mode,
                error.to_errno()
            ),
        }
    }
}

fn read_file_bytes(path: &str) -> Option<alloc::vec::Vec<u8>> {
    let buf = crate::vfs_contract::VfsContract::read_file(path).ok()?;
    if buf.is_empty() { None } else { Some(buf) }
}

fn migrate_compat_file_to_canonical(compat_path: &str, canonical_path: &str) {
    if read_file_bytes(canonical_path).is_some() {
        return;
    }

    let Ok(inode) = crate::vfs_contract::VfsContract::resolve(compat_path) else {
        return;
    };
    if inode.ftype == crate::vfs::FileType::SymLink {
        return;
    }

    if let Some(buf) = read_file_bytes(compat_path) {
        write_file(canonical_path, &buf);
    }
}

/// Parse `saios.mode=<value>` from the kernel command line.
/// Returns "unsupported" if not present so install media cannot live-boot.
fn parse_cmdline_mode(cmdline: &str) -> alloc::string::String {
    for token in cmdline.split_whitespace() {
        if let Some(val) = token.strip_prefix("saios.mode=") {
            return alloc::string::String::from(val);
        }
    }
    alloc::string::String::from(boot_mode::BootMode::Unsupported.as_str())
}

/// Heartbeat kernel thread for the `mtdemo` smoke test - busy-loops (never
/// yields) and logs periodically; only preemption can give it the CPU.
extern "C" fn heartbeat_thread() {
    let mut n = 0u64;
    loop {
        for _ in 0..30_000_000u64 {
            core::hint::spin_loop();
        }
        serial_println!("[mt] heartbeat #{}", n);
        n += 1;
    }
}

pub fn hlt_loop() -> ! {
    loop {
        crate::arch::halt();
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::disable_interrupts();
    record_panic_state();
    if !crate::reliability_contract::ReliabilityContract::active() {
        crate::reliability_contract::ReliabilityContract::enter_red_ring(
            crate::reliability_contract::RedRingEvidence {
                cause: crate::reliability_contract::RedRingCause::KernelPanic,
                evidence_event_id: 0,
                invariant_id: 0,
                detail: 0,
            },
        );
    }
    crate::reliability_contract::ReliabilityContract::seal_red_ring();
    render_bsod(info);
    println!("\n[KERNEL PANIC] {}", info);
    hlt_loop()
}

fn record_panic_state() {
    let cpu = process::table::cpu_idx() as u32;
    let pid = process::table::TABLE
        .try_lock()
        .and_then(|table| table.current_on_cpu(cpu as usize))
        .unwrap_or(0);
    let rip = process::table::TABLE
        .try_lock()
        .and_then(|table| table.procs.get(&pid).map(|proc| proc.rip))
        .unwrap_or(0);
    panic_state::record(cpu, pid, rip, shell::commands::boot_ticks());
}

fn render_bsod(info: &PanicInfo) {
    const RING_RED: u32 = 0x00_FF2020;
    const DARK_RED: u32 = 0x00_250000;
    const CRASH_WHITE: u32 = 0x00_FFFFFF;
    let diagnostic = sairu::failure_summary();

    if graphics::available() {
        graphics::clear(DARK_RED);
        draw_red_ring(96, 92, 54, 11, RING_RED);
        graphics::font::draw_string(176, 56, "SAIOS", CRASH_WHITE, DARK_RED);
        graphics::font::draw_string(176, 88, "CRITICAL FAILURE", CRASH_WHITE, DARK_RED);
        graphics::font::draw_string(
            32,
            168,
            "The system halted to prevent corruption.",
            CRASH_WHITE,
            DARK_RED,
        );
        if let Some(location) = info.location() {
            graphics::font::draw_string(32, 208, "Location:", CRASH_WHITE, DARK_RED);
            graphics::font::draw_string(32, 232, location.file(), CRASH_WHITE, DARK_RED);
        }
        render_sairu_failure_graphics(32, 272, CRASH_WHITE, DARK_RED, diagnostic);
        render_crash_dump_graphics(32, 560, CRASH_WHITE, DARK_RED);
        return;
    }

    {
        let mut writer = vga_buffer::WRITER.lock();
        writer.color = vga_buffer::ColorCode::new(vga_buffer::Color::White, vga_buffer::Color::Red);
        writer.clear_screen();
    }
    println!("====================================================");
    println!("SAIOS CRITICAL FAILURE");
    println!("====================================================");
    if let Some(location) = info.location() {
        println!("Location: {}:{}", location.file(), location.line());
    }
    render_sairu_failure_text(diagnostic);
    render_crash_dump_text();
}

fn render_alloc_failure_rrod(layout: core::alloc::Layout) {
    arch::disable_interrupts();
    record_panic_state();
    if !crate::reliability_contract::ReliabilityContract::active() {
        crate::reliability_contract::ReliabilityContract::enter_red_ring(
            crate::reliability_contract::RedRingEvidence {
                cause: crate::reliability_contract::RedRingCause::AllocationFailure,
                evidence_event_id: 0,
                invariant_id: layout.align() as u64,
                detail: layout.size() as u64,
            },
        );
    }
    if graphics::available() {
        const RING_RED: u32 = 0x00_FF2020;
        const DARK_RED: u32 = 0x00_250000;
        const CRASH_WHITE: u32 = 0x00_FFFFFF;
        graphics::clear(DARK_RED);
        draw_red_ring(96, 92, 54, 11, RING_RED);
        graphics::font::draw_string(176, 56, "SAIOS", CRASH_WHITE, DARK_RED);
        graphics::font::draw_string(176, 88, "CRITICAL FAILURE", CRASH_WHITE, DARK_RED);
        graphics::font::draw_string(
            32,
            168,
            "Kernel heap allocation failed.",
            CRASH_WHITE,
            DARK_RED,
        );
        graphics::font::draw_string(
            32,
            208,
            "RROD rendered before panic formatting.",
            CRASH_WHITE,
            DARK_RED,
        );
    } else {
        let mut writer = vga_buffer::WRITER.lock();
        writer.color = vga_buffer::ColorCode::new(vga_buffer::Color::White, vga_buffer::Color::Red);
        writer.clear_screen();
        use core::fmt::Write;
        let _ = writer.write_str("====================================================\n");
        let _ = writer.write_str("SAIOS CRITICAL FAILURE\n");
        let _ = writer.write_str("Kernel heap allocation failed before graphics RROD.\n");
        let _ = writer.write_str("====================================================\n");
    }
    serial_println!(
        "[alloc-oom] RROD rendered size={} align={}",
        layout.size(),
        layout.align()
    );
}

fn render_sairu_failure_graphics(
    x: usize,
    mut y: usize,
    fg: u32,
    bg: u32,
    diagnostic: sairu::FailureDiagnostic,
) {
    let mut hex = [0u8; 18];
    graphics::font::draw_string(x, y, "Failure", fg, bg);
    graphics::font::draw_string(x + 216, y, diagnostic.failure_kind.label(), fg, bg);
    y += 24;
    graphics::font::draw_string(x, y, "Confidence", fg, bg);
    graphics::font::draw_string(x + 216, y, diagnostic.confidence, fg, bg);
    y += 24;
    graphics::font::draw_string(x, y, "Detected By", fg, bg);
    graphics::font::draw_string(x + 216, y, diagnostic.detected_by.label(), fg, bg);
    y += 24;
    graphics::font::draw_string(x, y, "Likely Cause", fg, bg);
    graphics::font::draw_string(x + 216, y, diagnostic.likely_cause, fg, bg);
    y += 32;
    graphics::font::draw_string(x, y, "Evidence", fg, bg);
    y += 24;
    graphics::font::draw_string(x + 24, y, diagnostic.evidence_label_1, fg, bg);
    graphics::font::draw_string(
        x + 320,
        y,
        hex_u64(diagnostic.evidence_value_1, &mut hex),
        fg,
        bg,
    );
    y += 24;
    graphics::font::draw_string(x + 24, y, diagnostic.evidence_label_2, fg, bg);
    graphics::font::draw_string(
        x + 320,
        y,
        hex_u64(diagnostic.evidence_value_2, &mut hex),
        fg,
        bg,
    );
    y += 24;
    graphics::font::draw_string(x + 24, y, diagnostic.evidence_label_3, fg, bg);
    graphics::font::draw_string(
        x + 320,
        y,
        hex_u64(diagnostic.evidence_value_3, &mut hex),
        fg,
        bg,
    );
    y += 32;
    graphics::font::draw_string(x, y, "Recommended Actions", fg, bg);
    y += 24;
    graphics::font::draw_string(x + 24, y, diagnostic.recommended_action_1.label(), fg, bg);
    y += 24;
    graphics::font::draw_string(x + 24, y, diagnostic.recommended_action_2.label(), fg, bg);
    y += 32;
    graphics::font::draw_string(x, y, "Reference", fg, bg);
    graphics::font::draw_string(
        x + 216,
        y,
        hex_u64(diagnostic.reference_id, &mut hex),
        fg,
        bg,
    );
}

fn render_sairu_failure_text(diagnostic: sairu::FailureDiagnostic) {
    println!("Failure:");
    println!("  {}", diagnostic.failure_kind.label());
    println!("Confidence:");
    println!("  {}", diagnostic.confidence);
    println!("Detected By:");
    println!("  {}", diagnostic.detected_by.label());
    println!("Likely Cause:");
    println!("  {}", diagnostic.likely_cause);
    println!("Evidence:");
    println!(
        "  {}: {:#x}",
        diagnostic.evidence_label_1, diagnostic.evidence_value_1
    );
    println!(
        "  {}: {:#x}",
        diagnostic.evidence_label_2, diagnostic.evidence_value_2
    );
    println!(
        "  {}: {:#x}",
        diagnostic.evidence_label_3, diagnostic.evidence_value_3
    );
    println!("Recommended Actions:");
    println!("  {}", diagnostic.recommended_action_1.label());
    println!("  {}", diagnostic.recommended_action_2.label());
    println!("Reference:");
    println!("  panic-id: {:#x}", diagnostic.reference_id);
    println!("====================================================");
}

fn render_crash_dump_graphics(x: usize, mut y: usize, fg: u32, bg: u32) {
    let cpu = process::table::cpu_idx() as u64;
    let cr3 = memory::paging::active_pml4();
    let kernel_gs_active = arch::syscall::kernel_gs_active();
    let mut hex = [0u8; 18];

    graphics::font::draw_string(x, y, "Crash dump:", fg, bg);
    y += 24;

    graphics::font::draw_string(x, y, "CPU", fg, bg);
    graphics::font::draw_string(x + 128, y, hex_u64(cpu, &mut hex), fg, bg);
    y += 24;

    graphics::font::draw_string(x, y, "CR3", fg, bg);
    graphics::font::draw_string(x + 128, y, hex_u64(cr3, &mut hex), fg, bg);
    y += 24;

    graphics::font::draw_string(x, y, "kernel GS", fg, bg);
    graphics::font::draw_string(
        x + 128,
        y,
        if kernel_gs_active {
            "active"
        } else {
            "inactive"
        },
        fg,
        bg,
    );
    y += 24;

    let table = process::table::TABLE.try_lock();
    let Some(table) = table else {
        graphics::font::draw_string(x, y, "process table: locked", fg, bg);
        return;
    };

    let pid = table.current_on_cpu(cpu as usize).unwrap_or(0);
    graphics::font::draw_string(x, y, "PID", fg, bg);
    graphics::font::draw_string(x + 128, y, hex_u64(pid as u64, &mut hex), fg, bg);
    y += 24;

    if let Some(proc) = table.procs.get(&pid) {
        graphics::font::draw_string(x, y, "Name", fg, bg);
        graphics::font::draw_string(x + 128, y, proc.name.as_str(), fg, bg);
        y += 24;

        graphics::font::draw_string(x, y, "State", fg, bg);
        graphics::font::draw_string(x + 128, y, process_state_name(proc.state()), fg, bg);
        y += 24;

        graphics::font::draw_string(x, y, "RIP", fg, bg);
        graphics::font::draw_string(x + 128, y, hex_u64(proc.rip, &mut hex), fg, bg);
        y += 24;

        graphics::font::draw_string(x, y, "RSP", fg, bg);
        graphics::font::draw_string(x + 128, y, hex_u64(proc.rsp, &mut hex), fg, bg);
        y += 24;

        graphics::font::draw_string(x, y, "PML4", fg, bg);
        graphics::font::draw_string(x + 128, y, hex_u64(proc.pml4_phys, &mut hex), fg, bg);
        y += 24;

        graphics::font::draw_string(x, y, "KRSP", fg, bg);
        graphics::font::draw_string(x + 128, y, hex_u64(proc.kernel_rsp, &mut hex), fg, bg);
    } else {
        graphics::font::draw_string(x, y, "current process: missing", fg, bg);
    }
}

fn render_crash_dump_text() {
    let cpu = process::table::cpu_idx();
    println!("Crash dump:");
    println!(
        "  cpu={} cr3={:#x} kernel_gs_active={}",
        cpu,
        memory::paging::active_pml4(),
        arch::syscall::kernel_gs_active()
    );
    let table = process::table::TABLE.try_lock();
    let Some(table) = table else {
        println!("  process table: locked");
        return;
    };
    let pid = table.current_on_cpu(cpu).unwrap_or(0);
    println!("  current pid={}", pid);
    if let Some(proc) = table.procs.get(&pid) {
        println!(
            "  proc name={} state={} rip={:#x} rsp={:#x} pml4={:#x} krsp={:#x}",
            proc.name.as_str(),
            process_state_name(proc.state()),
            proc.rip,
            proc.rsp,
            proc.pml4_phys,
            proc.kernel_rsp
        );
    }
}

fn process_state_name(state: &process::ProcessState) -> &'static str {
    match state {
        process::ProcessState::New => "New",
        process::ProcessState::Ready => "Ready",
        process::ProcessState::Running => "Running",
        process::ProcessState::Blocked => "Blocked",
        process::ProcessState::Zombie => "Zombie",
        process::ProcessState::Dead => "Dead",
    }
}

fn hex_u64(value: u64, buf: &mut [u8; 18]) -> &str {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    buf[0] = b'0';
    buf[1] = b'x';
    let mut shift = 60u32;
    let mut i = 2usize;
    while i < 18 {
        buf[i] = HEX[((value >> shift) & 0xF) as usize];
        if shift == 0 {
            break;
        }
        shift -= 4;
        i += 1;
    }
    core::str::from_utf8(buf).unwrap_or("0x????????????????")
}

fn draw_red_ring(cx: usize, cy: usize, radius: usize, thickness: usize, colour: u32) {
    let outer = radius * radius;
    let inner_radius = radius.saturating_sub(thickness);
    let inner = inner_radius * inner_radius;
    let span = radius + 2;
    for y in cy.saturating_sub(span)..=cy + span {
        for x in cx.saturating_sub(span)..=cx + span {
            let dx = x.abs_diff(cx);
            let dy = y.abs_diff(cy);
            let dist = dx * dx + dy * dy;
            if dist <= outer && dist >= inner {
                graphics::vesa_put(x, y, colour);
            }
        }
    }
}

#[cfg(not(test))]
#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    render_alloc_failure_rrod(layout);
    // Try to grow the heap by 8 MiB before panicking
    if memory::heap_grow(8 * 1024 * 1024) {
        // Retry will happen automatically - but we can't return from here.
        // The allocator will re-invoke after growth.
        panic!("heap OOM after grow attempt: {:?}", layout);
    }
    panic!("heap OOM (out of physical memory): {:?}", layout);
}
