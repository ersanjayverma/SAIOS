use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::x86_64::sync::StaticCell;

use crate::pci;

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

#[derive(Clone)]
struct StorageState {
    initialized: bool,
    volumes: Vec<DetectedVolume>,
}

impl StorageState {
    fn new() -> Self {
        Self {
            initialized: false,
            volumes: Vec::new(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct ProbeResult {
    fs: FilesystemKind,
    total_bytes: u64,
    sector_size: u16,
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

fn probe_ext4(image: &[u8]) -> Option<ProbeResult> {
    if image.len() < 2048 {
        return None;
    }

    let superblock = 1024usize;
    let magic = le_u16(image, superblock + 56)?;
    if magic != 0xEF53 {
        return None;
    }

    let log_block_size = le_u32(image, superblock + 24)?;
    let block_size = 1024u64.checked_shl(log_block_size.min(20))?;
    let blocks = le_u32(image, superblock + 4)? as u64;

    Some(ProbeResult {
        fs: FilesystemKind::Ext4,
        total_bytes: blocks.saturating_mul(block_size),
        sector_size: 512,
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

    let sector_size = le_u16(image, 11)?;
    let total_sectors = le_u64(image, 40)?;

    Some(ProbeResult {
        fs: FilesystemKind::Ntfs,
        total_bytes: total_sectors.saturating_mul(sector_size as u64),
        sector_size,
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
    let total_sectors = if sectors_16 != 0 { sectors_16 } else { sectors_32 };
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
    } else {
        // Conservative fallback for custom FAT variants.
        if sectors_16 != 0 {
            FilesystemKind::Fat16
        } else {
            FilesystemKind::Fat32
        }
    };

    Some(ProbeResult {
        fs,
        total_bytes: (total_sectors as u64).saturating_mul(sector_size as u64),
        sector_size,
    })
}

fn probe_filesystem(image: &[u8]) -> Option<ProbeResult> {
    probe_ext4(image)
        .or_else(|| probe_ntfs(image))
        .or_else(|| probe_fat(image))
}

fn synthetic_image_for(fs: FilesystemKind) -> Vec<u8> {
    let mut image = vec![0u8; 4096];
    image[510] = 0x55;
    image[511] = 0xAA;

    match fs {
        FilesystemKind::Ext4 => {
            let sb = 1024usize;
            image[sb + 56] = 0x53;
            image[sb + 57] = 0xEF;
            image[sb + 4..sb + 8].copy_from_slice(&4096u32.to_le_bytes());
            image[sb + 24..sb + 28].copy_from_slice(&0u32.to_le_bytes());
        }
        FilesystemKind::Ntfs => {
            image[3..11].copy_from_slice(b"NTFS    ");
            image[11..13].copy_from_slice(&512u16.to_le_bytes());
            image[40..48].copy_from_slice(&262_144u64.to_le_bytes());
        }
        FilesystemKind::Fat16 | FilesystemKind::Fat32 | FilesystemKind::Fat64 | FilesystemKind::Fat128 => {
            image[11..13].copy_from_slice(&512u16.to_le_bytes());
            image[13] = 8;
            image[14..16].copy_from_slice(&32u16.to_le_bytes());
            image[32..36].copy_from_slice(&131_072u32.to_le_bytes());
            let label = match fs {
                FilesystemKind::Fat16 => b"FAT16   ",
                FilesystemKind::Fat32 => b"FAT32   ",
                FilesystemKind::Fat64 => b"FAT64   ",
                FilesystemKind::Fat128 => b"FAT128  ",
                _ => b"FAT32   ",
            };
            image[82..90].copy_from_slice(label);
        }
        FilesystemKind::TmpFs => {}
    }

    image
}

fn seed_default_volumes(state: &mut StorageState) {
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

    let mut index = 0usize;
    let fs_kinds = [
        FilesystemKind::Ext4,
        FilesystemKind::Ntfs,
        FilesystemKind::Fat16,
        FilesystemKind::Fat32,
        FilesystemKind::Fat64,
        FilesystemKind::Fat128,
    ];

    for fs in fs_kinds {
        let image = synthetic_image_for(fs);
        if let Some(probe) = probe_filesystem(&image) {
            state.volumes.push(DetectedVolume {
                name: probe.fs.as_str().to_string(),
                filesystem: probe.fs,
                backing: format!("ramdisk{}", index),
                total_bytes: probe.total_bytes,
                sector_size: probe.sector_size,
                mounted_at: None,
                writable: true,
            });
            index = index.saturating_add(1);
        }
    }
}

fn append_mass_storage_backends(state: &mut StorageState) {
    let mut disk_index = 0usize;
    for dev in pci::devices() {
        if dev.class == 0x01 {
            state.volumes.push(DetectedVolume {
                name: format!("disk{}", disk_index),
                filesystem: FilesystemKind::TmpFs,
                backing: format!(
                    "pci {:02x}:{:02x}.{}",
                    dev.bus, dev.device, dev.function
                ),
                total_bytes: 0,
                sector_size: 512,
                mounted_at: None,
                writable: true,
            });
            disk_index = disk_index.saturating_add(1);
        }
    }
}

pub fn init() {
    with_state_mut(|state| {
        if state.initialized {
            return;
        }

        seed_default_volumes(state);
        append_mass_storage_backends(state);
        state.initialized = true;
    });
}

pub fn rescan() {
    with_state_mut(|state| {
        seed_default_volumes(state);
        append_mass_storage_backends(state);
        state.initialized = true;
    });
}

pub fn supported_filesystems() -> &'static [FilesystemKind] {
    &[
        FilesystemKind::Ext4,
        FilesystemKind::Ntfs,
        FilesystemKind::Fat16,
        FilesystemKind::Fat32,
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
