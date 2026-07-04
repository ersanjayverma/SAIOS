//! DISKPART — SAIOS disk and volume management utility.
//!
//! Invoked as a /bin binary via `exec diskpart [command] [args]`.
//!
//! Usage:
//!   diskpart                              Full view: disks + volumes
//!   diskpart list                         List all volumes
//!   diskpart volumes                      List all volumes
//!   diskpart info   <volume>              Detailed info for one volume
//!   diskpart format <volume> <fs>         Format a volume (ext4/ntfs/fat32/…)
//!   diskpart mount  <volume> <path> [ro]  Mount a volume
//!   diskpart umount <path>                Unmount a path
//!   diskpart scan                         Rescan PCI storage devices
//!   diskpart help                         Show this help

use alloc::format;
use alloc::string::String;

use crate::console;
use crate::driver::storage::{self, FilesystemKind};
use crate::vfs;

type DiskpartResult = Result<i32, &'static str>;

// ─────────────────────────────────────────────────────────────────────────────
//  Banner helpers
// ─────────────────────────────────────────────────────────────────────────────

const W: usize = 66;

fn rule() {
    console::println!("══════════════════════════════════════════════════════════════════");
}

fn thin() {
    console::println!("──────────────────────────────────────────────────────────────────");
}

fn banner(title: &str) {
    let inner = W.saturating_sub(4);
    let pad = inner.saturating_sub(title.len());
    let left = pad / 2;
    let right = pad - left;

    let mut line = String::new();
    line.push_str("══");
    for _ in 0..left {
        line.push('═');
    }
    line.push(' ');
    line.push_str(title);
    line.push(' ');
    for _ in 0..right {
        line.push('═');
    }
    line.push_str("══");
    console::println!("{}", line);
}

// ─────────────────────────────────────────────────────────────────────────────
//  Shared display helpers
// ─────────────────────────────────────────────────────────────────────────────

fn mb(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

fn sector_label(sz: u16) -> &'static str {
    match sz {
        512 => "512B",
        4096 => "4KB",
        _ => "?",
    }
}

fn writable_label(w: bool) -> &'static str {
    if w { "RW" } else { "RO" }
}

fn mount_label(m: &Option<String>) -> &str {
    m.as_deref().unwrap_or("—")
}

fn is_physical_volume(name: &str, backing: &str) -> bool {
    storage::disks_cached().iter().any(|disk| {
        disk.hardware
            && (disk.name.eq_ignore_ascii_case(name)
                || backing.starts_with(format!("{}:", disk.name).as_str()))
    })
}

// ─────────────────────────────────────────────────────────────────────────────
//  Commands
// ─────────────────────────────────────────────────────────────────────────────

fn cmd_list() {
    let vols = storage::volumes_cached();

    banner("DISKPART — ALL VOLUMES");
    console::println!(
        "  {:<10}  {:<8}  {:>8}  {:<6}  {:<6}  {}",
        "NAME",
        "FS",
        "MB",
        "SECTOR",
        "ACCESS",
        "MOUNTED AT"
    );
    thin();

    for v in &vols {
        console::println!(
            "  {:<10}  {:<8}  {:>8}  {:<6}  {:<6}  {}",
            v.name,
            v.filesystem.as_str(),
            mb(v.total_bytes),
            sector_label(v.sector_size),
            writable_label(v.writable),
            mount_label(&v.mounted_at),
        );
    }

    if vols.is_empty() {
        console::println!("  (no volumes — run 'diskpart scan')");
    }

    thin();
    console::println!(
        "  {} volume(s)  |  {} mounted",
        vols.len(),
        vols.iter().filter(|v| v.mounted_at.is_some()).count()
    );
    rule();
}

fn cmd_info(name: &str) -> DiskpartResult {
    let v = storage::find_volume_cached(name).ok_or("diskpart: volume not found")?;

    banner("VOLUME INFO");
    console::println!("  Name        : {}", v.name);
    console::println!("  Filesystem  : {}", v.filesystem.as_str());
    console::println!(
        "  Size        : {} MB  ({} bytes)",
        mb(v.total_bytes),
        v.total_bytes
    );
    console::println!("  Sector Size : {}", sector_label(v.sector_size));
    console::println!(
        "  Access      : {}",
        if v.writable {
            "Read-Write"
        } else {
            "Read-Only"
        }
    );
    console::println!("  Backing     : {}", v.backing);
    console::println!(
        "  Type        : {}",
        if is_physical_volume(&v.name, &v.backing) {
            "Physical (PCI)"
        } else {
            "Virtual/RAM"
        }
    );

    match &v.mounted_at {
        Some(p) => console::println!("  Mounted at  : {}", p),
        None => console::println!("  Mounted at  : (not mounted)"),
    }

    rule();
    Ok(0)
}

fn cmd_format(name: &str, fs_str: &str) -> DiskpartResult {
    let fs = FilesystemKind::from_str(fs_str)
        .ok_or("diskpart format: unknown filesystem; use ext4|ntfs|fat16|fat32|fat64|fat128")?;

    storage::format_volume(name, fs)?;

    console::println!("diskpart: volume '{}' formatted as {}", name, fs.as_str());
    Ok(0)
}

fn cmd_mount(name: &str, path: &str, read_only: bool) -> DiskpartResult {
    let vol = storage::resolve_mountable_volume(name)
        .ok_or("diskpart: no mountable volume found (run 'diskpart scan', then use a partition like sata0p1)")?;

    // Auto-create mount point directory
    let _ = vfs::mkdir(path);

    vfs::mount(path, vol.filesystem.as_str(), read_only)?;
    if let Err(e) = storage::mount_volume(vol.name.as_str(), path, read_only) {
        let _ = vfs::umount(path);
        return Err(e);
    }

    console::println!(
        "diskpart: '{}' ({}) mounted at {} [{}]",
        vol.name,
        vol.filesystem.as_str(),
        path,
        if read_only { "ro" } else { "rw" }
    );
    Ok(0)
}

fn cmd_umount(path: &str) -> DiskpartResult {
    vfs::umount(path)?;
    let _ = storage::umount_volume(path);
    console::println!("diskpart: {} unmounted", path);
    Ok(0)
}

fn cmd_scan() -> DiskpartResult {
    storage::request_rescan();
    let scan = storage::scan_status();
    console::println!(
        "diskpart: scan {} epoch={} queued={} running={} failures={} — completed",
        scan.phase,
        scan.epoch,
        if scan.queued { "yes" } else { "no" },
        if scan.running { "yes" } else { "no" },
        scan.failures
    );
    Ok(0)
}

fn cmd_help() {
    banner("DISKPART HELP");
    console::println!("  diskpart                              Full volume listing");
    console::println!("  diskpart list                         All volumes");
    console::println!("  diskpart volumes                      All volumes");
    console::println!("  diskpart scan                         Request storage discovery and list volumes");
    console::println!("  diskpart disks                        Alias for volumes");
    console::println!("  diskpart hdd                          Alias for scan + volumes");
    console::println!("  diskpart info   <vol>                 Detailed info for a volume");
    console::println!(
        "  diskpart format <vol> <fs>            Format: ext4 ntfs fat16 fat32 fat64 fat128"
    );
    console::println!("  diskpart mount  <vol> <path> [ro]     Mount a volume");
    console::println!("  diskpart umount <path>                Unmount a path");
    console::println!("  diskpart help                         Show this help");
    rule();
}

// ─────────────────────────────────────────────────────────────────────────────
//  Entry point
// ─────────────────────────────────────────────────────────────────────────────

pub fn run(args: &[&str], _env: &[(String, String)]) -> DiskpartResult {
    match args.first().copied() {
        None | Some("list") | Some("volumes") | Some("volume") => {
            cmd_list();
            Ok(0)
        }

        Some("disks") | Some("disk") => {
            console::println!("diskpart: disk commands are merged into volumes");
            cmd_list();
            Ok(0)
        }

        Some("info") => {
            let name = args
                .get(1)
                .copied()
                .ok_or("diskpart info: missing volume name")?;
            cmd_info(name)
        }

        Some("format") => {
            let name = args
                .get(1)
                .copied()
                .ok_or("diskpart format: missing volume name")?;
            let fs = args
                .get(2)
                .copied()
                .ok_or("diskpart format: missing filesystem type")?;
            cmd_format(name, fs)
        }

        Some("mount") => {
            let name = args
                .get(1)
                .copied()
                .ok_or("diskpart mount: missing volume name")?;
            let path = args
                .get(2)
                .copied()
                .ok_or("diskpart mount: missing mount path")?;
            let ro = args.get(3).is_some_and(|f| f.eq_ignore_ascii_case("ro"));
            cmd_mount(name, path, ro)
        }

        Some("umount") | Some("unmount") => {
            let path = args
                .get(1)
                .copied()
                .ok_or("diskpart umount: missing path")?;
            cmd_umount(path)
        }

        Some("scan") => {
            cmd_scan()?;
            cmd_list();
            Ok(0)
        }

        Some("hdd") | Some("check") | Some("drives") | Some("drive") => {
            cmd_scan()?;
            cmd_list();
            Ok(0)
        }

        Some("help") | Some("-h") | Some("--help") => {
            cmd_help();
            Ok(0)
        }

        Some(other) => {
            // Try treating the first argument as a volume name — show its info
            if storage::find_volume_cached(other).is_some() {
                return cmd_info(other);
            }
            Err("diskpart: unknown command; run 'diskpart help'")
        }
    }
}
