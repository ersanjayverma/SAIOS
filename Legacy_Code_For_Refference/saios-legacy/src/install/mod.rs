//! SAIOS disk installer - streams directly to the block device.
//!
//! Disk layout:
//!   Sector 0        : MBR (GRUB boot.img, 512 bytes)
//!   Sectors 1-2047  : GRUB core.img (up to ~1 MiB)
//!   Sector 2048+    : ext4 partition (rest of disk)
//!
//! Design constraints:
//!   • No full-image RAM buffer - every block is written to disk immediately.
//!   • Maximum 128 KiB allocated at any one time.
//!   • Works on disks from 32 MiB to 2 TiB.

pub mod elf_wrap;
mod ext4_mk;
pub mod fat;
mod mbr; // FAT16 ESP builder (UEFI install) - wiring in progress

use crate::block::BlockDevice;
use crate::diag::watchdog;
use crate::vfs_contract::VfsContract;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

// GRUB i386-pc binary images.
//
// Committed, curated blob: GRUB_BOOT_IMG / GRUB_CORE_IMG (BIOS) and the
// self-contained GRUB_EFI_IMG (the UEFI BOOTX64.EFI the installer actually
// uses).  build.rs only (re)writes BIOS images and ONLY when this file is empty
// - the populated committed content (incl. the EFI image) is preserved.
include!("grub_embed.rs");

// The BIOS GRUB images (boot.img / core.img) are retained for a possible future
// BIOS-install path; the current installer is UEFI-only and uses GRUB_EFI_IMG.
#[allow(dead_code)]
const _BIOS_GRUB_RETAINED: (&[u8], &[u8]) = (GRUB_BOOT_IMG, GRUB_CORE_IMG);

const SECTOR: usize = 512;
const PART_START: u64 = 2048; // LBA where the ext4 partition starts (1 MiB)
/// Minimum disk size we'll accept: 64 MiB.
const MIN_SECTORS: u64 = 64 * 1024 * 1024 / SECTOR as u64;

// -- Public entry point -----------------------------------------------------

/// Largest ESP we build (held fully in RAM before streaming to disk).  ~8 MiB
/// kernel + 0.7 MiB GRUB EFI leaves plenty of headroom; 48 MiB keeps the FAT16
/// cluster count comfortably inside the 4085..65524 range at 4 KiB clusters.
const ESP_MAX_BYTES: u64 = 48 * 1024 * 1024;

type InstallTarget = (alloc::sync::Arc<dyn BlockDevice>, u64, u64, u64);

fn target_disk() -> Result<InstallTarget, &'static str> {
    let dev = crate::block::get()
        .ok_or("No block device found. In VirtualBox: add a second VDI hard disk.")?;

    let total_sectors = dev.sector_count();
    if total_sectors < MIN_SECTORS {
        return Err("Disk too small - need at least 64 MiB");
    }

    if GRUB_EFI_IMG.is_empty() {
        crate::println!("[install] ERROR: GRUB BOOTX64.EFI not embedded.");
        crate::println!("[install] Fix: ensure grub-efi-amd64-bin is installed in WSL,");
        crate::println!("[install]   then regenerate src/install/grub_embed.rs.");
        return Err("GRUB EFI image not embedded - see above");
    }

    let esp_start = PART_START;
    let avail_bytes = (total_sectors - esp_start) * SECTOR as u64;
    let esp_bytes = avail_bytes.min(ESP_MAX_BYTES) & !(SECTOR as u64 - 1);
    let esp_sectors = esp_bytes / SECTOR as u64;
    Ok((dev, total_sectors, esp_start, esp_sectors))
}

fn build_esp_image(
    prefix: &str,
    elf_step: &str,
    fat_step: &str,
    esp_bytes: usize,
    grub_cfg: &[u8],
) -> Result<Vec<u8>, &'static str> {
    crate::print!("{} {}  Building kernel ELF ... ", prefix, elf_step);
    let elf = elf_wrap::build_elf().inspect_err(|e| {
        crate::println!("ELF error: {}", e);
    })?;
    crate::println!("OK ({} KiB)", elf.len() / 1024);

    crate::print!("{} {}  Building FAT ESP image ... ", prefix, fat_step);
    let mut fs = fat::FatBuilder::new(esp_bytes);
    fs.write_file("EFI/BOOT/BOOTX64.EFI", GRUB_EFI_IMG)?;
    fs.write_file("boot/grub/grub.cfg", grub_cfg)?;
    fs.write_file("boot/saios.elf", &elf)?;
    let img = fs.finish()?;
    crate::println!("OK ({} MiB)", img.len() / (1024 * 1024));
    Ok(img)
}

/// Install SAIOS to the Storage Platform-approved block device as a **UEFI** disk.
///
/// Layout: one MBR partition (type 0xEF, EFI System Partition) formatted FAT16,
/// starting at LBA 2048, containing:
///   /EFI/BOOT/BOOTX64.EFI   - self-contained GRUB (x86_64-efi) with early cfg
///   /boot/grub/grub.cfg     - installed GRUB config loaded from the ESP
///   /boot/saios.elf         - the running kernel, wrapped in a fresh ELF
///
/// UEFI firmware scans the partition table, finds the ESP, mounts it FAT, and
/// runs BOOTX64.EFI. The EFI image sets the GRUB prefix to the ESP and chains
/// into /boot/grub/grub.cfg, which keeps the boot policy editable without
/// rebuilding the PE image.
pub fn run(_dev_path: &str) -> Result<(), &'static str> {
    crate::println!("[install] analysis-only mode: no disk modifications will be made");
    let snapshot = crate::saios::storage_platform::decision_snapshot();
    let analysis = &snapshot.target;
    let plan = &snapshot.plan;
    crate::println!("[install] target: {}", analysis.classification);
    crate::println!("[install] risk: {}", analysis.risk.label());
    crate::println!(
        "[install] dual_boot_required: {}",
        analysis.dual_boot_required
    );
    if plan.operations.is_empty() {
        crate::println!("[install] required operations: none");
    } else {
        crate::println!("[install] required operations:");
        for operation in &plan.operations {
            crate::println!("[install]   - {}", operation);
        }
    }
    if let Some(reason) = plan.refusal_reason {
        crate::println!("[install] target issue: {}", reason);
        return Err(reason);
    }
    crate::println!("[install] recommendation: backup first and require explicit confirmation");
    Err("explicit confirmation required; run install or saios install")
}

pub fn run_approved(_dev_path: &str) -> Result<(), &'static str> {
    crate::println!("[install] Preparing user-approved operation...");

    let plan = crate::saios::storage_platform::install_gate()?;
    run_approved_with_plan(_dev_path, plan)
}

pub fn run_reinstall_approved(_dev_path: &str) -> Result<(), &'static str> {
    crate::println!("[reinstall] Preparing user-approved replacement...");

    let plan = crate::saios::storage_platform::reinstall_gate()?;
    run_approved_with_plan(_dev_path, plan)
}

fn run_approved_with_plan(
    _dev_path: &str,
    plan: crate::saios::storage_platform::OperationPlan,
) -> Result<(), &'static str> {
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationBegin,
        crate::kds::KdsSeverity::Info,
        [
            plan.operation_id,
            plan.operations.len() as u64,
            plan.risk as u64,
            plan.estimated_seconds,
        ],
    );

    let (dev, total_sectors, esp_start, esp_sectors) = match target_disk() {
        Ok(target) => target,
        Err(error) => return Err(record_install_failure(&plan, None, None, 1, error)),
    };
    let mut rollback_mbr = [0u8; SECTOR];
    if let Err(error) = dev.read_sectors(0, &mut rollback_mbr) {
        return Err(record_install_failure(&plan, Some(&*dev), None, 2, error));
    }
    let diagnostic = crate::block::diagnose();
    let root_state = crate::block::classify_root_filesystem(&diagnostic);
    crate::println!(
        "[install]   Disk: {} MiB ({} sectors)",
        total_sectors * SECTOR as u64 / (1024 * 1024),
        total_sectors
    );
    crate::println!(
        "[install]   Target state: {} ({})",
        crate::block::root_filesystem_state_name(root_state),
        crate::block::root_filesystem_status(root_state)
    );
    if root_state == crate::block::RootFilesystemState::PartitionTableMissing {
        crate::println!("[install]   Disk uninitialized - normal install target");
    }
    let esp_bytes = esp_sectors * SECTOR as u64;
    crate::println!(
        "[install]   ESP: FAT16 at LBA {} - {} MiB",
        esp_start,
        esp_bytes / (1024 * 1024)
    );
    crate::println!("[install]   GRUB EFI: {} KiB", GRUB_EFI_IMG.len() / 1024);
    crate::println!();

    let install_grub_cfg = grub_config();
    let img = match build_esp_image(
        "[install]",
        "1/6",
        "2/6",
        esp_bytes as usize,
        install_grub_cfg.as_bytes(),
    ) {
        Ok(image) => image,
        Err(error) => {
            return Err(record_install_failure(
                &plan,
                Some(&*dev),
                Some(&rollback_mbr),
                3,
                error,
            ));
        }
    };

    // -- Step 3: Stream the ESP image to disk --------------------------------
    crate::print!("[install] 3/6  Writing ESP to disk ... ");
    if let Err(error) = write_sectors_chunked(&*dev, esp_start, &img).inspect_err(|_e| {
        crate::println!("FAILED");
    }) {
        return Err(record_install_failure(
            &plan,
            Some(&*dev),
            Some(&rollback_mbr),
            4,
            error,
        ));
    }
    if let Err(error) = dev.flush() {
        return Err(record_install_failure(
            &plan,
            Some(&*dev),
            Some(&rollback_mbr),
            5,
            error,
        ));
    }
    crate::println!("OK");

    // -- Step 4: ext4 root/data partition filling the rest of the disk -------
    // Gives the installed system a persistent root filesystem (the FAT ESP only
    // holds the bootloader + kernel).  Without this the kernel finds no ext4 and
    // falls back to a volatile tmpfs root.
    let ext4_start = esp_start + esp_sectors;
    let ext4_sectors = total_sectors.saturating_sub(ext4_start);
    let have_ext4 = ext4_sectors >= (16 * 1024 * 1024 / SECTOR as u64); // ≥16 MiB
    if have_ext4 {
        crate::print!(
            "[install] 4/6  Formatting ext4 root at LBA {} ({} MiB) ... ",
            ext4_start,
            ext4_sectors * SECTOR as u64 / (1024 * 1024)
        );
        if let Err(error) = format_ext4(&*dev, ext4_start, ext4_sectors) {
            return Err(record_install_failure(
                &plan,
                Some(&*dev),
                Some(&rollback_mbr),
                6,
                error,
            ));
        }
        crate::println!("OK");
    } else {
        crate::println!("[install] 4/6  (disk too small for ext4 root - skipped)");
    }

    // -- Step 5: MBR partition table - ESP (0xEF) + ext4 (0x83) --------------
    crate::print!("[install] 5/6  Writing MBR partition table ... ");
    let ext4_part = if have_ext4 {
        Some((ext4_start, ext4_sectors))
    } else {
        None
    };
    if let Err(error) =
        mbr::write_efi_mbr(&*dev, esp_start, esp_sectors, ext4_part).inspect_err(|_e| {
            crate::println!("FAILED");
        })
    {
        return Err(record_install_failure(
            &plan,
            Some(&*dev),
            Some(&rollback_mbr),
            7,
            error,
        ));
    }
    crate::println!("OK");

    if have_ext4 {
        crate::print!("[install] 6/6  Seeding authoritative rootfs ... ");
        if let Err(error) =
            populate_installed_rootfs(dev.clone(), plan.operation_id, ext4_start, ext4_sectors)
        {
            return Err(record_install_failure(
                &plan,
                Some(&*dev),
                Some(&rollback_mbr),
                8,
                error,
            ));
        }
        if dev.flush().is_err() {
            return Err(record_install_failure(
                &plan,
                Some(&*dev),
                Some(&rollback_mbr),
                9,
                "disk flush failed after rootfs population",
            ));
        }
        crate::println!("OK");
    }

    crate::println!();
    crate::println!("╔══════════════════════════════════════╗");
    crate::println!("║  SAIOS installed (UEFI) successfully!║");
    crate::println!("║                                      ║");
    crate::println!("║  1. Power off the virtual machine    ║");
    crate::println!("║  2. Detach SAIOS live media          ║");
    crate::println!("║  3. Ensure VM firmware = EFI         ║");
    crate::println!("║  4. Boot from the hard disk          ║");
    crate::println!("╚══════════════════════════════════════╝");
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationComplete,
        crate::kds::KdsSeverity::Info,
        [
            plan.operation_id,
            total_sectors,
            esp_sectors,
            have_ext4 as u64,
        ],
    );
    Ok(())
}

fn record_install_failure(
    plan: &crate::saios::storage_platform::OperationPlan,
    dev: Option<&dyn BlockDevice>,
    rollback_mbr: Option<&[u8; SECTOR]>,
    step: u64,
    error: &'static str,
) -> &'static str {
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationFailure,
        crate::kds::KdsSeverity::Error,
        [
            plan.operation_id,
            step,
            plan.operations.len() as u64,
            plan.risk as u64,
        ],
    );
    if let (Some(dev), Some(rollback_mbr)) = (dev, rollback_mbr) {
        let rollback_ok = dev
            .write_sectors(0, rollback_mbr)
            .and_then(|_| dev.flush())
            .is_ok();
        crate::observability_contract::ObservabilityContract::kds_event(
            crate::kds::KdsSubsystem::Storage,
            crate::kds::KdsEventType::DiskOperationRollback,
            if rollback_ok {
                crate::kds::KdsSeverity::Info
            } else {
                crate::kds::KdsSeverity::Error
            },
            [plan.operation_id, step, rollback_ok as u64, 1],
        );
    }
    error
}

fn populate_installed_rootfs(
    dev: Arc<dyn BlockDevice>,
    operation_id: u64,
    root_start_lba: u64,
    root_size_lba: u64,
) -> Result<(), &'static str> {
    let root = VfsContract::mount_install_rootfs(dev)?;

    for dir in crate::saios::rootfs::AUTHORITATIVE_ROOTS {
        VfsContract::ensure_install_dir(&root, dir)?;
    }
    for dir in crate::saios::rootfs::AUTHORITATIVE_DIRS {
        VfsContract::ensure_install_dir(&root, dir)?;
    }
    for dir in crate::saios::rootfs::COMPATIBILITY_ROOTS {
        VfsContract::ensure_install_dir(&root, dir)?;
    }
    for dir in crate::saios::rootfs::COMPATIBILITY_DIRS {
        VfsContract::ensure_install_dir(&root, dir)?;
    }
    for dir in crate::saios::rootfs::LEGACY_ROOTS {
        VfsContract::ensure_install_dir(&root, dir)?;
    }
    for dir in crate::saios::rootfs::WINDOWS_COMPAT_DIRS {
        VfsContract::ensure_install_dir(&root, dir)?;
    }
    for dir in crate::saios::rootfs::MACOS_COMPAT_DIRS {
        VfsContract::ensure_install_dir(&root, dir)?;
    }

    for (path, data) in crate::saios::rootfs::initial_files() {
        let mode = if matches!(path, "/bin/sh" | "/bin/bash") {
            0o755
        } else {
            0o644
        };
        VfsContract::write_install_file(&root, path, &data, mode)?;
    }

    crate::saios::storage_platform::seed_installed_metadata(
        &root,
        operation_id,
        root_start_lba,
        root_size_lba,
    )?;

    Ok(())
}

pub fn update(_dev_path: &str) -> Result<(), &'static str> {
    crate::println!("[update] Preparing user-approved install-over-existing operation...");
    let plan = crate::saios::storage_platform::update_gate()?;
    run_approved_with_plan(_dev_path, plan)
}

// -- ext4 partition formatter ----------------------------------------------
//
// We write only the bare minimum to make GRUB happy:
//   Block 0  (byte 0)     : boot block (unused - zero)
//   Block 1  (byte 4096)  : ext4 superblock
//   Block 2  (byte 8192)  : block group descriptor table
//   Block 3  (byte 12288) : block bitmap
//   Block 4  (byte 16384) : inode bitmap
//   Blocks 5-260          : inode table (256 inodes × 256 bytes)
//   Block 261+            : data blocks (files)
//
// Files are written sequentially starting at block 261.
// We track the next free data block in a simple counter.

const BLOCK: usize = 4096;
const INODE_SIZE: usize = 256;
const INODES_PER_BLOCK: usize = BLOCK / INODE_SIZE; // 16

// -- Multi-block-group ext4 geometry (4 KiB blocks) -------------------------
// ext4 splits the volume into block groups.  One group's block bitmap is a
// single block (BLOCK*8 = 32768 bits), so 32768 blocks (128 MiB) is the HARD
// maximum blocks-per-group for 4 KiB blocks.  A 50 GiB partition therefore
// needs ~400 groups, each with its own block bitmap, inode bitmap and inode
// table.  (The previous formatter declared ONE group spanning the whole disk -
// blocks_per_group ≈ 13 million - which is an invalid filesystem that GRUB and
// e2fsprogs both reject, the root cause of the un-bootable install.)
const BPG: u32 = 32768; // blocks per group
const IPG: u32 = 2048; // inodes per group
const ITB: u32 = IPG * INODE_SIZE as u32 / BLOCK as u32; // inode-table blocks/group (128)
const DESC_SIZE: u32 = 32; // group descriptor size (no 64-bit feat)

// Group-0 layout depends on the disk size (gdt size), so it is computed at
// format time and shared with write_inode / write_ext4_file via these statics.
static G0_INODE_TABLE: AtomicU32 = AtomicU32::new(0); // group-0 inode-table start block
static G0_DATA_START: AtomicU32 = AtomicU32::new(0); // group-0 first data block
static G0_BLOCK_BITMAP: AtomicU32 = AtomicU32::new(0); // group-0 block-bitmap block

// Inode numbers
const INO_ROOT: u32 = 2;
const INO_BOOT: u32 = 11;
const INO_GRUB: u32 = 12;
const INO_CFG: u32 = 13;
const INO_ELF: u32 = 14;

// EXT4 constants
const EXT4_MAGIC: u16 = 0xEF53;
const EXT4_EXTENTS_FL: u32 = 0x80000;
const S_IFDIR: u16 = 0o040755;
const S_IFREG: u16 = 0o100644;

/// Format the partition as a proper multi-block-group ext4 filesystem.
///
/// Writes the primary superblock + group-descriptor table, then every block
/// group's block & inode bitmaps, and finally group 0's inode table and the
/// root / boot / grub directory blocks.  All files live in group 0 (a single
/// group spans 128 MiB - far more than the ~6 MiB kernel), so GRUB only ever
/// reads group 0.
#[allow(dead_code)]
fn format_ext4(
    dev: &dyn BlockDevice,
    part_lba: u64,
    part_sectors: u64,
) -> Result<(), &'static str> {
    let total_blocks = (part_sectors * SECTOR as u64 / BLOCK as u64) as u32;
    if total_blocks < 64 {
        return Err("install: partition too small");
    }
    let groups = total_blocks.div_ceil(BPG);
    let total_inodes = groups.saturating_mul(IPG);
    let gdt_blocks = (groups * DESC_SIZE).div_ceil(BLOCK as u32);

    // -- Group-0 metadata layout -----------------------------------------------
    //   block 0            superblock (at byte 1024) + boot area
    //   blocks 1..1+gdt    group descriptor table
    //   block  bb0         block bitmap
    //   block  ib0         inode bitmap
    //   blocks it0..+ITB   inode table
    //   block  data0+      directories + files
    let bb0 = 1 + gdt_blocks;
    let ib0 = bb0 + 1;
    let it0 = ib0 + 1;
    let data0 = it0 + ITB;
    G0_BLOCK_BITMAP.store(bb0, Ordering::SeqCst);
    G0_INODE_TABLE.store(it0, Ordering::SeqCst);
    G0_DATA_START.store(data0, Ordering::SeqCst);

    // Group-0 reserves: metadata + 3 directory blocks + grub.cfg + first elf
    // block.  write_ext4_file extends the bitmap for the rest of the kernel.
    let g0_meta_used = data0 + 5;

    // -- Build the group-descriptor table + write each group's bitmaps ---------
    let mut gdt = alloc::vec![0u8; gdt_blocks as usize * BLOCK];
    let mut total_free_blocks: u64 = 0;
    let mut total_free_inodes: u64 = 0;

    for g in 0..groups {
        let group_start = g * BPG;
        let group_blocks = if g == groups - 1 {
            total_blocks - group_start
        } else {
            BPG
        };

        // Metadata block numbers (absolute).  Group 0 sits after the SB+GDT;
        // every other group keeps its bitmaps + inode table at the group start.
        let (bb, ib, it, meta_used) = if g == 0 {
            (bb0, ib0, it0, g0_meta_used)
        } else {
            (group_start, group_start + 1, group_start + 2, 2 + ITB)
        };

        // Block bitmap (one block, 32768 bits).  Bit b ↔ block (group_start + b);
        // `meta_used` blocks at the group start hold the metadata.
        let mut bm = [0u8; BLOCK];
        for b in 0..meta_used {
            bm[(b / 8) as usize] |= 1 << (b % 8);
        }
        // Blocks past the (possibly short) last group don't exist → mark used.
        for b in group_blocks..BPG {
            bm[(b / 8) as usize] |= 1 << (b % 8);
        }
        write_block(dev, part_lba, bb, &bm)?;

        // Inode bitmap: group 0 marks inodes 1..14 used; every group marks the
        // non-existent inodes past IPG used.
        let mut im = [0u8; BLOCK];
        let used_inodes = if g == 0 { 14u32 } else { 0 };
        for i in 0..used_inodes {
            im[(i / 8) as usize] |= 1 << (i % 8);
        }
        for i in IPG..(BLOCK as u32 * 8) {
            im[(i / 8) as usize] |= 1 << (i % 8);
        }
        write_block(dev, part_lba, ib, &im)?;

        let free_blocks = group_blocks.saturating_sub(meta_used);
        let free_inodes = IPG - used_inodes;
        total_free_blocks += free_blocks as u64;
        total_free_inodes += free_inodes as u64;

        // Group descriptor (32 bytes, non-64-bit).
        let off = (g * DESC_SIZE) as usize;
        let d = &mut gdt[off..off + DESC_SIZE as usize];
        w32(d, 0, bb); // bg_block_bitmap_lo
        w32(d, 4, ib); // bg_inode_bitmap_lo
        w32(d, 8, it); // bg_inode_table_lo
        w16(d, 12, free_blocks.min(0xFFFF) as u16);
        w16(d, 14, free_inodes.min(0xFFFF) as u16);
        w16(d, 16, if g == 0 { 3 } else { 0 }); // bg_used_dirs_count_lo

        // Report progress - ext4 format can take several seconds for large disks
        if g % 8 == 0 || g + 1 == groups {
            watchdog::note_progress();
        }
    }

    // Write the GDT (block 1 .. 1+gdt_blocks).
    for i in 0..gdt_blocks {
        let s = i as usize * BLOCK;
        write_block(dev, part_lba, 1 + i, &gdt[s..s + BLOCK])?;
        // Progress notification - GDT write can be slow for large partitions
        if i % 16 == 0 || i + 1 == gdt_blocks {
            watchdog::note_progress();
        }
    }

    // -- Primary superblock (block 0, byte 1024) ------------------------------
    let mut sb = [0u8; BLOCK];
    {
        let s = &mut sb[1024..];
        w32(s, 0x00, total_inodes); // s_inodes_count
        w32(s, 0x04, total_blocks); // s_blocks_count_lo
        w32(s, 0x08, 0); // s_r_blocks_count_lo
        w32(s, 0x0C, total_free_blocks as u32); // s_free_blocks_count_lo
        w32(s, 0x10, total_free_inodes as u32); // s_free_inodes_count
        w32(s, 0x14, 0); // s_first_data_block (0 for >1K blocks)
        w32(s, 0x18, 2); // s_log_block_size  → 4 KiB
        w32(s, 0x1C, 2); // s_log_cluster_size
        w32(s, 0x20, BPG); // s_blocks_per_group
        w32(s, 0x24, BPG); // s_clusters_per_group
        w32(s, 0x28, IPG); // s_inodes_per_group
        w16(s, 0x34, 0); // s_mnt_count
        w16(s, 0x36, 0xFFFF); // s_max_mnt_count (-1)
        s[0x38] = (EXT4_MAGIC & 0xFF) as u8; // s_magic lo
        s[0x39] = (EXT4_MAGIC >> 8) as u8; // s_magic hi
        w16(s, 0x3A, 1); // s_state: cleanly unmounted
        w16(s, 0x3C, 1); // s_errors: continue
        w32(s, 0x4C, 1); // s_rev_level: dynamic
        w32(s, 0x54, 11); // s_first_ino
        w16(s, 0x58, INODE_SIZE as u16); // s_inode_size
        w16(s, 0x5A, 0); // s_block_group_nr (primary)
        w32(s, 0x5C, 0); // s_feature_compat
        w32(s, 0x60, 0x0042); // s_feature_incompat: filetype | extents
        w32(s, 0x64, 0x0001); // s_feature_ro_compat: sparse_super
        s[0x68..0x78].copy_from_slice(
            // s_uuid
            &[
                0x5A, 0xA1, 0x05, 0x10, 0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x13, 0x37,
                0xC0, 0xDE,
            ],
        );
        s[0x78..0x80].copy_from_slice(b"SAIOS\0\0\0"); // s_volume_name
    }
    write_block(dev, part_lba, 0, &sb)?;

    // -- Group-0 inode table: root + boot + grub dirs, grub.cfg + saios.elf ----
    // Directory i_size MUST be the block size (GRUB reads exactly i_size bytes,
    // so size 0 makes a directory look EMPTY → blank GRUB cursor).
    // This ext4 is the UEFI install's DATA/ROOT partition - the bootloader and
    // kernel live on the FAT ESP, NOT here.  Create only an empty root directory:
    // the kernel's init_rootfs() populates bin/etc/home/... after mounting, and
    // they persist.  Critically we must NOT create a /boot/saios.elf here - GRUB's
    // `search --file /boot/saios.elf` would otherwise match this empty ext4 copy
    // instead of the real kernel on the ESP ("premature end of file").
    write_inode(dev, part_lba, INO_ROOT, S_IFDIR, BLOCK as u64, data0, 1)?;

    // Root directory (data0): just "." and ".." (empty root).
    let mut dir = [0u8; BLOCK];
    let pos = write_dirent(&mut dir, 0, INO_ROOT, 2, b".");
    let _ = write_dirent_last(&mut dir, pos, INO_ROOT, 2, b"..");
    write_block(dev, part_lba, data0, &dir)?;

    // -- Read the superblock back to prove the write landed --------------------
    // (Catches silent AHCI/host-cache write failures before we declare success.)
    let mut chk = [0u8; 64];
    dev.read_bytes(part_lba * SECTOR as u64 + 1024, &mut chk)
        .map_err(|_| "ext4: superblock read-back failed")?;
    if chk[0x38] != (EXT4_MAGIC & 0xFF) as u8 || chk[0x39] != (EXT4_MAGIC >> 8) as u8 {
        crate::println!(
            "[install] ERROR: superblock read-back magic={:02x}{:02x} (want ef53) - write not landing",
            chk[0x39],
            chk[0x38]
        );
        return Err("ext4: superblock did not persist");
    }
    crate::println!(
        "[install]   ext4: {} groups, {} blocks, {} inodes (sb magic OK)",
        groups,
        total_blocks,
        total_inodes
    );
    Ok(())
}

/// Write a file's data blocks and update its inode with the real size + extent.
/// Files live in group 0: grub.cfg at data0+3, saios.elf at data0+4 onward.
#[allow(dead_code)]
fn write_ext4_file(
    dev: &dyn BlockDevice,
    part_lba: u64,
    name: &str,
    data: &[u8],
) -> Result<(), &'static str> {
    let data0 = G0_DATA_START.load(Ordering::SeqCst);
    let (ino, first_blk) = match name {
        "grub.cfg" => (INO_CFG, data0 + 3),
        "saios.elf" => (INO_ELF, data0 + 4),
        _ => return Err("install: unknown file name"),
    };

    let num_blocks = data.len().div_ceil(BLOCK) as u32;

    // Write the file's data blocks, reporting a live progress bar.
    for i in 0..num_blocks {
        let mut blk = [0u8; BLOCK];
        let start = i as usize * BLOCK;
        let end = (start + BLOCK).min(data.len());
        blk[..end - start].copy_from_slice(&data[start..end]);
        write_block(dev, part_lba, first_blk + i, &blk)?;

        if i % 64 == 0 || i + 1 == num_blocks {
            crate::shell::progress_set(
                "writing",
                (i as u64 + 1) * BLOCK as u64,
                num_blocks as u64 * BLOCK as u64,
            );
            crate::shell::progress_render();
        }
    }
    crate::shell::progress_clear();
    crate::println!();

    // Update the inode with the real size + extent.
    write_inode(
        dev,
        part_lba,
        ino,
        S_IFREG,
        data.len() as u64,
        first_blk,
        num_blocks,
    )?;

    // Mark the file's blocks used in group 0's block bitmap.
    let bb0 = G0_BLOCK_BITMAP.load(Ordering::SeqCst);
    let mut bm = [0u8; BLOCK];
    dev.read_bytes(
        part_lba * SECTOR as u64 + bb0 as u64 * BLOCK as u64,
        &mut bm,
    )
    .map_err(|_| "bitmap read failed")?;
    for i in 0..num_blocks {
        let b = (first_blk + i) as usize % (BLOCK * 8);
        bm[b / 8] |= 1 << (b % 8);
    }
    write_block(dev, part_lba, bb0, &bm)?;

    Ok(())
}

// -- Low-level helpers -----------------------------------------------------

/// Write one 4 KiB block to the partition.
/// Translates block number → byte offset on the device.
#[allow(dead_code)]
fn write_block(
    dev: &dyn BlockDevice,
    part_lba: u64,
    block: u32,
    data: &[u8],
) -> Result<(), &'static str> {
    let byte_off = part_lba * SECTOR as u64 + block as u64 * BLOCK as u64;
    dev.write_bytes(byte_off, data)
        .map_err(|_| "block write failed")
}

/// Write one inode using extent-tree encoding.
#[allow(dead_code)]
fn write_inode(
    dev: &dyn BlockDevice,
    part_lba: u64,
    ino: u32,
    mode: u16,
    size: u64,
    data_block: u32,
    num_blocks: u32,
) -> Result<(), &'static str> {
    // All our inodes (1..14) live in group 0's inode table.
    // Inode N (1-based) lives at table_offset = (N-1) * INODE_SIZE.
    let it_start = G0_INODE_TABLE.load(Ordering::SeqCst) as u64;
    let idx = (ino - 1) as u64;
    let blk_num = it_start + idx / INODES_PER_BLOCK as u64;
    let blk_off = (idx % INODES_PER_BLOCK as u64) as usize * INODE_SIZE;

    // Read the whole inode table block, modify, write back
    let mut blk = [0u8; BLOCK];
    dev.read_bytes(part_lba * SECTOR as u64 + blk_num * BLOCK as u64, &mut blk)
        .map_err(|_| "inode table read failed")?;

    let s = &mut blk[blk_off..blk_off + INODE_SIZE];
    w16(s, 0, mode);
    w32(s, 4, size as u32); // i_size_lo
    w32(s, 28, num_blocks * 8); // i_blocks_lo (512-byte units)
    w32(s, 32, EXT4_EXTENTS_FL); // use extent tree

    // Write minimal extent header + one leaf extent
    // Extent header at offset 40 (i_block[0..])
    let eh = &mut s[40..52];
    w16(eh, 0, 0xF30A); // eh_magic
    w16(eh, 2, if num_blocks > 0 { 1 } else { 0 }); // eh_entries
    w16(eh, 4, 4); // eh_max
    w16(eh, 6, 0); // eh_depth (leaf)

    if num_blocks > 0 {
        // First extent at offset 52 (right after header)
        let ext = &mut s[52..64];
        w32(ext, 0, 0); // ee_block (logical 0)
        w16(ext, 4, num_blocks as u16); // ee_len
        w16(ext, 6, 0); // ee_start_hi
        w32(ext, 8, data_block); // ee_start_lo
    }

    // Link count
    w16(s, 26, if mode & 0xF000 == 0o040000 { 2 } else { 1 });

    dev.write_bytes(part_lba * SECTOR as u64 + blk_num * BLOCK as u64, &blk)
        .map_err(|_| "inode table write failed")
}

/// Write a directory entry. Returns new position.
#[allow(dead_code)]
fn write_dirent(buf: &mut [u8], pos: usize, ino: u32, ftype: u8, name: &[u8]) -> usize {
    let rec = (8 + name.len()).div_ceil(4) * 4;
    if pos + rec > buf.len() {
        return pos;
    }
    w32(buf, pos, ino);
    w16(buf, pos + 4, rec as u16);
    buf[pos + 6] = name.len() as u8;
    buf[pos + 7] = ftype;
    buf[pos + 8..pos + 8 + name.len()].copy_from_slice(name);
    pos + rec
}

/// Write the last directory entry - extends its rec_len to fill the block.
#[allow(dead_code)]
fn write_dirent_last(buf: &mut [u8], pos: usize, ino: u32, ftype: u8, name: &[u8]) -> usize {
    let rec = BLOCK - pos; // absorb all remaining space
    if pos + rec > buf.len() {
        return pos;
    }
    w32(buf, pos, ino);
    w16(buf, pos + 4, rec as u16);
    buf[pos + 6] = name.len() as u8;
    buf[pos + 7] = ftype;
    buf[pos + 8..pos + 8 + name.len()].copy_from_slice(name);
    pos + rec
}

/// Write data in 32-sector (16 KiB) chunks - respects VirtIO-Block ring size.
fn write_sectors_chunked(
    dev: &dyn BlockDevice,
    start_lba: u64,
    data: &[u8],
) -> Result<(), &'static str> {
    const CHUNK: usize = 32 * SECTOR; // 16 KiB per write
    let mut lba = start_lba;
    let mut offset = 0;
    let used = esp_used_len(data);

    let total = used as u64;
    let mut since_report = 0usize;
    while offset < used {
        let end = (offset + CHUNK).min(used);
        let chunk = &data[offset..end];
        // Pad to sector boundary
        let padded_len = chunk.len().div_ceil(SECTOR) * SECTOR;
        let mut buf = alloc::vec![0u8; padded_len];
        buf[..chunk.len()].copy_from_slice(chunk);
        dev.write_sectors(lba, &buf)
            .map_err(|_| "sector write failed")?;
        lba += (padded_len / SECTOR) as u64;
        offset += chunk.len();
        // Live progress bar (~every 1 MiB) - the ESP carries the ~15 MiB kernel.
        since_report += chunk.len();
        if since_report >= 1024 * 1024 || offset == data.len() {
            since_report = 0;
            crate::shell::progress_set("writing ESP", offset as u64, total);
            crate::shell::progress_render();
        }
        // Watchdog progress - long ESP writes need to report forward progress
        watchdog::note_progress();
    }
    crate::shell::progress_clear();
    crate::println!();
    Ok(())
}

fn esp_used_len(data: &[u8]) -> usize {
    // The FAT ESP image is sized to tens of MiB but only its first portion (boot
    // sector + FATs + GRUB + kernel) is non-zero; the rest is free clusters left
    // at 0.  Don't write that empty tail - it roughly halves the (one-sector-per-
    // command) AHCI write and is what made install look like it stalled.
    let mut used = data.len();
    while used > SECTOR && data[used - 1] == 0 {
        used -= 1;
    }
    (used.div_ceil(SECTOR) * SECTOR).min(data.len())
}

fn restore_update_snapshot(
    dev: &dyn BlockDevice,
    plan: &crate::saios::storage_platform::OperationPlan,
    esp_start: u64,
    esp_sectors: u64,
    snapshot: &crate::saios::storage_platform::UpdatePreservationSnapshot,
    reason: u64,
) {
    crate::observability_contract::ObservabilityContract::kds_event(
        crate::kds::KdsSubsystem::Storage,
        crate::kds::KdsEventType::DiskOperationRollback,
        crate::kds::KdsSeverity::Warn,
        [plan.operation_id, snapshot.snapshot_id, esp_sectors, reason],
    );
    let esp_bytes = esp_sectors.saturating_mul(SECTOR as u64) as usize;
    let restored = build_esp_image(
        "[rollback]",
        "1/2",
        "2/2",
        esp_bytes,
        &snapshot.previous_boot_policy,
    );
    if restored
        .and_then(|image| write_sectors_chunked(dev, esp_start, &image))
        .and_then(|_| dev.flush())
        .is_ok()
    {
        crate::observability_contract::ObservabilityContract::kds_event(
            crate::kds::KdsSubsystem::Storage,
            crate::kds::KdsEventType::DiskOperationRollback,
            crate::kds::KdsSeverity::Info,
            [plan.operation_id, snapshot.snapshot_id, esp_sectors, 0],
        );
    } else {
        crate::observability_contract::ObservabilityContract::kds_event(
            crate::kds::KdsSubsystem::Storage,
            crate::kds::KdsEventType::DiskOperationFailure,
            crate::kds::KdsSeverity::Error,
            [plan.operation_id, esp_start, esp_sectors, 10 + reason],
        );
    }
}

// -- Little-endian write helpers --------------------------------------------

fn w16(buf: &mut [u8], off: usize, v: u16) {
    if off + 2 <= buf.len() {
        buf[off] = v as u8;
        buf[off + 1] = (v >> 8) as u8;
    }
}
fn w32(buf: &mut [u8], off: usize, v: u32) {
    if off + 4 <= buf.len() {
        buf[off] = v as u8;
        buf[off + 1] = (v >> 8) as u8;
        buf[off + 2] = (v >> 16) as u8;
        buf[off + 3] = (v >> 24) as u8;
    }
}

// -- GRUB configuration ----------------------------------------------------

#[allow(dead_code)]
fn grub_config() -> String {
    // Keep the current GOP mode instead of forcing a specific resolution.
    // That avoids "no suitable video mode found" on firmwares whose GOP mode
    // list does not include 1024x768x32 while still passing a framebuffer to
    // the kernel when one is active.
    alloc::format!(
        "insmod part_msdos\n\
         insmod part_gpt\n\
         insmod fat\n\
         insmod ext2\n\
         insmod multiboot2\n\
         insmod efi_gop\n\
         insmod video_bochs\n\
         insmod video_cirrus\n\
         insmod gfxterm\n\
         set gfxmode=auto\n\
         terminal_output gfxterm\n\
         set gfxpayload=keep\n\
         set timeout=5\n\
         set default=0\n\
         menuentry \"SAIOS\" {{\n\
             echo \"Loading SAIOS...\"\n\
             multiboot2 /boot/saios.elf saios.boot=hdd saios.mode={}\n\
             boot\n\
         }}\n\
         menuentry \"SAIOS Safe Mode\" {{\n\
             echo \"Loading SAIOS Safe Mode...\"\n\
             multiboot2 /boot/saios.elf saios.boot=hdd saios.mode={}\n\
             boot\n\
         }}\n",
        crate::boot_mode::BootMode::FirstBoot.as_str(),
        crate::boot_mode::BootMode::Safe.as_str()
    )
}
