use crate::println;

pub const BOOT_MODE_INSTALL: &str = "install";
pub const BOOT_MODE_UPDATE: &str = "update";
pub const BOOT_MODE_LIVE: &str = "live";
pub const BOOT_MODE_RECOVER: &str = "recover";
pub const BOOT_MODE_STORAGE_DIAGNOSTICS: &str = "storage-diagnostics";
pub const BOOT_MODE_MEMORY_DIAGNOSTICS: &str = "memory-diagnostics";
pub const BOOT_MODE_FIRSTBOOT: &str = "firstboot";
pub const BOOT_MODE_SAFE: &str = "safe";
pub const BOOT_MODE_DEBUG: &str = "debug";
pub const BOOT_MODE_UNSUPPORTED: &str = "unsupported";
pub const BOOT_MODE_OTHER: &str = "other";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootMode {
    Install,
    Update,
    Live,
    Recover,
    StorageDiagnostics,
    MemoryDiagnostics,
    FirstBoot,
    Safe,
    Debug,
    Unsupported,
    Other,
}

impl BootMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => BOOT_MODE_INSTALL,
            Self::Update => BOOT_MODE_UPDATE,
            Self::Live => BOOT_MODE_LIVE,
            Self::Recover => BOOT_MODE_RECOVER,
            Self::StorageDiagnostics => BOOT_MODE_STORAGE_DIAGNOSTICS,
            Self::MemoryDiagnostics => BOOT_MODE_MEMORY_DIAGNOSTICS,
            Self::FirstBoot => BOOT_MODE_FIRSTBOOT,
            Self::Safe => BOOT_MODE_SAFE,
            Self::Debug => BOOT_MODE_DEBUG,
            Self::Unsupported => BOOT_MODE_UNSUPPORTED,
            Self::Other => BOOT_MODE_OTHER,
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            BOOT_MODE_INSTALL => Self::Install,
            BOOT_MODE_UPDATE => Self::Update,
            BOOT_MODE_LIVE => Self::Live,
            BOOT_MODE_RECOVER => Self::Recover,
            BOOT_MODE_STORAGE_DIAGNOSTICS => Self::StorageDiagnostics,
            BOOT_MODE_MEMORY_DIAGNOSTICS => Self::MemoryDiagnostics,
            BOOT_MODE_FIRSTBOOT => Self::FirstBoot,
            BOOT_MODE_SAFE => Self::Safe,
            BOOT_MODE_DEBUG => Self::Debug,
            BOOT_MODE_UNSUPPORTED => Self::Unsupported,
            _ => Self::Other,
        }
    }

    pub const fn supported_installed_modes() -> &'static str {
        "firstboot, safe, and debug modes"
    }
}

pub fn run_boot_mode(
    boot_mode: &str,
    _shell_thread: extern "C" fn(),
    _heartbeat_thread: extern "C" fn(),
) {
    match BootMode::parse(boot_mode) {
        BootMode::Install => {
            println!("╔══════════════════════════════════════════════════╗");
            println!("║          SAIOS Operation Mode                    ║");
            println!("║  Intent: Install SAIOS                           ║");
            println!("║  Advisor analyzes; user confirmation executes.   ║");
            println!("╚══════════════════════════════════════════════════╝");
            println!();
            if let Err(e) = crate::install::run("/dev/vda") {
                println!("Analysis note: {}", e);
            }
            println!();
            println!("Recommended next commands:");
            println!("  saios install");
            println!("  storage plan install");
            println!("  storage recommend");
            println!("  storage graph");
            println!("  storage analyze");
            println!("  sairu diagnose storage");
            println!();
            println!("SAIOS Shell. Type 'help' for commands.");
        }
        BootMode::Update => {
            println!("╔══════════════════════════════════════════════════╗");
            println!("║          SAIOS Operation Mode                    ║");
            println!("║  Intent: Update SAIOS                            ║");
            println!("║  Update is install-over-existing after consent.  ║");
            println!("╚══════════════════════════════════════════════════╝");
            println!();
            let plan = crate::saios::storage_platform::plan_update();
            println!("Analysis Complete");
            println!("  Risk: {}", plan.risk.label());
            println!("  Recommendation: backup first and run update for confirmation.");
            println!("  The final decision belongs to you.");
            println!();
            println!("Recommended next commands:");
            println!("  saios update");
            println!("  storage plan update");
            println!("  storage recommend");
            println!("  storage analyze");
            println!();
            println!("SAIOS Shell. Type 'help' for commands.");
        }
        BootMode::Live => {
            println!("╔══════════════════════════════════════════════════╗");
            println!("║          SAIOS Operation Mode                    ║");
            println!("║  Intent: Interactive SPC diagnostics and tools   ║");
            println!("╚══════════════════════════════════════════════════╝");
            println!("Recommended commands:");
            println!("  storage graph");
            println!("  storage plan install");
            println!("  storage plan update");
            println!("  storage plan recover");
            println!("  sairu diagnose storage");
            println!();
            println!("SAIOS Shell. Type 'help' for commands.");
        }
        BootMode::Recover => {
            println!("╔══════════════════════════════════════════════════╗");
            println!("║          SAIOS Operation Mode                    ║");
            println!("║  Intent: Recover Existing System                 ║");
            println!("║  Recovery uses the same runtime and advisor.     ║");
            println!("╚══════════════════════════════════════════════════╝");
            let report = crate::saios::storage_platform::recovery_report();
            println!(
                "Recovery diagnostics: disk={} partition={} filesystem={}",
                report.disk_diagnostics,
                report.partition_diagnostics,
                report.filesystem_diagnostics
            );
            println!("Recommended commands:");
            println!("  storage graph");
            println!("  storage plan recover");
            println!("  storage recovery");
            println!("  kds view");
            println!();
            println!("SAIOS Shell. Type 'help' for commands.");
        }
        BootMode::StorageDiagnostics => {
            println!("╔══════════════════════════════════════════════════╗");
            println!("║          SAIOS Operation Mode                    ║");
            println!("║  Intent: Storage Diagnostics                     ║");
            println!("╚══════════════════════════════════════════════════╝");
            let plan = crate::saios::storage_platform::execution_plan(
                crate::saios::storage_platform::StorageIntent::Diagnose,
            );
            println!(
                "SPC diagnostic plan: id={} graph={} gates={}",
                plan.plan_id,
                plan.graph.classification.label(),
                plan.gates.len()
            );
            println!(
                "Recommended commands: storage graph; storage plan diagnose; sairu diagnose storage"
            );
            println!();
            println!("SAIOS Shell. Type 'help' for commands.");
        }
        BootMode::MemoryDiagnostics => {
            println!("╔══════════════════════════════════════════════════╗");
            println!("║          SAIOS Operation Mode                    ║");
            println!("║  Intent: Memory Diagnostics                      ║");
            println!("╚══════════════════════════════════════════════════╝");
            let (total_frames, free_frames, used_frames) = crate::memory::frame_stats();
            println!(
                "Memory frames: total={} free={} used={}",
                total_frames, free_frames, used_frames
            );
            println!("Recommended commands: meminfo; kds view; sairu diagnose memory");
            println!();
            println!("SAIOS Shell. Type 'help' for commands.");
        }
        BootMode::FirstBoot => {
            crate::firstboot::run();
        }
        BootMode::Safe => {
            println!("SAIOS safe mode - network disabled.");
            println!("Type 'help' for commands.");
        }
        BootMode::Debug => {
            println!("SAIOS debug mode - verbose logging enabled.");
            if let Some(dev) = crate::block::get() {
                let lba = 1024u64;
                let mut pre = [0u8; 512];
                if dev.read_sectors(lba, &mut pre).is_ok() {
                    let persisted =
                        pre[0] == 0xA5 && pre[1] == 0xA4 && pre[2] == 0xA7 && pre[3] == 0xA6;
                    println!(
                        "[selftest] LBA{} pre-write = {:02x} {:02x} {:02x} {:02x}  -> {}",
                        lba,
                        pre[0],
                        pre[1],
                        pre[2],
                        pre[3],
                        if persisted {
                            "PERSISTED across power-cycle"
                        } else {
                            "NOT persisted (or first run)"
                        }
                    );
                }
                let mut wbuf = [0u8; 512];
                let mut i = 0;
                while i < 512 {
                    wbuf[i] = (i as u8) ^ 0xA5;
                    i += 1;
                }
                match dev.write_sectors(lba, &wbuf) {
                    Ok(()) => println!("[selftest] wrote 512B to LBA {}", lba),
                    Err(e) => println!("[selftest] WRITE FAILED: {}", e),
                }
                let mut rbuf = [0u8; 512];
                match dev.read_sectors(lba, &mut rbuf) {
                    Ok(()) => {
                        let ok = rbuf == wbuf;
                        println!(
                            "[selftest] readback {} (got {:02x} {:02x} {:02x} {:02x}, want {:02x} {:02x} {:02x} {:02x})",
                            if ok {
                                "MATCH - disk writes persist"
                            } else {
                                "MISMATCH - disk writes NOT persisting"
                            },
                            rbuf[0],
                            rbuf[1],
                            rbuf[2],
                            rbuf[3],
                            wbuf[0],
                            wbuf[1],
                            wbuf[2],
                            wbuf[3]
                        );
                    }
                    Err(e) => println!("[selftest] READ FAILED: {}", e),
                }
                let mut mbr = [0u8; 512];
                if dev.read_sectors(0, &mut mbr).is_ok() {
                    println!(
                        "[selftest] MBR LBA0: boot-code[0..4]={:02x} {:02x} {:02x} {:02x}  sig[510..512]={:02x} {:02x} {}",
                        mbr[0],
                        mbr[1],
                        mbr[2],
                        mbr[3],
                        mbr[510],
                        mbr[511],
                        if mbr[510] == 0x55 && mbr[511] == 0xAA {
                            "(VALID - install persisted)"
                        } else {
                            "(blank/invalid)"
                        }
                    );
                }
            } else {
                println!("[selftest] no block device found");
            }
            println!("Type 'help' for commands.");
        }
        _ => {
            println!("Unsupported boot mode: {}", boot_mode);
            if crate::shell::booted_from_hdd() {
                println!(
                    "Installed SAIOS supports {}.",
                    BootMode::supported_installed_modes()
                );
                println!(
                    "Check the installed GRUB entry for a stale or misspelled saios.mode value."
                );
            } else {
                println!("SAIOS install media supports only install, update, and disk boot.");
                println!("Use the GRUB disk-boot entry to start an installed system.");
            }
            loop {
                crate::arch::halt();
            }
        }
    }
}
