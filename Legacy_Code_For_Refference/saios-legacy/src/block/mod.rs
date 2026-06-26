//! Block device layer — abstraction over AHCI/SATA, VirtIO-Block, etc.
//!
//! Probe order (first found wins):
//!   1. AHCI / SATA  — VirtualBox default (VDI on SATA port)
//!   2. VirtIO-Block — QEMU / VirtualBox virtio controller

pub mod ahci;
pub mod virtio_blk;

use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageController {
    Unknown,
    Ahci,
    VirtioBlk,
}

#[derive(Clone, Copy, Debug)]
pub struct BlockDeviceInfo {
    pub controller: StorageController,
    pub port: Option<u32>,
    pub sector_count: u64,
    pub sector_size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionTableKind {
    Mbr,
    Gpt,
    Fallback,
}

#[derive(Clone, Copy, Debug)]
pub struct PartitionInfo {
    pub index: usize,
    pub table: PartitionTableKind,
    pub type_code: u8,
    pub start_lba: u64,
    pub size_lba: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Ext4ProbeInfo {
    pub partition_index: Option<usize>,
    pub probe_target_lba: u64,
    pub superblock_lba: u64,
    pub superblock_offset: u64,
    pub expected_magic: u16,
    pub actual_magic: u16,
    pub read_ok: bool,
    pub result: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootFilesystemState {
    NoDisk,
    DiskPresent,
    PartitionTableMissing,
    PartitionTablePresent,
    FilesystemMissing,
    FilesystemCorrupt,
    FilesystemValid,
    RootMounted,
}

#[derive(Clone, Debug)]
pub struct StorageDiagnostic {
    pub disk_detected: bool,
    pub device: Option<BlockDeviceInfo>,
    pub mbr_valid: bool,
    pub gpt_valid: bool,
    pub partitions: Vec<PartitionInfo>,
    pub probes: Vec<Ext4ProbeInfo>,
    pub root_mount_success: bool,
    pub root_mount_failure: Option<&'static str>,
}

#[derive(Clone, Copy, Debug)]
struct RootMountState {
    attempted: bool,
    success: bool,
    failure: Option<&'static str>,
}

const EXT4_MAGIC: u16 = 0xEF53;
const EMPTY_ROOT_MOUNT: RootMountState = RootMountState {
    attempted: false,
    success: false,
    failure: None,
};

pub trait BlockDevice: Send + Sync {
    fn sector_size(&self) -> usize {
        512
    }
    fn sector_count(&self) -> u64;
    fn device_info(&self) -> BlockDeviceInfo {
        BlockDeviceInfo {
            controller: StorageController::Unknown,
            port: None,
            sector_count: self.sector_count(),
            sector_size: self.sector_size(),
        }
    }
    fn read_sectors(&self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str>;
    fn write_sectors(&self, lba: u64, buf: &[u8]) -> Result<(), &'static str>;

    /// Flush any writes the device deferred while write-through was disabled.
    fn flush(&self) -> Result<(), &'static str> {
        Ok(())
    }

    /// When `on` (the default), each write is flushed to stable storage before
    /// returning.  When `off`, the device MAY defer flushes until `flush()` is
    /// called — far faster for bulk writes (a per-block flush turned a 48 MB
    /// `apt` index write into ~12k flush commands, which looked like a hang).
    fn set_write_through(&self, _on: bool) {}

    fn read_bytes(&self, offset: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        let ss = self.sector_size() as u64;
        let start_lba = offset / ss;
        let end_lba = (offset + buf.len() as u64).div_ceil(ss);
        let total_secs = (end_lba - start_lba) as usize;

        let mut tmp = alloc::vec![0u8; total_secs * self.sector_size()];
        self.read_sectors(start_lba, &mut tmp)?;

        let off_in_buf = (offset % ss) as usize;
        buf.copy_from_slice(&tmp[off_in_buf..off_in_buf + buf.len()]);
        Ok(())
    }

    fn write_bytes(&self, offset: u64, buf: &[u8]) -> Result<(), &'static str> {
        let ss = self.sector_size() as u64;
        // Fast path: a sector-aligned, whole-sector write needs no read-modify-
        // write.  ext4 block writes are block-aligned and block-sized, so this
        // eliminates a redundant read per block on bulk writes (the 48 MB apt
        // index went from ~98k extra sector reads to zero).
        if offset.is_multiple_of(ss) && (buf.len() as u64).is_multiple_of(ss) && !buf.is_empty() {
            return self.write_sectors(offset / ss, buf);
        }

        let start_lba = offset / ss;
        let end_lba = (offset + buf.len() as u64).div_ceil(ss);
        let total_secs = (end_lba - start_lba) as usize;

        // Read-modify-write for unaligned access
        let mut tmp = alloc::vec![0u8; total_secs * self.sector_size()];
        self.read_sectors(start_lba, &mut tmp)?;

        let off_in_buf = (offset % ss) as usize;
        tmp[off_in_buf..off_in_buf + buf.len()].copy_from_slice(buf);
        self.write_sectors(start_lba, &tmp)
    }
}

static DISK: Mutex<Option<Arc<dyn BlockDevice>>> = Mutex::new(None);
static ROOT_MOUNT: Mutex<RootMountState> = Mutex::new(EMPTY_ROOT_MOUNT);

pub fn register(dev: Arc<dyn BlockDevice>) {
    let sectors = dev.sector_count();
    let mb = sectors / 2048;
    crate::serial_println!("[blk] disk: {} sectors = {} MiB", sectors, mb);
    *DISK.lock() = Some(dev);
}

pub fn get() -> Option<Arc<dyn BlockDevice>> {
    DISK.lock().clone()
}

pub fn present() -> bool {
    DISK.lock().is_some()
}

pub fn sync() -> Result<(), &'static str> {
    match get() {
        Some(dev) => dev.flush(),
        None => Ok(()),
    }
}

pub fn record_root_mount_result(success: bool, failure: Option<&'static str>) {
    *ROOT_MOUNT.lock() = RootMountState {
        attempted: true,
        success,
        failure,
    };
}

pub fn controller_name(controller: StorageController) -> &'static str {
    match controller {
        StorageController::Unknown => "unknown",
        StorageController::Ahci => "AHCI",
        StorageController::VirtioBlk => "VirtIO-Block",
    }
}

pub fn partition_table_name(kind: PartitionTableKind) -> &'static str {
    match kind {
        PartitionTableKind::Mbr => "MBR",
        PartitionTableKind::Gpt => "GPT",
        PartitionTableKind::Fallback => "fallback",
    }
}

pub fn root_filesystem_state_name(state: RootFilesystemState) -> &'static str {
    match state {
        RootFilesystemState::NoDisk => "No Disk",
        RootFilesystemState::DiskPresent => "Disk Present",
        RootFilesystemState::PartitionTableMissing => "Partition Table Missing",
        RootFilesystemState::PartitionTablePresent => "Partition Table Present",
        RootFilesystemState::FilesystemMissing => "Filesystem Missing",
        RootFilesystemState::FilesystemCorrupt => "Filesystem Corrupt",
        RootFilesystemState::FilesystemValid => "Filesystem Valid",
        RootFilesystemState::RootMounted => "Root Mounted",
    }
}

pub fn root_filesystem_status(state: RootFilesystemState) -> &'static str {
    match state {
        RootFilesystemState::NoDisk => "No disk",
        RootFilesystemState::DiskPresent => "Disk present",
        RootFilesystemState::PartitionTableMissing => "Disk uninitialized",
        RootFilesystemState::PartitionTablePresent => "Partition table present",
        RootFilesystemState::FilesystemMissing => "Filesystem missing",
        RootFilesystemState::FilesystemCorrupt => "Filesystem corrupt",
        RootFilesystemState::FilesystemValid => "Filesystem valid",
        RootFilesystemState::RootMounted => "Root mounted",
    }
}

pub fn diagnose() -> StorageDiagnostic {
    let Some(dev) = get() else {
        let root = *ROOT_MOUNT.lock();
        return StorageDiagnostic {
            disk_detected: false,
            device: None,
            mbr_valid: false,
            gpt_valid: false,
            partitions: Vec::new(),
            probes: Vec::new(),
            root_mount_success: root.success,
            root_mount_failure: root.failure,
        };
    };

    let device = dev.device_info();
    let mut mbr = [0u8; 512];
    let mbr_valid = dev.read_bytes(0, &mut mbr).is_ok() && mbr[510] == 0x55 && mbr[511] == 0xAA;
    let mut partitions = Vec::new();
    if mbr_valid {
        parse_mbr_partitions(&mbr, &mut partitions);
    }

    let gpt_valid = parse_gpt_partitions(&*dev, &mut partitions).unwrap_or(false);

    let mut probes = Vec::new();
    for partition in &partitions {
        if partition.start_lba == 0 && partition.table != PartitionTableKind::Fallback {
            continue;
        }
        probes.push(probe_ext4(
            &*dev,
            Some(partition.index),
            partition.start_lba,
        ));
    }

    let root = *ROOT_MOUNT.lock();
    StorageDiagnostic {
        disk_detected: true,
        device: Some(device),
        mbr_valid,
        gpt_valid,
        partitions,
        probes,
        root_mount_success: root.attempted && root.success,
        root_mount_failure: root.failure,
    }
}

pub fn classify_root_filesystem(diagnostic: &StorageDiagnostic) -> RootFilesystemState {
    if !diagnostic.disk_detected {
        return RootFilesystemState::NoDisk;
    }
    let partition_table_present = diagnostic.mbr_valid || diagnostic.gpt_valid;
    if !partition_table_present {
        return RootFilesystemState::PartitionTableMissing;
    }
    if diagnostic.partitions.is_empty() {
        return RootFilesystemState::PartitionTablePresent;
    }
    if diagnostic.root_mount_success {
        return RootFilesystemState::RootMounted;
    }
    let mut saw_root_candidate = false;
    let mut saw_corrupt_candidate = false;
    for partition in &diagnostic.partitions {
        if !is_root_filesystem_candidate(*partition) {
            continue;
        }
        saw_root_candidate = true;
        for probe in diagnostic
            .probes
            .iter()
            .filter(|probe| probe.partition_index == Some(partition.index))
        {
            if probe.actual_magic == probe.expected_magic {
                return RootFilesystemState::FilesystemValid;
            }
            if probe.read_ok && probe.actual_magic != 0 {
                saw_corrupt_candidate = true;
            }
        }
    }
    if !saw_root_candidate {
        return RootFilesystemState::FilesystemMissing;
    }
    if saw_corrupt_candidate {
        RootFilesystemState::FilesystemCorrupt
    } else {
        RootFilesystemState::FilesystemMissing
    }
}

fn is_root_filesystem_candidate(partition: PartitionInfo) -> bool {
    match partition.table {
        PartitionTableKind::Mbr => partition.type_code != 0 && partition.type_code != 0xEF,
        PartitionTableKind::Gpt => true,
        PartitionTableKind::Fallback => false,
    }
}

pub fn validate_storage() -> StorageValidationReport {
    let diagnostic = diagnose();
    StorageValidationReport {
        disk_detected: diagnostic.disk_detected,
        partition_table_detected: diagnostic.mbr_valid || diagnostic.gpt_valid,
        partition_discovered: !diagnostic.partitions.is_empty(),
        filesystem_probe: diagnostic.probes.iter().any(|probe| probe.read_ok),
        root_mount: diagnostic.root_mount_success,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StorageValidationReport {
    pub disk_detected: bool,
    pub partition_table_detected: bool,
    pub partition_discovered: bool,
    pub filesystem_probe: bool,
    pub root_mount: bool,
}

fn parse_mbr_partitions(mbr: &[u8; 512], partitions: &mut Vec<PartitionInfo>) {
    for i in 0..4usize {
        let entry = 0x1BE + i * 16;
        let type_code = mbr[entry + 4];
        let start_lba = u32::from_le_bytes([
            mbr[entry + 8],
            mbr[entry + 9],
            mbr[entry + 10],
            mbr[entry + 11],
        ]) as u64;
        let size_lba = u32::from_le_bytes([
            mbr[entry + 12],
            mbr[entry + 13],
            mbr[entry + 14],
            mbr[entry + 15],
        ]) as u64;
        if type_code != 0 && start_lba != 0 && size_lba != 0 {
            partitions.push(PartitionInfo {
                index: partitions.len() + 1,
                table: PartitionTableKind::Mbr,
                type_code,
                start_lba,
                size_lba,
            });
        }
    }
}

fn parse_gpt_partitions(
    dev: &dyn BlockDevice,
    partitions: &mut Vec<PartitionInfo>,
) -> Result<bool, &'static str> {
    let sector_size = dev.sector_size() as u64;
    let mut header = [0u8; 512];
    dev.read_bytes(sector_size, &mut header)?;
    if &header[0..8] != b"EFI PART" {
        return Ok(false);
    }

    let entries_lba = u64::from_le_bytes([
        header[72], header[73], header[74], header[75], header[76], header[77], header[78],
        header[79],
    ]);
    let entry_count = u32::from_le_bytes([header[80], header[81], header[82], header[83]]) as usize;
    let entry_size = u32::from_le_bytes([header[84], header[85], header[86], header[87]]) as usize;
    if entries_lba == 0 || entry_size < 56 || entry_count == 0 {
        return Ok(true);
    }

    let scan_count = entry_count.min(32);
    let mut entries = alloc::vec![0u8; scan_count * entry_size];
    dev.read_bytes(entries_lba.saturating_mul(sector_size), &mut entries)?;
    for i in 0..scan_count {
        let base = i * entry_size;
        if entries[base..base + 16].iter().all(|byte| *byte == 0) {
            continue;
        }
        let first_lba = u64::from_le_bytes([
            entries[base + 32],
            entries[base + 33],
            entries[base + 34],
            entries[base + 35],
            entries[base + 36],
            entries[base + 37],
            entries[base + 38],
            entries[base + 39],
        ]);
        let last_lba = u64::from_le_bytes([
            entries[base + 40],
            entries[base + 41],
            entries[base + 42],
            entries[base + 43],
            entries[base + 44],
            entries[base + 45],
            entries[base + 46],
            entries[base + 47],
        ]);
        if first_lba == 0 || last_lba < first_lba {
            continue;
        }
        partitions.push(PartitionInfo {
            index: partitions.len() + 1,
            table: PartitionTableKind::Gpt,
            type_code: 0,
            start_lba: first_lba,
            size_lba: last_lba - first_lba + 1,
        });
    }
    Ok(true)
}

fn push_unique_probe(
    dev: &dyn BlockDevice,
    probes: &mut Vec<Ext4ProbeInfo>,
    partition_index: Option<usize>,
    start_lba: u64,
) {
    if probes
        .iter()
        .any(|probe| probe.probe_target_lba == start_lba)
    {
        return;
    }
    probes.push(probe_ext4(dev, partition_index, start_lba));
}

fn probe_ext4(
    dev: &dyn BlockDevice,
    partition_index: Option<usize>,
    start_lba: u64,
) -> Ext4ProbeInfo {
    let sector_size = dev.sector_size() as u64;
    let superblock_offset = start_lba.saturating_mul(sector_size).saturating_add(1024);
    let superblock_lba = superblock_offset / sector_size;
    let mut magic = [0u8; 2];
    let read_ok = dev.read_bytes(superblock_offset + 0x38, &mut magic).is_ok();
    let actual_magic = if read_ok {
        u16::from_le_bytes(magic)
    } else {
        0
    };
    let result = if !read_ok {
        "superblock read failed"
    } else if actual_magic == EXT4_MAGIC {
        "valid ext4 superblock"
    } else {
        "invalid superblock"
    };
    Ext4ProbeInfo {
        partition_index,
        probe_target_lba: start_lba,
        superblock_lba,
        superblock_offset,
        expected_magic: EXT4_MAGIC,
        actual_magic,
        read_ok,
        result,
    }
}
