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
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::kernel::device::{self, DeviceStatus};
use crate::pci;

const MBR_SECTOR: usize = 512;
const FAT_STORE_MAGIC: &[u8; 8] = b"SAFAT32\0";
const FAT_STORE_VERSION: u32 = 1;
const SYNTHETIC_DISK_SECTORS: u64 = 4_096;

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
        match s.to_ascii_lowercase().as_str() {
            "tmpfs" => Some(Self::TmpFs),
            "ext4" => Some(Self::Ext4),
            "ntfs" => Some(Self::Ntfs),
            "fat16" => Some(Self::Fat16),
            "fat32" => Some(Self::Fat32),
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

#[derive(Clone)]
struct RamBlockDevice {
    sector_size: u16,
    sectors: u64,
    bytes: Vec<u8>,
    dirty: bool,
}

impl RamBlockDevice {
    fn new(sectors: u64, sector_size: u16) -> Self {
        let total = (sectors as usize).saturating_mul(sector_size as usize);
        let mut dev = Self {
            sector_size,
            sectors,
            bytes: vec![0u8; total],
            dirty: false,
        };
        seed_default_mbr_fat32(&mut dev);
        dev
    }

    fn read_sector(&self, lba: u64, out: &mut [u8]) -> Result<(), &'static str> {
        if lba >= self.sectors {
            return Err("storage: lba out of range");
        }
        if out.len() != self.sector_size as usize {
            return Err("storage: invalid read buffer size");
        }

        let start = (lba as usize).saturating_mul(self.sector_size as usize);
        let end = start.saturating_add(self.sector_size as usize);
        if end > self.bytes.len() {
            return Err("storage: read past device end");
        }

        out.copy_from_slice(&self.bytes[start..end]);
        Ok(())
    }

    fn write_sector(&mut self, lba: u64, data: &[u8]) -> Result<(), &'static str> {
        if lba >= self.sectors {
            return Err("storage: lba out of range");
        }
        if data.len() != self.sector_size as usize {
            return Err("storage: invalid write buffer size");
        }

        let start = (lba as usize).saturating_mul(self.sector_size as usize);
        let end = start.saturating_add(self.sector_size as usize);
        if end > self.bytes.len() {
            return Err("storage: write past device end");
        }

        self.bytes[start..end].copy_from_slice(data);
        self.dirty = true;
        Ok(())
    }

    fn flush(&mut self) {
        self.dirty = false;
    }
}

#[derive(Clone)]
struct DiskDevice {
    name: String,
    backing: String,
    block: RamBlockDevice,
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
}

impl StorageState {
    fn new() -> Self {
        Self {
            initialized: false,
            volumes: Vec::new(),
            disks: Vec::new(),
            mounted: Vec::new(),
        }
    }
}

static STATE: StaticCell<Option<StorageState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

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

    Some(ProbeResult {
        fs,
    })
}

fn probe_filesystem(image: &[u8]) -> Option<ProbeResult> {
    probe_ext4(image)
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
        FsNode {
            path: "/boot".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
        FsNode {
            path: "/dev".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
        FsNode {
            path: "/home".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
        FsNode {
            path: "/etc".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
        FsNode {
            path: "/tmp".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
        FsNode {
            path: "/proc".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
        FsNode {
            path: "/sys".to_string(),
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        },
    ]
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

fn serialize_tree(nodes: &[FsNode]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(FAT_STORE_MAGIC);
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
    if bytes.get(0..8)? != FAT_STORE_MAGIC {
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
        let path = core::str::from_utf8(bytes.get(at..p_end)?).ok()?.to_string();
        at = p_end;

        let d_end = at.checked_add(data_len)?;
        let data = bytes.get(at..d_end)?.to_vec();
        at = d_end;

        nodes.push(FsNode {
            path,
            kind,
            data,
        });
    }

    Some(nodes)
}

fn write_partition_bytes(
    disk: &mut DiskDevice,
    part: &Partition,
    bytes: &[u8],
) -> Result<(), &'static str> {
    let sector_size = disk.block.sector_size as usize;
    let mut lba = part.start_lba;
    let mut at = 0usize;
    let mut scratch = vec![0u8; sector_size];
    let max_lba = part.start_lba.saturating_add(part.sector_count);

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
    let sector_size = disk.block.sector_size as usize;
    let mut lba = part.start_lba;
    let max_lba = part.start_lba.saturating_add(part.sector_count);
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

    let bytes = serialize_tree(mounted.nodes.as_slice());
    write_partition_bytes(disk, &part, bytes.as_slice())
}

fn resolve_volume_owner(state: &StorageState, volume: &str) -> Result<(String, String), &'static str> {
    for disk in &state.disks {
        for part in &disk.partitions {
            if part.name.eq_ignore_ascii_case(volume) {
                return Ok((disk.name.clone(), part.name.clone()));
            }
        }
    }
    Err("storage: volume backend unavailable")
}

fn load_volume_tree(state: &StorageState, volume: &str) -> Result<Vec<FsNode>, &'static str> {
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
    Ok(deserialize_tree(bytes.as_slice()).unwrap_or_else(default_fat_tree))
}

fn seed_default_mbr_fat32(dev: &mut RamBlockDevice) {
    if dev.sector_size as usize != MBR_SECTOR {
        return;
    }

    let mut mbr = [0u8; MBR_SECTOR];
    let start_lba = 2048u32;
    let sectors = (dev.sectors.saturating_sub(start_lba as u64).min(u32::MAX as u64)) as u32;

    let p0 = 446usize;
    mbr[p0 + 4] = 0x0C;
    mbr[p0 + 8..p0 + 12].copy_from_slice(&start_lba.to_le_bytes());
    mbr[p0 + 12..p0 + 16].copy_from_slice(&sectors.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;

    let _ = dev.write_sector(0, &mbr);

    let mut bpb = [0u8; MBR_SECTOR];
    bpb[0] = 0xEB;
    bpb[1] = 0x58;
    bpb[2] = 0x90;
    bpb[3..11].copy_from_slice(b"MSWIN4.1");
    bpb[11..13].copy_from_slice(&(512u16).to_le_bytes());
    bpb[13] = 8;
    bpb[14..16].copy_from_slice(&(32u16).to_le_bytes());
    bpb[16] = 2;
    bpb[32..36].copy_from_slice(&sectors.to_le_bytes());
    bpb[36..40].copy_from_slice(&(128u32).to_le_bytes());
    bpb[44..48].copy_from_slice(&(2u32).to_le_bytes());
    bpb[71..82].copy_from_slice(b"NO NAME    ");
    bpb[82..90].copy_from_slice(b"FAT32   ");
    bpb[510] = 0x55;
    bpb[511] = 0xAA;
    let _ = dev.write_sector(start_lba as u64, &bpb);
    dev.flush();
}

fn parse_mbr_partitions(disk: &DiskDevice) -> Vec<Partition> {
    let mut mbr = vec![0u8; disk.block.sector_size as usize];
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
    let mut sector = vec![0u8; disk.block.sector_size as usize];
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
    let sec_size = disk.block.sector_size as usize;
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
            sector_count: disk.block.sectors.saturating_sub(2048),
            fs_hint: FilesystemKind::Fat32,
        });
    }

    for (i, part) in parts.iter_mut().enumerate() {
        part.name = format!("{}p{}", disk.name, i + 1);
    }

    disk.partitions = parts;
}

fn register_devices(state: &StorageState) {
    for disk in &state.disks {
        let _ = device::ensure_device(
            format!("/dev/{}", disk.name).as_str(),
            "storage",
            "block/disk",
            DeviceStatus::Online,
        );

        for part in &disk.partitions {
            let _ = device::ensure_device(
                format!("/dev/{}", part.name).as_str(),
                "storage",
                "block/partition",
                DeviceStatus::Online,
            );
        }
    }
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
        state.volumes.push(DetectedVolume {
            name: disk.name.clone(),
            filesystem: FilesystemKind::TmpFs,
            backing: disk.backing.clone(),
            total_bytes: disk
                .block
                .sectors
                .saturating_mul(disk.block.sector_size as u64),
            sector_size: disk.block.sector_size,
            mounted_at: None,
            writable: true,
        });

        for part in &disk.partitions {
            state.volumes.push(DetectedVolume {
                name: part.name.clone(),
                filesystem: part.fs_hint,
                backing: format!("{}:{}", disk.name, part.name),
                total_bytes: part.sector_count.saturating_mul(disk.block.sector_size as u64),
                sector_size: disk.block.sector_size,
                mounted_at: None,
                writable: true,
            });
        }
    }
}

fn discover_disks_from_pci(state: &mut StorageState) {
    state.disks.clear();

    let mut idx = 0usize;
    for dev in pci::devices() {
        if dev.class != 0x01 {
            continue;
        }

        let controller = match (dev.vendor_id, dev.device_id) {
            (0x1AF4, _) => "virtio-blk",
            _ if dev.subclass == 0x06 => "sata-ahci",
            _ if dev.subclass == 0x01 => "ata",
            _ => "storage-generic",
        };

        let mut disk = DiskDevice {
            name: format!("disk{}", idx),
            backing: format!(
                "{} pci {:02x}:{:02x}.{}",
                controller, dev.bus, dev.device, dev.function
            ),
            // Early boot only needs a tiny synthetic backing store so storage
            // enumeration does not allocate hundreds of MiB on real hardware.
            block: RamBlockDevice::new(SYNTHETIC_DISK_SECTORS, 512),
            partitions: Vec::new(),
        };
        detect_partitions_for_disk(&mut disk);
        state.disks.push(disk);
        idx = idx.saturating_add(1);
    }
}

fn ensure_fat_mounted(state: &mut StorageState, volume: &str) -> Result<(), &'static str> {
    if state
        .mounted
        .iter()
        .any(|m| m.volume.eq_ignore_ascii_case(volume))
    {
        return Ok(());
    }

    let nodes = load_volume_tree(state, volume)?;
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
        discover_disks_from_pci(state);
        rebuild_volume_registry(state);
        register_devices(state);
        state.initialized = true;
    });
}

pub fn rescan() {
    with_state_mut(|state| {
        discover_disks_from_pci(state);
        rebuild_volume_registry(state);
        state.mounted.clear();
        register_devices(state);
        state.initialized = true;
    });
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
    with_state_mut(|state| {
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

        if state.volumes[idx].filesystem == FilesystemKind::Fat32 {
            let vol_name = state.volumes[idx].name.clone();
            ensure_fat_mounted(state, vol_name.as_str())?;
        }

        state.volumes[idx].mounted_at = Some(path.to_string());
        Ok(())
    })
}

pub fn umount_volume(path: &str) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let idx = state
            .volumes
            .iter()
            .position(|v| v.mounted_at.as_deref() == Some(path))
            .ok_or("storage: no volume mounted at that path")?;

        if state.volumes[idx].filesystem == FilesystemKind::Fat32 {
            let name = state.volumes[idx].name.clone();
            let _ = save_mounted_volume(state, name.as_str());
            state.mounted.retain(|m| !m.volume.eq_ignore_ascii_case(name.as_str()));
        }

        state.volumes[idx].mounted_at = None;
        Ok(())
    })
}

pub fn format_volume(name: &str, fs: FilesystemKind) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let idx = state
            .volumes
            .iter()
            .position(|v| v.name.eq_ignore_ascii_case(name))
            .ok_or("storage: volume not found")?;

        if state.volumes[idx].mounted_at.is_some() {
            return Err("storage: volume is currently mounted; unmount before formatting");
        }

        state.volumes[idx].filesystem = fs;
        if fs == FilesystemKind::Fat32 {
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

            let bytes = serialize_tree(default_fat_tree().as_slice());
            write_partition_bytes(disk, &part, bytes.as_slice())?;
            disk.block.flush();
        }

        Ok(())
    })
}

pub fn mounted_volume_for_path(path: &str) -> Option<DetectedVolume> {
    init();
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
        if vol_fs != FilesystemKind::Fat32 {
            return Err("storage: filesystem backend not implemented");
        }
        ensure_fat_mounted(state, vol_name.as_str())?;

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
        if vol_fs != FilesystemKind::Fat32 {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_fat_mounted(state, vol_name.as_str())?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or("storage: mounted fs not found")?;

        if find_node(mounted.nodes.as_slice(), rel.as_str()).is_some() {
            return Err("already exists");
        }
        ensure_parent_dir(mounted.nodes.as_slice(), rel.as_str())?;

        mounted.nodes.push(FsNode {
            path: rel,
            kind: FsNodeKind::File,
            data: Vec::new(),
        });

        save_mounted_volume(state, vol_name.as_str())
    })
}

pub fn fs_mkdir(path: &str) -> Result<(), &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if vol_fs != FilesystemKind::Fat32 {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_fat_mounted(state, vol_name.as_str())?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or("storage: mounted fs not found")?;

        if find_node(mounted.nodes.as_slice(), rel.as_str()).is_some() {
            return Err("already exists");
        }
        ensure_parent_dir(mounted.nodes.as_slice(), rel.as_str())?;

        mounted.nodes.push(FsNode {
            path: rel,
            kind: FsNodeKind::Directory,
            data: Vec::new(),
        });

        save_mounted_volume(state, vol_name.as_str())
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
        if vol_fs != FilesystemKind::Fat32 {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_fat_mounted(state, vol_name.as_str())?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or("storage: mounted fs not found")?;

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
        save_mounted_volume(state, vol_name.as_str())
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
        if from_fs != FilesystemKind::Fat32 {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_fat_mounted(state, from_name.as_str())?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(from_name.as_str()))
            .ok_or("storage: mounted fs not found")?;

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

        save_mounted_volume(state, from_name.as_str())
    })
}

pub fn fs_read(path: &str) -> Result<Vec<u8>, &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if vol_fs != FilesystemKind::Fat32 {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_fat_mounted(state, vol_name.as_str())?;
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
        if vol_fs != FilesystemKind::Fat32 {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_fat_mounted(state, vol_name.as_str())?;
        let mounted = state
            .mounted
            .iter_mut()
            .find(|m| m.volume.eq_ignore_ascii_case(vol_name.as_str()))
            .ok_or("storage: mounted fs not found")?;

        if find_node(mounted.nodes.as_slice(), rel.as_str()).is_none() {
            ensure_parent_dir(mounted.nodes.as_slice(), rel.as_str())?;
            mounted.nodes.push(FsNode {
                path: rel.clone(),
                kind: FsNodeKind::File,
                data: Vec::new(),
            });
        }

        let node = find_node_mut(mounted.nodes.as_mut_slice(), rel.as_str()).ok_or("path not found")?;
        if node.kind != FsNodeKind::File {
            return Err("not a file");
        }
        node.data.clear();
        node.data.extend_from_slice(data);

        save_mounted_volume(state, vol_name.as_str())
    })
}

pub fn fs_readdir(path: &str) -> Result<Vec<String>, &'static str> {
    init();
    with_state_mut(|state| {
        let (vol_name, vol_fs, rel) = mounted_volume_info_internal(state, path)
            .ok_or("storage: path is not on a mounted volume")?;
        if vol_fs != FilesystemKind::Fat32 {
            return Err("storage: filesystem backend not implemented");
        }

        ensure_fat_mounted(state, vol_name.as_str())?;
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
                    return disk.block.read_sector(part.start_lba.saturating_add(lba), out);
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
                || disk
                    .partitions
                    .iter()
                    .any(|p| {
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
                return Some(disk.block.sectors);
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
                return Some(disk.block.sector_size);
            }

            for part in &disk.partitions {
                if part.name.eq_ignore_ascii_case(device_name)
                    || format!("/dev/{}", part.name).eq_ignore_ascii_case(device_name)
                {
                    return Some(disk.block.sector_size);
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
        if vol_fs != FilesystemKind::Fat32 {
            return Some(0);
        }
        ensure_fat_mounted(state, vol_name.as_str()).ok()?;
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
