//! Storage driver and volume registry.
//!
//! This module wires the existing PCI enumeration, driver manager, device
//! manager and VFS mount flow into a concrete storage path:
//! PCI controller -> block device -> partitions -> FAT32 volume operations.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::min;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::driver::ahci;
use crate::kernel::device::{self, DeviceStatus};
use crate::pci;

const FAT_STORE_MAGIC: &[u8; 8] = b"SAFAT32\0";
const EXT4_STORE_MAGIC: &[u8; 8] = b"SAEXT4\0\0";
const FAT_STORE_VERSION: u32 = 1;
const PARTITION_PROBE_WINDOW_BYTES: usize = 4096;
const MOUNT_TREE_READ_WINDOW_BYTES: usize = 256 * 1024;
const MOUNT_TREE_WRITE_WINDOW_BYTES: usize = MOUNT_TREE_READ_WINDOW_BYTES;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FilesystemKind {
    TmpFs,
    Ext4,
    Ntfs,
    Fat16,
    Fat32,
    Fat64,
    Fat128,
}

impl FilesystemKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            FilesystemKind::TmpFs => "tmpfs",
            FilesystemKind::Ext4 => "ext4",
            FilesystemKind::Ntfs => "ntfs",
            FilesystemKind::Fat16 => "fat16",
            FilesystemKind::Fat32 => "fat32",
            FilesystemKind::Fat64 => "fat64",
            FilesystemKind::Fat128 => "fat128",
        }
    }

    pub const fn driver_name(self) -> &'static str {
        match self {
            FilesystemKind::TmpFs => "storage",
            FilesystemKind::Ext4 => "ext4",
            FilesystemKind::Ntfs => "ntfs",
            FilesystemKind::Fat16 => "fat16",
            FilesystemKind::Fat32 => "fat32",
            FilesystemKind::Fat64 => "fat64",
            FilesystemKind::Fat128 => "fat128",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        let lowered = s.trim().to_ascii_lowercase();
        let canonical: String = lowered
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();

        match canonical.as_str() {
            "tmpfs" => Some(Self::TmpFs),
            "ext4" => Some(Self::Ext4),
            "ext" => Some(Self::Ext4),
            "ext2" => Some(Self::Ext4),
            "ext3" => Some(Self::Ext4),
            "ntfs" => Some(Self::Ntfs),
            "fat16" => Some(Self::Fat16),
            "fat32" => Some(Self::Fat32),
            "fat" => Some(Self::Fat32),
            "vfat" => Some(Self::Fat32),
            "fat64" => Some(Self::Fat64),
            "fat128" => Some(Self::Fat128),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DetectedVolume {
    pub name: String,
    pub filesystem: FilesystemKind,
    pub backing: String,
    pub total_bytes: u64,
    pub sector_size: u16,
    pub mounted_at: Option<String>,
    pub writable: bool,
}

#[derive(Clone, Debug)]
pub struct DetectedDisk {
    pub name: String,
    pub backing: String,
    pub total_bytes: u64,
    pub sector_size: u16,
    pub hardware: bool,
    pub partitions: Vec<String>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FsNodeKind {
    File,
    Directory,
}

#[derive(Debug, Clone)]
pub struct FsStat {
    pub kind: FsNodeKind,
    pub size: usize,
}

#[derive(Clone)]
struct FsNode {
    path: String,
    kind: FsNodeKind,
    data: Vec<u8>,
}

#[derive(Clone)]
struct Partition {
    name: String,
    start_lba: u64,
    sector_count: u64,
    fs_hint: FilesystemKind,
}

/// Block device backing for real AHCI hardware.
#[derive(Clone)]
enum BlockBacking {
    Ahci { disk_id: u32, sector_size: u16, sectors: u64 },
}

impl BlockBacking {
    fn sector_size(&self) -> u16 {
        match self {
            BlockBacking::Ahci { sector_size, .. } => *sector_size,
        }
    }

    fn sectors(&self) -> u64 {
        match self {
            BlockBacking::Ahci { sectors, .. } => *sectors,
        }
    }

    fn read_sector(&self, lba: u64, out: &mut [u8]) -> Result<(), &'static str> {
        match self {
            BlockBacking::Ahci { disk_id, .. } => ahci::read_sector(*disk_id, lba, out),
        }
    }

    fn write_sector(&mut self, lba: u64, data: &[u8]) -> Result<(), &'static str> {
        match self {
            BlockBacking::Ahci { disk_id, .. } => ahci::write_sector(*disk_id, lba, data),
        }
    }

    fn flush(&mut self) {
        match self {
            BlockBacking::Ahci { disk_id, .. } => { let _ = ahci::flush(*disk_id); }
        }
    }

    fn is_real_hardware(&self) -> bool {
        matches!(self, BlockBacking::Ahci { .. })
    }
}

#[derive(Clone)]
struct DiskDevice {
    name: String,
    backing: String,
    block: BlockBacking,
    partitions: Vec<Partition>,
}

#[derive(Clone)]
struct MountedFs {
    volume: String,
    nodes: Vec<FsNode>,
}

#[derive(Clone)]
struct StorageState {
    initialized: bool,
    volumes: Vec<DetectedVolume>,
    disks: Vec<DiskDevice>,
    mounted: Vec<MountedFs>,
    diagnostics: Vec<String>,
}

impl StorageState {
    fn new() -> Self {
        Self {
            initialized: false,
            volumes: Vec::new(),
            disks: Vec::new(),
            mounted: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

static STATE: StaticCell<Option<StorageState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);
static SCAN_REQUESTED: AtomicBool = AtomicBool::new(false);
static SCAN_RUNNING: AtomicBool = AtomicBool::new(false);
static SCAN_COMPLETED: AtomicBool = AtomicBool::new(false);
static SCAN_EPOCH: AtomicU64 = AtomicU64::new(0);
static SCAN_PHASE: AtomicU8 = AtomicU8::new(SCAN_IDLE);

const SCAN_IDLE: u8 = 0;
const SCAN_QUEUED: u8 = 1;
const SCAN_PCI: u8 = 2;
const SCAN_AHCI: u8 = 3;
const SCAN_PARTITIONS: u8 = 4;
const SCAN_PUBLISH: u8 = 5;
const SCAN_DONE: u8 = 6;
const SCAN_FAILED: u8 = 7;

#[derive(Clone, Debug)]
pub struct StorageScanStatus {
    pub queued: bool,
    pub running: bool,
    pub completed: bool,
    pub epoch: u64,
    pub phase: &'static str,
    pub disks: usize,
    pub volumes: usize,
    pub failures: usize,
}

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn scan_phase_name(phase: u8) -> &'static str {
    match phase {
        SCAN_IDLE => "idle",
        SCAN_QUEUED => "queued",
        SCAN_PCI => "pci",
        SCAN_AHCI => "ahci",
        SCAN_PARTITIONS => "partitions",
        SCAN_PUBLISH => "publish",
        SCAN_DONE => "done",
        SCAN_FAILED => "failed",
        _ => "unknown",
    }
}

fn with_state_mut<R>(f: impl FnOnce(&mut StorageState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(StorageState::new());
            }
            slot.as_mut().expect("storage state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&StorageState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(StorageState::new());
            }
            slot.as_ref().expect("storage state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn le_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}

fn le_u64(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
        *bytes.get(at + 4)?,
        *bytes.get(at + 5)?,
        *bytes.get(at + 6)?,
        *bytes.get(at + 7)?,
    ]))
}

fn has_mbr_signature(image: &[u8]) -> bool {
    matches!((image.get(510), image.get(511)), (Some(0x55), Some(0xAA)))
}

fn parse_ascii(bytes: &[u8], at: usize, len: usize) -> Option<&str> {
    let slice = bytes.get(at..at + len)?;
    core::str::from_utf8(slice).ok()
}

#[derive(Debug, Copy, Clone)]
struct ProbeResult {
    fs: FilesystemKind,
}

fn probe_ext4(image: &[u8]) -> Option<ProbeResult> {
    if image.len() < 2048 {
        return None;
    }
    let superblock = 1024usize;
    let magic = le_u16(image, superblock + 56)?;
    if magic != 0xEF53 {
        return None;
    }
    let _ = le_u32(image, superblock + 24)?;
    let _ = le_u32(image, superblock + 4)?;
    Some(ProbeResult {
        fs: FilesystemKind::Ext4,
    })
}

fn probe_ntfs(image: &[u8]) -> Option<ProbeResult> {
    if image.len() < 512 || !has_mbr_signature(image) {
        return None;
    }
    let oem = parse_ascii(image, 3, 8)?;
    if oem != "NTFS    " {
        return None;
    }
    let _ = le_u16(image, 11)?;
    let _ = le_u64(image, 40)?;
    Some(ProbeResult {
        fs: FilesystemKind::Ntfs,
    })
}

fn probe_fat(image: &[u8]) -> Option<ProbeResult> {
    if image.len() < 512 || !has_mbr_signature(image) {
        return None;
    }

    let sector_size = le_u16(image, 11)?;
    if sector_size == 0 {
        return None;
    }

    let sectors_16 = le_u16(image, 19)? as u32;
    let sectors_32 = le_u32(image, 32)?;
    let total_sectors = if sectors_16 != 0 {
        sectors_16
    } else {
        sectors_32
    };
    if total_sectors == 0 {
        return None;
    }

    let fs16 = parse_ascii(image, 54, 8).unwrap_or("        ").trim();
    let fs32 = parse_ascii(image, 82, 8).unwrap_or("        ").trim();
    let fs = if fs16.eq_ignore_ascii_case("FAT16") {
        FilesystemKind::Fat16
    } else if fs32.eq_ignore_ascii_case("FAT32") {
        FilesystemKind::Fat32
    } else if fs32.eq_ignore_ascii_case("FAT64") || fs16.eq_ignore_ascii_case("FAT64") {
        FilesystemKind::Fat64
    } else if fs32.eq_ignore_ascii_case("FAT128") || fs16.eq_ignore_ascii_case("FAT128") {
        FilesystemKind::Fat128
    } else if sectors_16 != 0 {
        FilesystemKind::Fat16
    } else {
        FilesystemKind::Fat32
    };

    Some(ProbeResult { fs })
}

fn probe_managed_store(image: &[u8]) -> Option<ProbeResult> {
    let magic = image.get(0..8)?;
    let version = le_u32(image, 8)?;
    if version != FAT_STORE_VERSION {
        return None;
    }

    if magic == FAT_STORE_MAGIC {
        return Some(ProbeResult {
            fs: FilesystemKind::Fat32,
        });
    }

    if magic == EXT4_STORE_MAGIC {
        return Some(ProbeResult {
            fs: FilesystemKind::Ext4,
        });
    }

    None
}

fn probe_filesystem(image: &[u8]) -> Option<ProbeResult> {
    probe_managed_store(image)
        .or_else(|| probe_ext4(image))
        .or_else(|| probe_ntfs(image))
        .or_else(|| probe_fat(image))
}

fn normalize_path(path: &str) -> String {
    if path.is_empty() || path == "/" {
        return "/".to_string();
    }

    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            let _ = out.pop();
            continue;
        }
        out.push(seg);
    }

    if out.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", out.join("/"))
    }
}

fn is_child_of(parent: &str, path: &str) -> bool {
    if parent == "/" {
        return path.starts_with('/');
    }
    path.starts_with(parent)
        && (path.len() == parent.len() || path.as_bytes().get(parent.len()) == Some(&b'/'))
}

fn split_parent(path: &str) -> Option<(String, String)> {
    let p = normalize_path(path);
    if p == "/" {
        return None;
    }
    let mut parts: Vec<&str> = p.split('/').filter(|v| !v.is_empty()).collect();
    let name = parts.pop()?.to_string();
    let parent = if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    };
    Some((parent, name))
}

fn default_fat_tree() -> Vec<FsNode> {
    vec![
        FsNode {
            path: "/".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
    ]
}

fn default_ro_tree(_fs: FilesystemKind) -> Vec<FsNode> {
    vec![FsNode {
        path: "/".to_string(),
        kind: FsNodeKind::Directory,
        data: Vec::new(),
    }]
}

fn default_rw_tree(fs: FilesystemKind) -> Vec<FsNode> {
    if fs == FilesystemKind::Fat32 {
        return default_fat_tree();
    }

    let nodes = vec![
        FsNode {
            path: "/".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
    ];

    nodes
}

fn is_legacy_scaffold_dir(path: &str) -> bool {
    matches!(path, "/boot" | "/dev" | "/etc" | "/home" | "/mnt" | "/proc" | "/sys" | "/tmp")
}

fn prune_legacy_scaffold_dirs(nodes: &mut Vec<FsNode>) {
    let mut i = 0usize;
    while i < nodes.len() {
        let path = nodes[i].path.clone();
        let removable = nodes[i].kind == FsNodeKind::Directory
            && is_legacy_scaffold_dir(path.as_str())
            && nodes[i].data.is_empty()
            && !nodes
                .iter()
                .any(|n| n.path != path && is_child_of(path.as_str(), n.path.as_str()));

        if removable {
            nodes.remove(i);
            continue;
        }

        i += 1;
    }
}

fn is_legacy_ext4_stub_tree(nodes: &[FsNode]) -> bool {
    nodes.iter().all(|n| {
        n.path == "/" || is_legacy_scaffold_dir(n.path.as_str()) || n.path == "/lost+found"
    })
}

fn fs_supports_rw_tree(fs: FilesystemKind) -> bool {
    fs == FilesystemKind::Fat32 || fs == FilesystemKind::Ext4
}

fn fs_supports_mount_tree(fs: FilesystemKind) -> bool {
    fs == FilesystemKind::Fat32 || fs == FilesystemKind::Ext4
}

fn find_node<'a>(nodes: &'a [FsNode], path: &str) -> Option<&'a FsNode> {
    let key = normalize_path(path);
    nodes.iter().find(|n| n.path == key)
}

fn find_node_mut<'a>(nodes: &'a mut [FsNode], path: &str) -> Option<&'a mut FsNode> {
    let key = normalize_path(path);
    nodes.iter_mut().find(|n| n.path == key)
}

fn ensure_parent_dir(nodes: &[FsNode], path: &str) -> Result<(), &'static str> {
    let (parent, _) = split_parent(path).ok_or("storage: invalid path")?;
    let Some(node) = find_node(nodes, &parent) else {
        return Err("storage: parent path not found");
    };
    if node.kind != FsNodeKind::Directory {
        return Err("storage: parent is not a directory");
    }
    Ok(())
}

fn serialize_tree(nodes: &[FsNode], fs: FilesystemKind) -> Vec<u8> {
    let mut out = Vec::new();
    let magic = if fs == FilesystemKind::Ext4 {
        EXT4_STORE_MAGIC
    } else {
        FAT_STORE_MAGIC
    };
    out.extend_from_slice(magic);
    out.extend_from_slice(&FAT_STORE_VERSION.to_le_bytes());
    out.extend_from_slice(&(nodes.len() as u32).to_le_bytes());

    for node in nodes {
        out.push(match node.kind {
            FsNodeKind::File => 1,
            FsNodeKind::Directory => 2,
        });
        let p = node.path.as_bytes();
        out.extend_from_slice(&(p.len() as u16).to_le_bytes());
        out.extend_from_slice(&(node.data.len() as u32).to_le_bytes());
        out.extend_from_slice(p);
        out.extend_from_slice(node.data.as_slice());
    }

    out
}

fn deserialize_tree(bytes: &[u8]) -> Option<Vec<FsNode>> {
    if bytes.len() < 16 {
        return None;
    }
    let magic = bytes.get(0..8)?;
    if magic != FAT_STORE_MAGIC && magic != EXT4_STORE_MAGIC {
        return None;
    }

    let version = le_u32(bytes, 8)?;
    if version != FAT_STORE_VERSION {
        return None;
    }

    let count = le_u32(bytes, 12)? as usize;
    let mut at = 16usize;
    let mut nodes = Vec::new();

    for _ in 0..count {
        let kind = match *bytes.get(at)? {
            1 => FsNodeKind::File,
            2 => FsNodeKind::Directory,
            _ => return None,
        };
        at += 1;

        let path_len = le_u16(bytes, at)? as usize;
        at += 2;
        let data_len = le_u32(bytes, at)? as usize;
        at += 4;

        let p_end = at.checked_add(path_len)?;
        let path = core::str::from_utf8(bytes.get(at..p_end)?)
            .ok()?
            .to_string();
        at = p_end;

        let d_end = at.checked_add(data_len)?;
        let data = bytes.get(at..d_end)?.to_vec();
        at = d_end;

        nodes.push(FsNode { path, kind, data });
    }

    Some(nodes)
}

fn write_partition_bytes(
    disk: &mut DiskDevice,
    part: &Partition,
    bytes: &[u8],
) -> Result<(), &'static str> {
    let sector_size = disk.block.sector_size() as usize;
    let mut lba = part.start_lba;
    let mut at = 0usize;
    let mut scratch = vec![0u8; sector_size];
    let window_sectors = MOUNT_TREE_WRITE_WINDOW_BYTES
        .div_ceil(sector_size)
        .max(1) as u64;
    let max_lba = part
        .start_lba
        .saturating_add(core::cmp::min(part.sector_count, window_sectors));

    while lba < max_lba {
        if at >= bytes.len() {
            scratch.fill(0);
        } else {
            scratch.fill(0);
            let n = min(sector_size, bytes.len() - at);
            scratch[..n].copy_from_slice(&bytes[at..at + n]);
            at += n;
        }
        disk.block.write_sector(lba, scratch.as_slice())?;
        lba = lba.saturating_add(1);
    }

    if at < bytes.len() {
        return Err("storage: out of space");
    }

    Ok(())
}

fn read_partition_bytes(disk: &DiskDevice, part: &Partition) -> Result<Vec<u8>, &'static str> {
    let sector_size = disk.block.sector_size() as usize;
    let mut lba = part.start_lba;
    let window_sectors = MOUNT_TREE_READ_WINDOW_BYTES
        .div_ceil(sector_size)
        .max(1) as u64;
    let max_lba = part
        .start_lba
        .saturating_add(core::cmp::min(part.sector_count, window_sectors));
    let mut scratch = vec![0u8; sector_size];
    let mut out = Vec::new();

    while lba < max_lba {
        disk.block.read_sector(lba, scratch.as_mut_slice())?;
        out.extend_from_slice(scratch.as_slice());
        lba = lba.saturating_add(1);
    }

    Ok(out)
}

fn save_mounted_volume(state: &mut StorageState, volume: &str) -> Result<(), &'static str> {
    let mounted = state
        .mounted
        .iter()
        .find(|m| m.volume.eq_ignore_ascii_case(volume))
        .ok_or("storage: mounted volume not found")?
        .clone();

    let (disk_name, part_name) = resolve_volume_owner(state, volume)?;
    let disk = state
        .disks
        .iter_mut()
        .find(|d| d.name.eq_ignore_ascii_case(disk_name.as_str()))
        .ok_or("storage: disk missing")?;
    let part = disk
        .partitions
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(part_name.as_str()))
        .ok_or("storage: partition missing")?
        .clone();

    let fs_kind = state
        .volumes
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(volume))
        .map(|v| v.filesystem)
        .unwrap_or(FilesystemKind::Fat32);

    let bytes = serialize_tree(mounted.nodes.as_slice(), fs_kind);
    write_partition_bytes(disk, &part, bytes.as_slice())
}

fn resolve_volume_owner(
    state: &StorageState,
    volume: &str,
) -> Result<(String, String), &'static str> {
    for disk in &state.disks {
        for part in &disk.partitions {
            if part.name.eq_ignore_ascii_case(volume) {
                return Ok((disk.name.clone(), part.name.clone()));
            }
        }
    }
    Err("storage: volume backend unavailable")
}

fn load_volume_tree(
    state: &StorageState,
    volume: &str,
    fs: FilesystemKind,
) -> Result<Vec<FsNode>, &'static str> {
    if fs != FilesystemKind::Fat32 && fs != FilesystemKind::Ext4 {
        return Err("storage: native filesystem reader not implemented for this volume");
    }

    let (disk_name, part_name) = resolve_volume_owner(state, volume)?;
    let disk = state
        .disks
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(disk_name.as_str()))
        .ok_or("storage: disk missing")?;
    let part = disk
        .partitions
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(part_name.as_str()))
        .ok_or("storage: partition missing")?;

    let bytes = read_partition_bytes(disk, part)?;
    if let Some(mut nodes) = deserialize_tree(bytes.as_slice()) {
        // Backward compatibility: old builds seeded scaffold directories into
        // managed volumes. Remove those empty placeholders on load.
        prune_legacy_scaffold_dirs(&mut nodes);

        // If this looks like an old managed ext4 scaffold and the partition
        // has a valid native ext4 superblock, prefer native ext4 traversal so
        // real Linux files are visible.
        if fs == FilesystemKind::Ext4
            && is_legacy_ext4_stub_tree(nodes.as_slice())
            && ext4_load_superblock(disk, part).is_ok()
        {
            return Err("storage: ext4 volume has native filesystem; using native reader");
        }

        return Ok(nodes);
    }

    if fs == FilesystemKind::Ext4 {
        return Err("storage: ext4 volume is native read-only; format it to enable managed read/write");
    }

    Ok(default_rw_tree(fs))
}

fn probe_partition_filesystem(disk: &DiskDevice, part: &Partition) -> Option<FilesystemKind> {
    if part.sector_count == 0 {
        return None;
    }

    let sector_size = disk.block.sector_size() as usize;
    if sector_size == 0 {
        return None;
    }

    let sectors_to_read = PARTITION_PROBE_WINDOW_BYTES
        .div_ceil(sector_size)
        .min(part.sector_count as usize);
    if sectors_to_read == 0 {
        return None;
    }

    let mut image = vec![0u8; sectors_to_read.saturating_mul(sector_size)];
    let mut scratch = vec![0u8; sector_size];
    for i in 0..sectors_to_read {
        let lba = part.start_lba.saturating_add(i as u64);
        disk.block.read_sector(lba, scratch.as_mut_slice()).ok()?;
        let at = i.saturating_mul(sector_size);
        image[at..at + sector_size].copy_from_slice(scratch.as_slice());
    }

    probe_filesystem(image.as_slice()).map(|p| p.fs)
}

fn parse_mbr_partitions(disk: &DiskDevice) -> Vec<Partition> {
    let mut mbr = vec![0u8; disk.block.sector_size() as usize];
    if disk.block.read_sector(0, mbr.as_mut_slice()).is_err() || !has_mbr_signature(&mbr) {
        return Vec::new();
    }

    let mut out = Vec::new();
    for i in 0..4usize {
        let off = 446 + i * 16;
        let ptype = mbr[off + 4];
        let start = le_u32(&mbr, off + 8).unwrap_or(0) as u64;
        let count = le_u32(&mbr, off + 12).unwrap_or(0) as u64;
        if ptype == 0 || start == 0 || count == 0 {
            continue;
        }

        let fs_hint = match ptype {
            0x0B | 0x0C | 0x0E => FilesystemKind::Fat32,
            0x07 => FilesystemKind::Ntfs,
            0x83 => FilesystemKind::Ext4,
            _ => FilesystemKind::TmpFs,
        };

        out.push(Partition {
            name: String::new(),
            start_lba: start,
            sector_count: count,
            fs_hint,
        });
    }

    out
}

fn parse_gpt_partitions(disk: &DiskDevice) -> Vec<Partition> {
    let mut sector = vec![0u8; disk.block.sector_size() as usize];
    if disk.block.read_sector(1, sector.as_mut_slice()).is_err() {
        return Vec::new();
    }
    if sector.get(0..8) != Some(b"EFI PART") {
        return Vec::new();
    }

    let entries_lba = le_u64(&sector, 72).unwrap_or(0);
    let entries_count = le_u32(&sector, 80).unwrap_or(0) as usize;
    let entry_size = le_u32(&sector, 84).unwrap_or(0) as usize;
    if entries_lba == 0 || entries_count == 0 || entry_size < 128 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let table_bytes = entries_count.saturating_mul(entry_size);
    let sec_size = disk.block.sector_size() as usize;
    let table_secs = table_bytes.div_ceil(sec_size);
    let mut table = vec![0u8; table_secs.saturating_mul(sec_size)];
    let mut scratch = vec![0u8; sec_size];

    for i in 0..table_secs {
        let lba = entries_lba.saturating_add(i as u64);
        if disk.block.read_sector(lba, scratch.as_mut_slice()).is_err() {
            return Vec::new();
        }
        let at = i.saturating_mul(sec_size);
        table[at..at + sec_size].copy_from_slice(scratch.as_slice());
    }

    for i in 0..entries_count {
        let off = i.saturating_mul(entry_size);
        if off.saturating_add(56) > table.len() {
            break;
        }
        let part_type_zero = table[off..off + 16].iter().all(|b| *b == 0);
        if part_type_zero {
            continue;
        }

        let first_lba = le_u64(&table, off + 32).unwrap_or(0);
        let last_lba = le_u64(&table, off + 40).unwrap_or(0);
        if first_lba == 0 || last_lba < first_lba {
            continue;
        }

        out.push(Partition {
            name: String::new(),
            start_lba: first_lba,
            sector_count: last_lba.saturating_sub(first_lba).saturating_add(1),
            fs_hint: FilesystemKind::Fat32,
        });
    }

    out
}

fn detect_partitions_for_disk(disk: &mut DiskDevice) {
    let mut parts = parse_gpt_partitions(disk);
    if parts.is_empty() {
        parts = parse_mbr_partitions(disk);
    }
    if parts.is_empty() {
        parts.push(Partition {
            name: String::new(),
            start_lba: 2048,
            sector_count: disk.block.sectors().saturating_sub(2048),
            fs_hint: FilesystemKind::Fat32,
        });
    }

    for (i, part) in parts.iter_mut().enumerate() {
        part.name = format!("{}p{}", disk.name, i + 1);
        if let Some(probed) = probe_partition_filesystem(disk, part) {
            part.fs_hint = probed;
        }
    }

    disk.partitions = parts;
}

fn register_devices(state: &StorageState) -> Vec<String> {
    let mut diagnostics = Vec::new();
    for disk in &state.disks {
        if let Err(err) = device::ensure_device(
            format!("/dev/{}", disk.name).as_str(),
            "storage",
            "block/disk",
            DeviceStatus::Online,
        ) {
            diagnostics.push(format!(
                "stage=registration target=/dev/{} detail={}"
                ,
                disk.name,
                err
            ));
        }

        for part in &disk.partitions {
            if let Err(err) = device::ensure_device(
                format!("/dev/{}", part.name).as_str(),
                "storage",
                "block/partition",
                DeviceStatus::Online,
            ) {
                diagnostics.push(format!(
                    "stage=registration target=/dev/{} detail={}"
                    ,
                    part.name,
                    err
                ));
            }
        }
    }
    diagnostics
}

fn rebuild_volume_registry(state: &mut StorageState) {
    state.volumes.clear();
    state.volumes.push(DetectedVolume {
        name: "tmpfs".to_string(),
        filesystem: FilesystemKind::TmpFs,
        backing: "memory".to_string(),
        total_bytes: 0,
        sector_size: 4096,
        mounted_at: Some("/".to_string()),
        writable: true,
    });

    for disk in &state.disks {
        let kind_str = if disk.block.is_real_hardware() { "ahci" } else { "ram" };
        state.volumes.push(DetectedVolume {
            name: disk.name.clone(),
            filesystem: FilesystemKind::TmpFs,
            backing: format!("{}:{}", kind_str, disk.backing),
            total_bytes: disk
                .block
                .sectors()
                .saturating_mul(disk.block.sector_size() as u64),
            sector_size: disk.block.sector_size(),
            mounted_at: None,
            writable: true,
        });

        for part in &disk.partitions {
            state.volumes.push(DetectedVolume {
                name: part.name.clone(),
                filesystem: part.fs_hint,
                backing: format!("{}:{}", disk.name, part.name),
                total_bytes: part
                    .sector_count
                    .saturating_mul(disk.block.sector_size() as u64),
                sector_size: disk.block.sector_size(),
                mounted_at: None,
                writable: true,
            });
        }
    }
}

/// Discover disks - first from AHCI driver (real hardware), then PCI fallback.
fn discover_disks_from_pci(diagnostics: &mut Vec<String>) -> Vec<DiskDevice> {
    let mut disks = Vec::new();

    // First: Query AHCI driver for real disks (uses cached list, won't stall)
    let ahci_disks = ahci::disks_cached();
    for (idx, adisk) in ahci_disks.iter().enumerate() {
        if adisk.total_sectors == 0 || adisk.sector_size == 0 {
            diagnostics.push(format!(
                "stage=controller_init target={} detail=skip invalid geometry sectors={} sector_size={}",
                adisk.name, adisk.total_sectors, adisk.sector_size
            ));
            continue;
        }
        let mut disk = DiskDevice {
            name: format!("sata{}", idx),
            backing: format!(
                "ahci {} port {} model \"{}\"",
                adisk.controller, adisk.port, adisk.model
            ),
            block: BlockBacking::Ahci {
                disk_id: adisk.id,
                sector_size: adisk.sector_size,
                sectors: adisk.total_sectors,
            },
            partitions: Vec::new(),
        };
        detect_partitions_for_disk(&mut disk);
        disks.push(disk);
    }

    // Report unsupported controllers but do not fabricate synthetic disks.
    for dev in pci::devices() {
        if dev.class != 0x01 {
            continue;
        }
        // Skip AHCI controllers - already handled above.
        if dev.subclass == 0x06 && dev.prog_if == 0x01 {
            continue;
        }
        let detail = if dev.subclass == 0x08 && dev.prog_if == 0x02 {
            "NVMe controller detected; NVMe driver not implemented yet"
        } else if dev.subclass == 0x04 {
            "RAID controller detected; AHCI mode may be required in firmware"
        } else if dev.subclass == 0x01 {
            "IDE controller detected; legacy IDE path not implemented"
        } else {
            "storage controller class detected but no matching kernel driver"
        };
        diagnostics.push(format!(
            "stage=pci_detection controller={:02x}:{:02x}.{} vendor={:04x} device={:04x} subclass={:02x} prog_if={:02x} detail={}",
            dev.bus,
            dev.device,
            dev.function,
            dev.vendor_id,
            dev.device_id,
            dev.subclass,
            dev.prog_if,
            detail
        ));
    }

    if disks.is_empty() {
        diagnostics.push("stage=controller_init detail=0 disks detected".to_string());
    }

    disks
}

fn ensure_volume_mounted(
    state: &mut StorageState,
    volume: &str,
    fs: FilesystemKind,
) -> Result<(), &'static str> {
    if state
        .mounted
        .iter()
        .any(|m| m.volume.eq_ignore_ascii_case(volume))
    {
        return Ok(());
    }

    if !fs_supports_mount_tree(fs) {
        return Err("storage: filesystem backend not implemented");
    }

    let nodes = if fs_supports_rw_tree(fs) {
        match load_volume_tree(state, volume, fs) {
            Ok(nodes) => nodes,
            Err(_) if fs == FilesystemKind::Ext4 => return Ok(()),
            Err(e) => return Err(e),
        }
    } else {
        default_ro_tree(fs)
    };
    state.mounted.push(MountedFs {
        volume: volume.to_string(),
        nodes,
    });
    Ok(())
}

fn mounted_volume_for_path_internal<'a>(
    state: &'a StorageState,
    abs_path: &str,
) -> Option<(&'a DetectedVolume, String)> {
    let path = normalize_path(abs_path);
    let mut best: Option<&DetectedVolume> = None;
    let mut best_len = 0usize;

    for vol in &state.volumes {
        let Some(mount) = vol.mounted_at.as_ref() else {
            continue;
        };
        let m = normalize_path(mount);
        if !is_child_of(m.as_str(), path.as_str()) {
            continue;
        }
        if m.len() >= best_len {
            best = Some(vol);
            best_len = m.len();
        }
    }

    let vol = best?;
    let mount = normalize_path(vol.mounted_at.as_ref()?);
    let rel = if mount == "/" {
        path.clone()
    } else if path == mount {
        "/".to_string()
    } else {
        path[mount.len()..].to_string()
    };

    Some((vol, normalize_path(rel.as_str())))
}

#[derive(Copy, Clone)]
struct Ext4Superblock {
    block_size: u64,
    inodes_per_group: u32,
    inode_size: u16,
    desc_size: u16,
}

#[derive(Copy, Clone)]
struct Ext4Inode {
    mode: u16,
    size: u64,
    flags: u32,
    block: [u8; 60],
}

#[derive(Clone)]
struct Ext4DirEntry {
    inode: u32,
    file_type: u8,
    name: String,
}

const EXT4_S_IFDIR: u16 = 0x4000;
const EXT4_S_IFREG: u16 = 0x8000;
const EXT4_S_IFLNK: u16 = 0xA000;
const EXT4_EXTENTS_FL: u32 = 0x0008_0000;
const EXT4_MAX_SYMLINK_DEPTH: usize = 16;

fn read_partition_at(
    disk: &DiskDevice,
    part: &Partition,
    byte_offset: u64,
    len: usize,
) -> Result<Vec<u8>, &'static str> {
    let sector_size = disk.block.sector_size() as usize;
    if sector_size == 0 {
        return Err("storage: invalid sector size");
    }
    if len == 0 {
        return Ok(Vec::new());
    }

    let first_sector = (byte_offset / sector_size as u64) as usize;
    let last_byte = byte_offset.saturating_add(len as u64).saturating_sub(1);
    let last_sector = (last_byte / sector_size as u64) as usize;

    let sectors_to_read = last_sector.saturating_sub(first_sector).saturating_add(1);
    if (first_sector as u64) >= part.sector_count
        || (last_sector as u64) >= part.sector_count
    {
        return Err("storage: ext4 read beyond partition");
    }

    let mut raw = vec![0u8; sectors_to_read.saturating_mul(sector_size)];
    let mut scratch = vec![0u8; sector_size];
    for i in 0..sectors_to_read {
        let lba = part
            .start_lba
            .saturating_add(first_sector as u64)
            .saturating_add(i as u64);
        disk.block.read_sector(lba, scratch.as_mut_slice())?;
        let at = i.saturating_mul(sector_size);
        raw[at..at + sector_size].copy_from_slice(scratch.as_slice());
    }

    let in_sector = (byte_offset % sector_size as u64) as usize;
    let end = in_sector.saturating_add(len);
    if end > raw.len() {
        return Err("storage: ext4 read window overflow");
    }
    Ok(raw[in_sector..end].to_vec())
}

fn ext4_load_superblock(disk: &DiskDevice, part: &Partition) -> Result<Ext4Superblock, &'static str> {
    let sb = read_partition_at(disk, part, 1024, 1024)?;
    let magic = le_u16(sb.as_slice(), 56).ok_or("storage: ext4 superblock truncated")?;
    if magic != 0xEF53 {
        return Err("storage: ext4 superblock magic missing");
    }

    let log_block_size = le_u32(sb.as_slice(), 24).ok_or("storage: ext4 sb truncated")?;
    let block_size = 1024u64.checked_shl(log_block_size).ok_or("storage: ext4 block size invalid")?;
    let _blocks_per_group = le_u32(sb.as_slice(), 32).ok_or("storage: ext4 sb truncated")?;
    let inodes_per_group = le_u32(sb.as_slice(), 40).ok_or("storage: ext4 sb truncated")?;
    let inode_size = le_u16(sb.as_slice(), 88).unwrap_or(128).max(128);
    let desc_size = le_u16(sb.as_slice(), 0xFE).unwrap_or(32).max(32);

    Ok(Ext4Superblock {
        block_size,
        inodes_per_group,
        inode_size,
        desc_size,
    })
}

fn ext4_read_block(
    disk: &DiskDevice,
    part: &Partition,
    sb: &Ext4Superblock,
    block_no: u64,
) -> Result<Vec<u8>, &'static str> {
    let offset = block_no.saturating_mul(sb.block_size);
    read_partition_at(disk, part, offset, sb.block_size as usize)
}

fn ext4_read_group_desc(
    disk: &DiskDevice,
    part: &Partition,
    sb: &Ext4Superblock,
    group: u32,
) -> Result<Vec<u8>, &'static str> {
    let gdt_block = if sb.block_size == 1024 { 2 } else { 1 };
    let gdt_offset = (gdt_block as u64)
        .saturating_mul(sb.block_size)
        .saturating_add((group as u64).saturating_mul(sb.desc_size as u64));
    read_partition_at(disk, part, gdt_offset, sb.desc_size as usize)
}

fn ext4_load_inode(
    disk: &DiskDevice,
    part: &Partition,
    sb: &Ext4Superblock,
    inode_no: u32,
) -> Result<Ext4Inode, &'static str> {
    if inode_no == 0 {
        return Err("storage: ext4 inode 0 invalid");
    }

    let ino_index = inode_no - 1;
    let group = ino_index / sb.inodes_per_group;
    let index = ino_index % sb.inodes_per_group;

    let gd = ext4_read_group_desc(disk, part, sb, group)?;
    let inode_table_lo = le_u32(gd.as_slice(), 8).ok_or("storage: ext4 group desc truncated")?;
    let inode_table_hi = if sb.desc_size >= 64 {
        le_u32(gd.as_slice(), 40).unwrap_or(0)
    } else {
        0
    };
    let inode_table_block = ((inode_table_hi as u64) << 32) | (inode_table_lo as u64);
    let inode_offset = inode_table_block
        .saturating_mul(sb.block_size)
        .saturating_add((index as u64).saturating_mul(sb.inode_size as u64));
    let raw = read_partition_at(disk, part, inode_offset, sb.inode_size as usize)?;
    if raw.len() < 128 {
        return Err("storage: ext4 inode truncated");
    }

    let mode = le_u16(raw.as_slice(), 0).ok_or("storage: ext4 inode mode missing")?;
    let size_lo = le_u32(raw.as_slice(), 4).unwrap_or(0) as u64;
    let size_high = le_u32(raw.as_slice(), 108).unwrap_or(0) as u64;
    let flags = le_u32(raw.as_slice(), 32).unwrap_or(0);

    let mut block = [0u8; 60];
    block.copy_from_slice(raw.get(40..100).ok_or("storage: ext4 inode block map missing")?);

    Ok(Ext4Inode {
        mode,
        size: size_lo | (size_high << 32),
        flags,
        block,
    })
}

fn ext4_extent_lookup_in_node(
    disk: &DiskDevice,
    part: &Partition,
    sb: &Ext4Superblock,
    node: &[u8],
    logical: u32,
) -> Result<Option<u64>, &'static str> {
    let magic = le_u16(node, 0).ok_or("storage: ext4 extent header truncated")?;
    if magic != 0xF30A {
        return Err("storage: ext4 extent header magic invalid");
    }
    let entries = le_u16(node, 2).unwrap_or(0) as usize;
    let depth = le_u16(node, 6).unwrap_or(0);
    let header_size = 12usize;

    if depth == 0 {
        for i in 0..entries {
            let at = header_size + i.saturating_mul(12);
            if at + 12 > node.len() {
                break;
            }
            let ee_block = le_u32(node, at).unwrap_or(0);
            let ee_len_raw = le_u16(node, at + 4).unwrap_or(0);
            let ee_len = (ee_len_raw & 0x7FFF) as u32;
            let ee_start_hi = le_u16(node, at + 6).unwrap_or(0) as u64;
            let ee_start_lo = le_u32(node, at + 8).unwrap_or(0) as u64;
            if ee_len == 0 {
                continue;
            }
            if logical >= ee_block && logical < ee_block.saturating_add(ee_len) {
                let delta = logical.saturating_sub(ee_block) as u64;
                let phys = ((ee_start_hi << 32) | ee_start_lo).saturating_add(delta);
                return Ok(Some(phys));
            }
        }
        return Ok(None);
    }

    let mut chosen: Option<(u32, u64)> = None;
    for i in 0..entries {
        let at = header_size + i.saturating_mul(12);
        if at + 12 > node.len() {
            break;
        }
        let ei_block = le_u32(node, at).unwrap_or(0);
        let ei_leaf_lo = le_u32(node, at + 4).unwrap_or(0) as u64;
        let ei_leaf_hi = le_u16(node, at + 8).unwrap_or(0) as u64;
        if logical >= ei_block {
            chosen = Some((ei_block, (ei_leaf_hi << 32) | ei_leaf_lo));
        } else {
            break;
        }
    }

    let Some((_lb, child_block)) = chosen else {
        return Ok(None);
    };
    let child = ext4_read_block(disk, part, sb, child_block)?;
    ext4_extent_lookup_in_node(disk, part, sb, child.as_slice(), logical)
}

fn ext4_resolve_file_block(
    disk: &DiskDevice,
    part: &Partition,
    sb: &Ext4Superblock,
    inode: &Ext4Inode,
    logical_block: u32,
) -> Result<Option<u64>, &'static str> {
    if (inode.flags & EXT4_EXTENTS_FL) != 0 {
        return ext4_extent_lookup_in_node(disk, part, sb, inode.block.as_slice(), logical_block);
    }

    // Fallback: direct blocks only.
    if logical_block < 12 {
        let at = (logical_block as usize).saturating_mul(4);
        let phys = le_u32(inode.block.as_slice(), at).unwrap_or(0) as u64;
        if phys == 0 {
            return Ok(None);
        }
        return Ok(Some(phys));
    }

    Ok(None)
}

fn ext4_read_inode_data(
    disk: &DiskDevice,
    part: &Partition,
    sb: &Ext4Superblock,
    inode: &Ext4Inode,
) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    if inode.size == 0 {
        return Ok(out);
    }

    let total_blocks = (inode.size.saturating_add(sb.block_size - 1) / sb.block_size) as u32;
    for lb in 0..total_blocks {
        let Some(pb) = ext4_resolve_file_block(disk, part, sb, inode, lb)? else {
            break;
        };
        let block = ext4_read_block(disk, part, sb, pb)?;
        out.extend_from_slice(block.as_slice());
        if out.len() >= inode.size as usize {
            break;
        }
    }

    out.truncate(inode.size as usize);
    Ok(out)
}

fn ext4_list_dir(
    disk: &DiskDevice,
    part: &Partition,
    sb: &Ext4Superblock,
    inode: &Ext4Inode,
) -> Result<Vec<Ext4DirEntry>, &'static str> {
    if (inode.mode & EXT4_S_IFDIR) == 0 {
        return Err("not a directory");
    }

    let data = ext4_read_inode_data(disk, part, sb, inode)?;
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= data.len() {
        let ino = le_u32(data.as_slice(), at).unwrap_or(0);
        let rec_len = le_u16(data.as_slice(), at + 4).unwrap_or(0) as usize;
        let name_len = *data.get(at + 6).unwrap_or(&0) as usize;
        let file_type = *data.get(at + 7).unwrap_or(&0);
        if rec_len == 0 {
            break;
        }
        if ino != 0 && at + rec_len <= data.len() && name_len <= rec_len.saturating_sub(8) {
            let name_bytes = &data[at + 8..at + 8 + name_len];
            if let Ok(name) = core::str::from_utf8(name_bytes) {
                out.push(Ext4DirEntry {
                    inode: ino,
                    file_type,
                    name: name.to_string(),
                });
            }
        }
        at = at.saturating_add(rec_len);
    }

    Ok(out)
}

fn ext4_lookup_path(
    disk: &DiskDevice,
    part: &Partition,
    rel: &str,
) -> Result<(Ext4Superblock, u32, Ext4Inode), &'static str> {
    let sb = ext4_load_superblock(disk, part)?;
    let mut path = normalize_path(rel);

    for _ in 0..EXT4_MAX_SYMLINK_DEPTH {
        let mut inode_no = 2u32;
        let mut inode = ext4_load_inode(disk, part, &sb, inode_no)?;
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut resolved_parts: Vec<&str> = Vec::new();
        let mut restarted = false;

        for (idx, seg) in segments.iter().enumerate() {
            if (inode.mode & EXT4_S_IFDIR) == 0 {
                return Err("not a directory");
            }
            let entries = ext4_list_dir(disk, part, &sb, &inode)?;
            let mut found = None;
            for ent in entries {
                if ent.name == *seg {
                    found = Some(ent.inode);
                    break;
                }
            }
            let next = found.ok_or("path not found")?;
            let next_inode = ext4_load_inode(disk, part, &sb, next)?;

            if (next_inode.mode & EXT4_S_IFLNK) != 0 {
                let target_bytes = if next_inode.size as usize <= next_inode.block.len() {
                    next_inode.block[..next_inode.size as usize].to_vec()
                } else {
                    ext4_read_inode_data(disk, part, &sb, &next_inode)?
                };
                let target = core::str::from_utf8(target_bytes.as_slice())
                    .map_err(|_| "storage: ext4 symlink target is not utf-8")?;
                let remaining = if idx + 1 < segments.len() {
                    segments[idx + 1..].join("/")
                } else {
                    String::new()
                };
                let parent = if resolved_parts.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", resolved_parts.join("/"))
                };
                let combined = if target.starts_with('/') {
                    if remaining.is_empty() {
                        target.to_string()
                    } else {
                        format!("{}/{}", target.trim_end_matches('/'), remaining)
                    }
                } else {
                    let base = if parent == "/" {
                        format!("/{}", target)
                    } else {
                        format!("{}/{}", parent, target)
                    };
                    if remaining.is_empty() {
                        base
                    } else {
                        format!("{}/{}", base.trim_end_matches('/'), remaining)
                    }
                };
                path = normalize_path(combined.as_str());
                restarted = true;
                break;
            }

            inode_no = next;
            inode = next_inode;
            resolved_parts.push(seg);
        }

        if !restarted {
            return Ok((sb, inode_no, inode));
        }
    }

    Err("storage: ext4 symlink resolution limit exceeded")
}

fn ext4_with_volume<R>(
    state: &StorageState,
    volume: &str,
    f: impl FnOnce(&DiskDevice, &Partition) -> Result<R, &'static str>,
) -> Result<R, &'static str> {
    let (disk_name, part_name) = resolve_volume_owner(state, volume)?;
    let disk = state
        .disks
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(disk_name.as_str()))
        .ok_or("storage: disk missing")?;
    let part = disk
        .partitions
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(part_name.as_str()))
        .ok_or("storage: partition missing")?;
    f(disk, part)
}

fn mounted_volume_info_internal(
    state: &StorageState,
    abs_path: &str,
) -> Option<(String, FilesystemKind, String)> {
    let (vol, rel) = mounted_volume_for_path_internal(state, abs_path)?;
    Some((vol.name.clone(), vol.filesystem, rel))
}

pub fn init() {
    with_state_mut(|state| {
        if state.initialized {
            return;
        }
        rebuild_volume_registry(state);
        state.initialized = true;
    });
}

fn publish_disks(disks: Vec<DiskDevice>, diagnostics: Vec<String>) {
    with_state_mut(|state| {
        state.disks = disks;
        rebuild_volume_registry(state);
        state.mounted.clear();
        state.diagnostics = diagnostics;
        let registration_diags = register_devices(state);
        state.diagnostics.extend(registration_diags);
        state.initialized = true;
    });
    crate::object_manager::refresh_storage_provider_if_ready();
}

fn perform_rescan() {
    let mut diagnostics = Vec::new();

    SCAN_RUNNING.store(true, Ordering::Release);
    SCAN_REQUESTED.store(false, Ordering::Release);
    SCAN_COMPLETED.store(false, Ordering::Release);

    SCAN_PHASE.store(SCAN_PCI, Ordering::Release);
    pci::init();
    diagnostics.push("stage=pci_detection detail=pci enumeration complete".to_string());

    SCAN_PHASE.store(SCAN_AHCI, Ordering::Release);
    ahci::rescan();
    for diag in ahci::diagnostics_cached() {
        diagnostics.push(format!("{}", diag));
    }

    SCAN_PHASE.store(SCAN_PARTITIONS, Ordering::Release);
    let disks = discover_disks_from_pci(&mut diagnostics);

    SCAN_PHASE.store(SCAN_PUBLISH, Ordering::Release);
    publish_disks(disks, diagnostics);

    SCAN_EPOCH.fetch_add(1, Ordering::AcqRel);
    SCAN_COMPLETED.store(true, Ordering::Release);
    SCAN_RUNNING.store(false, Ordering::Release);
    SCAN_PHASE.store(SCAN_DONE, Ordering::Release);
}

pub fn start_scan_worker() {
    // Single-core mode: keep scanning deterministic and foreground-only.
}

pub fn request_rescan() {
    init();
    SCAN_PHASE.store(SCAN_QUEUED, Ordering::Release);
    SCAN_REQUESTED.store(true, Ordering::Release);
    perform_rescan();
}

pub fn rescan() {
    request_rescan();
}

pub fn scan_status() -> StorageScanStatus {
    let (disks, volumes, failures) = with_state(|state| {
        (
            state.disks.len(),
            state.volumes.len(),
            state.diagnostics.len(),
        )
    });
    StorageScanStatus {
        queued: SCAN_REQUESTED.load(Ordering::Acquire),
        running: SCAN_RUNNING.load(Ordering::Acquire),
        completed: SCAN_COMPLETED.load(Ordering::Acquire),
        epoch: SCAN_EPOCH.load(Ordering::Acquire),
        phase: scan_phase_name(SCAN_PHASE.load(Ordering::Acquire)),
        disks,
        volumes,
        failures,
    }
}

pub fn scan_diagnostics() -> Vec<String> {
    init();
    with_state(|state| state.diagnostics.clone())
}

pub fn scan_diagnostics_cached() -> Vec<String> {
    with_state(|state| state.diagnostics.clone())
}

pub fn supported_filesystems() -> &'static [FilesystemKind] {
    &[
        FilesystemKind::Fat32,
        FilesystemKind::Ext4,
        FilesystemKind::Ntfs,
        FilesystemKind::Fat16,
        FilesystemKind::Fat64,
        FilesystemKind::Fat128,
    ]
}

pub fn volumes() -> Vec<DetectedVolume> {
    init();
    with_state(|state| state.volumes.clone())
}

pub fn volumes_cached() -> Vec<DetectedVolume> {
    with_state(|state| state.volumes.clone())
}

pub fn find_volume_cached(name: &str) -> Option<DetectedVolume> {
    with_state(|state| {
        state
            .volumes
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
            .cloned()
    })
}

pub fn resolve_mountable_volume(name: &str) -> Option<DetectedVolume> {
    with_state(|state| {
        if let Some(volume) = state
            .volumes
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
        {
            if fs_supports_mount_tree(volume.filesystem) && volume.name != "tmpfs" {
                return Some(volume.clone());
            }
        }

        let disk = state
            .disks
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(name))?;
        for part in &disk.partitions {
            if let Some(volume) = state
                .volumes
                .iter()
                .find(|v| v.name.eq_ignore_ascii_case(part.name.as_str()))
                && fs_supports_mount_tree(volume.filesystem)
            {
                return Some(volume.clone());
            }
        }
        None
    })
}

fn disk_snapshot(state: &StorageState) -> Vec<DetectedDisk> {
    state
        .disks
        .iter()
        .map(|disk| DetectedDisk {
            name: disk.name.clone(),
            backing: disk.backing.clone(),
            total_bytes: disk
                .block
                .sectors()
                .saturating_mul(disk.block.sector_size() as u64),
            sector_size: disk.block.sector_size(),
            hardware: disk.block.is_real_hardware(),
            partitions: disk.partitions.iter().map(|p| p.name.clone()).collect(),
        })
        .collect()
}

pub fn disks() -> Vec<DetectedDisk> {
    init();
    with_state(disk_snapshot)
}

pub fn disks_cached() -> Vec<DetectedDisk> {
    with_state(disk_snapshot)
}

pub fn probe_image(image: &[u8]) -> Option<FilesystemKind> {
    probe_filesystem(image).map(|p| p.fs)
}

pub fn find_volume(name: &str) -> Option<DetectedVolume> {
    init();
    with_state(|state| {
        state
            .volumes
            .iter()
            .find(|v| v.name.eq_ignore_ascii_case(name))
            .cloned()
    })
}

pub fn mount_volume(name: &str, path: &str, _read_only: bool) -> Result<(), &'static str> {
    init();
    let result = with_state_mut(|state| {
        let idx = state
            .volumes
            .iter()
            .position(|v| v.name.eq_ignore_ascii_case(name))
            .ok_or("storage: volume not found")?;

        if state.volumes[idx].mounted_at.is_some() {
            return Err("storage: volume already mounted");
        }

        if state
            .volumes
            .iter()
            .any(|v| v.mounted_at.as_deref() == Some(path))
        {
            return Err("storage: duplicate mount path");
        }

        let vol_name = state.volumes[idx].name.clone();
        let vol_fs = state.volumes[idx].filesystem;
        if vol_name == "tmpfs" {
            return Err("storage: volume is not mountable");
        }
        if !fs_supports_mount_tree(vol_fs) {
            return Err("storage: native filesystem reader not implemented for this volume");
        }
        if fs_supports_mount_tree(vol_fs) {
            ensure_volume_mounted(state, vol_name.as_str(), vol_fs)?;
        }

        state.volumes[idx].mounted_at = Some(path.to_string());
        Ok(())
    });
    if result.is_ok() {
        crate::object_manager::refresh_storage_provider_if_ready();
    }
    result
}

pub fn umount_volume(path: &str) -> Result<(), &'static str> {
    init();
    let result = with_state_mut(|state| {
        let idx = state
            .volumes
            .iter()
            .position(|v| v.mounted_at.as_deref() == Some(path))
            .ok_or("storage: no volume mounted at that path")?;

        if fs_supports_rw_tree(state.volumes[idx].filesystem) {
            let name = state.volumes[idx].name.clone();
            let _ = save_mounted_volume(state, name.as_str());
            state
                .mounted
                .retain(|m| !m.volume.eq_ignore_ascii_case(name.as_str()));
        } else {
            let name = state.volumes[idx].name.clone();
            state
                .mounted
                .retain(|m| !m.volume.eq_ignore_ascii_case(name.as_str()));
        }

        state.volumes[idx].mounted_at = None;
        Ok(())
    });
    if result.is_ok() {
        crate::object_manager::refresh_storage_provider_if_ready();
    }
    result
}

pub fn format_volume(name: &str, fs: FilesystemKind) -> Result<(), &'static str> {
    init();
    let result = with_state_mut(|state| {
        let idx = state
            .volumes
            .iter()
            .position(|v| v.name.eq_ignore_ascii_case(name))
            .ok_or("storage: volume not found")?;

        if !fs_supports_rw_tree(fs) {
            return Err("storage: formatter not implemented for requested filesystem");
        }

        if state.volumes[idx].mounted_at.is_some() {
            return Err("storage: volume is currently mounted; unmount before formatting");
        }

        state.volumes[idx].filesystem = fs;
        let (disk_name, part_name) = resolve_volume_owner(state, name)?;
        let disk = state
            .disks
            .iter_mut()
            .find(|d| d.name.eq_ignore_ascii_case(disk_name.as_str()))
            .ok_or("storage: disk missing")?;
        let part = disk
            .partitions
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(part_name.as_str()))
            .ok_or("storage: partition missing")?
            .clone();

        let tree = default_rw_tree(fs);
        let bytes = serialize_tree(tree.as_slice(), fs);
        write_partition_bytes(disk, &part, bytes.as_slice())?;
        disk.block.flush();

        Ok(())
    });
    if result.is_ok() {
        crate::object_manager::refresh_storage_provider_if_ready();
    }
    result
}

pub fn mounted_volume_for_path(path: &str) -> Option<DetectedVolume> {
    init();
    with_state(|state| mounted_volume_for_path_internal(state, path).map(|(v, _)| v.clone()))
}

pub fn mounted_volume_for_path_cached(path: &str) -> Option<DetectedVolume> {
    with_state(|state| mounted_volume_for_path_internal(state, path).map(|(v, _)| v.clone()))
}

pub fn mounted_relative_path(path: &str) -> Option<String> {
    init();
    with_state(|state| mounted_volume_for_path_internal(state, path).map(|(_, rel)| rel))
}

pub fn fs_stat(path: &str) -> Result<FsStat, &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if vol_fs == FilesystemKind::Ext4 {
            if let Some(mounted) = state
                .mounted
                .iter()
                .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            {
                let node = find_node(mounted.nodes.as_slice(), rel.as_str()).ok_or("path not found")?;
                return Ok(FsStat {
                    kind: node.kind,
                    size: node.data.len(),
                });
            }

            return ext4_with_volume(state, vol_name.as_str(), |disk, part| {
                let (_sb, _ino, inode) = ext4_lookup_path(disk, part, rel.as_str())?;
                let kind = if (inode.mode & EXT4_S_IFDIR) != 0 {
                    FsNodeKind::Directory
                } else {
                    FsNodeKind::File
                };
                Ok(FsStat {
                    kind,
                    size: inode.size as usize,
                })
            });
        }
        if !fs_supports_mount_tree(vol_fs) {
            return Err("storage: filesystem backend not implemented");
        }
        ensure_volume_mounted(state, vol_name.as_str(), vol_fs)?;

        let mounted = state
            .mounted
            .iter()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or("storage: mounted fs not found")?;
        let node = find_node(mounted.nodes.as_slice(), rel.as_str()).ok_or("path not found")?;
        Ok(FsStat {
            kind: node.kind,
            size: node.data.len(),
        })
    })
}

pub fn fs_lookup(path: &str) -> Result<(), &'static str> {
    fs_stat(path).map(|_| ())
}

pub fn fs_create(path: &str) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if !fs_supports_mount_tree(vol_fs) {
            return Err("storage: filesystem backend not implemented");
        }
        if !fs_supports_rw_tree(vol_fs) {
            return Err("storage: filesystem is read-only in this build");
        }

        ensure_volume_mounted(state, vol_name.as_str(), vol_fs)?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or(if vol_fs == FilesystemKind::Ext4 {
                "storage: ext4 volume is native read-only; format volume to enable writes"
            } else {
                "storage: mounted fs not found"
            })?;

        if find_node(mounted.nodes.as_slice(), rel.as_str()).is_some() {
            return Err("already exists");
        }
        ensure_parent_dir(mounted.nodes.as_slice(), rel.as_str())?;

        mounted.nodes.push(FsNode {
            path: rel,
            kind: FsNodeKind::File,
            data: Vec::new(),
        });

        if fs_supports_rw_tree(vol_fs) {
            save_mounted_volume(state, vol_name.as_str())
        } else {
            Ok(())
        }
    })
}

pub fn fs_mkdir(path: &str) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if !fs_supports_mount_tree(vol_fs) {
            return Err("storage: filesystem backend not implemented");
        }
        if !fs_supports_rw_tree(vol_fs) {
            return Err("storage: filesystem is read-only in this build");
        }

        ensure_volume_mounted(state, vol_name.as_str(), vol_fs)?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or(if vol_fs == FilesystemKind::Ext4 {
                "storage: ext4 volume is native read-only; format volume to enable writes"
            } else {
                "storage: mounted fs not found"
            })?;

        if find_node(mounted.nodes.as_slice(), rel.as_str()).is_some() {
            return Err("already exists");
        }
        ensure_parent_dir(mounted.nodes.as_slice(), rel.as_str())?;

        mounted.nodes.push(FsNode {
            path: rel,
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        });

        if fs_supports_rw_tree(vol_fs) {
            save_mounted_volume(state, vol_name.as_str())
        } else {
            Ok(())
        }
    })
}

pub fn fs_delete(path: &str) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if rel == "/" {
            return Err("cannot remove root");
        }
        if !fs_supports_mount_tree(vol_fs) {
            return Err("storage: filesystem backend not implemented");
        }
        if !fs_supports_rw_tree(vol_fs) {
            return Err("storage: filesystem is read-only in this build");
        }

        ensure_volume_mounted(state, vol_name.as_str(), vol_fs)?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or(if vol_fs == FilesystemKind::Ext4 {
                "storage: ext4 volume is native read-only; format volume to enable writes"
            } else {
                "storage: mounted fs not found"
            })?;

        let idx = mounted
            .nodes
            .iter()
            .position(|n| n.path == rel)
            .ok_or("path not found")?;
        let kind = mounted.nodes[idx].kind;

        if kind == FsNodeKind::Directory
            && mounted
                .nodes
                .iter()
                .any(|n| n.path != rel && is_child_of(rel.as_str(), n.path.as_str()))
        {
            return Err("directory not empty");
        }

        mounted.nodes.remove(idx);
        if fs_supports_rw_tree(vol_fs) {
            save_mounted_volume(state, vol_name.as_str())
        } else {
            Ok(())
        }
    })
}

pub fn fs_rename(from: &str, to: &str) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let (from_name, from_fs, from_rel) = mounted_volume_info_internal(state, from)
            .ok_or("storage: source is not on a mounted volume")?;
        let (to_name, _to_fs, to_rel) = mounted_volume_info_internal(state, to)
            .ok_or("storage: destination is not on a mounted volume")?;

        if !from_name.eq_ignore_ascii_case(to_name.as_str()) {
            return Err("storage: cross-volume rename is not supported");
        }
        if !fs_supports_mount_tree(from_fs) {
            return Err("storage: filesystem backend not implemented");
        }
        if !fs_supports_rw_tree(from_fs) {
            return Err("storage: filesystem is read-only in this build");
        }

        ensure_volume_mounted(state, from_name.as_str(), from_fs)?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(from_name.as_str()))
            .ok_or(if from_fs == FilesystemKind::Ext4 {
                "storage: ext4 volume is native read-only; format volume to enable writes"
            } else {
                "storage: mounted fs not found"
            })?;

        ensure_parent_dir(mounted.nodes.as_slice(), to_rel.as_str())?;
        if find_node(mounted.nodes.as_slice(), to_rel.as_str()).is_some() {
            return Err("destination exists");
        }

        let idx = mounted
            .nodes
            .iter()
            .position(|n| n.path == from_rel)
            .ok_or("path not found")?;
        let old = mounted.nodes[idx].path.clone();
        mounted.nodes[idx].path = to_rel.clone();

        if mounted.nodes[idx].kind == FsNodeKind::Directory {
            for node in &mut mounted.nodes {
                if node.path != old && is_child_of(old.as_str(), node.path.as_str()) {
                    node.path = format!("{}{}", to_rel, &node.path[old.len()..]);
                }
            }
        }

        if fs_supports_rw_tree(from_fs) {
            save_mounted_volume(state, from_name.as_str())
        } else {
            Ok(())
        }
    })
}

pub fn fs_read(path: &str) -> Result<Vec<u8>, &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if vol_fs == FilesystemKind::Ext4 {
            if let Some(mounted) = state
                .mounted
                .iter()
                .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            {
                let node = find_node(mounted.nodes.as_slice(), rel.as_str()).ok_or("path not found")?;
                if node.kind != FsNodeKind::File {
                    return Err("not a file");
                }
                return Ok(node.data.clone());
            }

            return ext4_with_volume(state, vol_name.as_str(), |disk, part| {
                let (sb, _ino, inode) = ext4_lookup_path(disk, part, rel.as_str())?;
                if (inode.mode & EXT4_S_IFREG) == 0 {
                    return Err("not a file");
                }
                ext4_read_inode_data(disk, part, &sb, &inode)
            });
        }
        if !fs_supports_mount_tree(vol_fs) {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_volume_mounted(state, vol_name.as_str(), vol_fs)?;
        let mounted = state
            .mounted
            .iter()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or("storage: mounted fs not found")?;
        let node = find_node(mounted.nodes.as_slice(), rel.as_str()).ok_or("path not found")?;
        if node.kind != FsNodeKind::File {
            return Err("not a file");
        }

        Ok(node.data.clone())
    })
}

pub fn fs_write(path: &str, data: &[u8]) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if !fs_supports_mount_tree(vol_fs) {
            return Err("storage: filesystem backend not implemented");
        }
        if !fs_supports_rw_tree(vol_fs) {
            return Err("storage: filesystem is read-only in this build");
        }

        ensure_volume_mounted(state, vol_name.as_str(), vol_fs)?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or(if vol_fs == FilesystemKind::Ext4 {
                "storage: ext4 volume is native read-only; format volume to enable writes"
            } else {
                "storage: mounted fs not found"
            })?;

        if find_node(mounted.nodes.as_slice(), rel.as_str()).is_none() {
            ensure_parent_dir(mounted.nodes.as_slice(), rel.as_str())?;
            mounted.nodes.push(FsNode {
                path: rel.clone(),
                kind: FsNodeKind::File,
                data: Vec::new(),
            });
        }

        let node =
            find_node_mut(mounted.nodes.as_mut_slice(), rel.as_str()).ok_or("path not found")?;
        if node.kind != FsNodeKind::File {
            return Err("not a file");
        }
        node.data.clear();
        node.data.extend_from_slice(data);

        if fs_supports_rw_tree(vol_fs) {
            save_mounted_volume(state, vol_name.as_str())
        } else {
            Ok(())
        }
    })
}

pub fn fs_readdir(path: &str) -> Result<Vec<String>, &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if vol_fs == FilesystemKind::Ext4 {
            if let Some(mounted) = state
                .mounted
                .iter()
                .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            {
                let dir = find_node(mounted.nodes.as_slice(), rel.as_str()).ok_or("path not found")?;
                if dir.kind != FsNodeKind::Directory {
                    return Err("not a directory");
                }

                let mut out = Vec::new();
                for node in &mounted.nodes {
                    if node.path == rel {
                        continue;
                    }
                    let Some((parent, name)) = split_parent(node.path.as_str()) else {
                        continue;
                    };
                    if parent == rel {
                        out.push(name);
                    }
                }

                out.sort();
                out.dedup();
                return Ok(out);
            }

            return ext4_with_volume(state, vol_name.as_str(), |disk, part| {
                let (sb, _ino, inode) = ext4_lookup_path(disk, part, rel.as_str())?;
                let entries = ext4_list_dir(disk, part, &sb, &inode)?;
                let mut out = Vec::new();
                for ent in entries {
                    if ent.name == "." || ent.name == ".." {
                        continue;
                    }
                    let _ = ent.file_type;
                    out.push(ent.name);
                }
                out.sort();
                out.dedup();
                Ok(out)
            });
        }
        if !fs_supports_mount_tree(vol_fs) {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_volume_mounted(state, vol_name.as_str(), vol_fs)?;
        let mounted = state
            .mounted
            .iter()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or("storage: mounted fs not found")?;

        let dir = find_node(mounted.nodes.as_slice(), rel.as_str()).ok_or("path not found")?;
        if dir.kind != FsNodeKind::Directory {
            return Err("not a directory");
        }

        let mut out = Vec::new();
        for node in &mounted.nodes {
            if node.path == rel {
                continue;
            }
            let Some((parent, name)) = split_parent(node.path.as_str()) else {
                continue;
            };
            if parent == rel {
                out.push(name);
            }
        }

        out.sort();
        out.dedup();
        Ok(out)
    })
}

pub fn read_sector(device_name: &str, lba: u64, out: &mut [u8]) -> Result<(), &'static str> {
    init();
    with_state(|state| {
        for disk in &state.disks {
            if disk.name.eq_ignore_ascii_case(device_name)
                || format!("/dev/{}", disk.name).eq_ignore_ascii_case(device_name)
            {
                return disk.block.read_sector(lba, out);
            }

            for part in &disk.partitions {
                if part.name.eq_ignore_ascii_case(device_name)
                    || format!("/dev/{}", part.name).eq_ignore_ascii_case(device_name)
                {
                    if lba >= part.sector_count {
                        return Err("storage: lba out of partition range");
                    }
                    return disk
                        .block
                        .read_sector(part.start_lba.saturating_add(lba), out);
                }
            }
        }
        Err("storage: device not found")
    })
}

pub fn write_sector(device_name: &str, lba: u64, data: &[u8]) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        for disk in &mut state.disks {
            if disk.name.eq_ignore_ascii_case(device_name)
                || format!("/dev/{}", disk.name).eq_ignore_ascii_case(device_name)
            {
                return disk.block.write_sector(lba, data);
            }

            for part in &disk.partitions {
                if part.name.eq_ignore_ascii_case(device_name)
                    || format!("/dev/{}", part.name).eq_ignore_ascii_case(device_name)
                {
                    if lba >= part.sector_count {
                        return Err("storage: lba out of partition range");
                    }
                    return disk
                        .block
                        .write_sector(part.start_lba.saturating_add(lba), data);
                }
            }
        }
        Err("storage: device not found")
    })
}

pub fn flush(device_name: &str) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        for disk in &mut state.disks {
            if disk.name.eq_ignore_ascii_case(device_name)
                || format!("/dev/{}", disk.name).eq_ignore_ascii_case(device_name)
                || disk.partitions.iter().any(|p| {
                    p.name.eq_ignore_ascii_case(device_name)
                        || format!("/dev/{}", p.name).eq_ignore_ascii_case(device_name)
                })
            {
                disk.block.flush();
                return Ok(());
            }
        }
        Err("storage: device not found")
    })
}

pub fn sector_count(device_name: &str) -> Option<u64> {
    init();
    with_state(|state| {
        for disk in &state.disks {
            if disk.name.eq_ignore_ascii_case(device_name)
                || format!("/dev/{}", disk.name).eq_ignore_ascii_case(device_name)
            {
                return Some(disk.block.sectors());
            }

            for part in &disk.partitions {
                if part.name.eq_ignore_ascii_case(device_name)
                    || format!("/dev/{}", part.name).eq_ignore_ascii_case(device_name)
                {
                    return Some(part.sector_count);
                }
            }
        }
        None
    })
}

pub fn sector_size(device_name: &str) -> Option<u16> {
    init();
    with_state(|state| {
        for disk in &state.disks {
            if disk.name.eq_ignore_ascii_case(device_name)
                || format!("/dev/{}", disk.name).eq_ignore_ascii_case(device_name)
            {
                return Some(disk.block.sector_size());
            }

            for part in &disk.partitions {
                if part.name.eq_ignore_ascii_case(device_name)
                    || format!("/dev/{}", part.name).eq_ignore_ascii_case(device_name)
                {
                    return Some(disk.block.sector_size());
                }
            }
        }
        None
    })
}

pub fn total_bytes(path: &str) -> Option<u64> {
    mounted_volume_for_path(path).map(|v| v.total_bytes)
}

pub fn used_bytes(path: &str) -> Option<u64> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, _rel) = mounted_volume_info_internal(state, path)?;
        if !fs_supports_mount_tree(vol_fs) {
            return Some(0);
        }
        ensure_volume_mounted(state, vol_name.as_str(), vol_fs).ok()?;
        let mounted = state
            .mounted
            .iter()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))?;
        let used = mounted
            .nodes
            .iter()
            .filter(|n| n.kind == FsNodeKind::File)
            .fold(0u64, |acc, n| acc.saturating_add(n.data.len() as u64));
        Some(used)
    })
}
