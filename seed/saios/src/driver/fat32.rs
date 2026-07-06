//! Native FAT12 / FAT16 / FAT32 filesystem driver.
//!
//! Implements the complete FAT lifecycle: probe, mount, read, write,
//! directory create/delete, file create/delete/truncate, and format (mkfs).
//!
//! # On-disk layout
//!
//! ```text
//! Sector 0      : Boot sector (BPB)
//! Sector 1      : FSInfo  (FAT32 only)
//! Sectors rsv   : Reserved area  (reserved_sector_count sectors total)
//! FAT1          : First File Allocation Table
//! FAT2          : Second FAT (mirror)
//! Data area     : Cluster 2 onwards
//!   Cluster 2   : Root directory (FAT32) / fixed root area (FAT16/12)
//! ```
//!
//! # FAT entry values
//!
//! | Value (FAT32) | Meaning         |
//! |---------------|-----------------|
//! | 0x00000000    | Free cluster    |
//! | 0x00000001    | Reserved        |
//! | 0x00000002 …  | Next cluster    |
//! | 0x0FFFFFF7    | Bad cluster     |
//! | 0x0FFFFFF8 …  | End of chain    |
//!
//! # Long File Name (LFN / VFAT)
//!
//! LFN entries precede their 8.3 short entry.  Each LFN entry holds 13
//! UTF-16LE characters (26 bytes) spread across three fields.  The ordinal
//! byte encodes position and the `0x40` bit marks the last (highest-order)
//! entry in the sequence.  Entries are stored in reverse order on disk.

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── Block I/O abstraction ─────────────────────────────────────────────────

/// Combined block I/O interface used by all FAT32 driver functions.
/// A single `&mut dyn BlockIo` replaces the previous separate reader + writer
/// closures, avoiding Rust's borrow-conflict when reading and writing the same
/// disk in one function call.
pub trait BlockIo {
    fn read_sector(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str>;
    fn write_sector(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str>;
}

/// `ReadOnlyIo` wraps any `FnMut(u64, &mut [u8])` as a read-only `BlockIo`.
/// Writes will panic — only use when the driver is known not to write.
pub struct ReadOnlyIo<F: FnMut(u64, &mut [u8]) -> Result<(), &'static str>> {
    pub reader: F,
}

impl<F: FnMut(u64, &mut [u8]) -> Result<(), &'static str>> BlockIo for ReadOnlyIo<F> {
    fn read_sector(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        (self.reader)(lba, buf)
    }
    fn write_sector(&mut self, _lba: u64, _buf: &[u8]) -> Result<(), &'static str> {
        Err("fat32: write_sector called on read-only I/O")
    }
}

// ── public re-export of Superblock for the storage layer ──────────────────

pub use Fat32Superblock as Superblock;

// ── constants ─────────────────────────────────────────────────────────────

const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_LFN: u8 = ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_VOLUME_ID;

const CLUSTER_FREE: u32 = 0x00000000;
#[allow(dead_code)]
const CLUSTER_BAD: u32 = 0x0FFFFFF7;
const CLUSTER_EOC: u32 = 0x0FFFFFF8;
const FAT32_MASK: u32 = 0x0FFFFFFF;

const DIR_ENTRY_SIZE: usize = 32;
const DIR_ENTRY_DELETED: u8 = 0xE5;
const DIR_ENTRY_END: u8 = 0x00;

const FSINFO_LEAD_SIG: u32 = 0x41615252;
const FSINFO_STRUCT_SIG: u32 = 0x61417272;
const FSINFO_TRAIL_SIG: u32 = 0xAA550000;

// ── Superblock (parsed BPB) ───────────────────────────────────────────────

/// Parsed FAT Boot Sector / BPB fields needed for all I/O.
#[derive(Clone, Copy, Debug)]
pub struct Fat32Superblock {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sector_count: u16,
    pub num_fats: u8,
    /// For FAT16/12: 0 means use `total_sectors_32`.
    pub root_entry_count: u16,
    pub total_sectors_16: u16,
    pub fat_size_16: u16,
    pub total_sectors_32: u32,
    /// FAT32 only: sectors per FAT.
    pub fat_size_32: u32,
    /// FAT32 only: first cluster of root directory.
    pub root_cluster: u32,
    pub fs_info_sector: u16,
    /// FAT variant determined during probe.
    pub fat_type: FatType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatType {
    Fat12,
    Fat16,
    Fat32,
}

impl Fat32Superblock {
    pub fn sectors_per_fat(&self) -> u32 {
        if self.fat_size_32 != 0 {
            self.fat_size_32
        } else {
            self.fat_size_16 as u32
        }
    }

    /// LBA of the first sector of FAT1 relative to partition start.
    pub fn fat1_start_sector(&self) -> u64 {
        self.reserved_sector_count as u64
    }

    /// LBA of the first sector of FAT2 (mirror) relative to partition start.
    pub fn fat2_start_sector(&self) -> u64 {
        self.fat1_start_sector() + self.sectors_per_fat() as u64
    }

    /// LBA of the first data sector (cluster 2) relative to partition start.
    pub fn data_start_sector(&self) -> u64 {
        let root_dir_sectors = ((self.root_entry_count as u64 * 32)
            .saturating_add(self.bytes_per_sector as u64 - 1))
            / self.bytes_per_sector as u64;
        self.fat1_start_sector()
            + self.num_fats as u64 * self.sectors_per_fat() as u64
            + root_dir_sectors
    }

    /// Convert a cluster number to its first absolute sector (partition-relative).
    pub fn cluster_to_sector(&self, cluster: u32) -> u64 {
        self.data_start_sector()
            .saturating_add((cluster as u64).saturating_sub(2).saturating_mul(self.sectors_per_cluster as u64))
    }

    /// Number of bytes per cluster.
    pub fn cluster_bytes(&self) -> u32 {
        self.bytes_per_sector as u32 * self.sectors_per_cluster as u32
    }

    pub fn total_sectors(&self) -> u64 {
        if self.total_sectors_16 != 0 {
            self.total_sectors_16 as u64
        } else {
            self.total_sectors_32 as u64
        }
    }

    /// FAT16/12: sector-relative LBA of root directory.
    pub fn fat16_root_start_sector(&self) -> u64 {
        self.fat1_start_sector() + self.num_fats as u64 * self.sectors_per_fat() as u64
    }

    /// FAT16/12: number of sectors in fixed root directory.
    pub fn fat16_root_sectors(&self) -> u64 {
        ((self.root_entry_count as u64 * 32)
            .saturating_add(self.bytes_per_sector as u64 - 1))
            / self.bytes_per_sector as u64
    }

    /// Return the "root cluster" for this volume:
    /// FAT32 → `root_cluster` field; FAT16/12 → 0 (sentinel for fixed root).
    pub fn root_cluster_for_(&self) -> u32 {
        if self.fat_type == FatType::Fat32 { self.root_cluster } else { 0 }
    }
}

// ── low-level helpers (endian, sector I/O) ────────────────────────────────

fn le_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

fn put_le_u16(b: &mut [u8], at: usize, v: u16) -> Result<(), &'static str> {
    let dst = b.get_mut(at..at + 2).ok_or("fat32: write u16 out of bounds")?;
    dst.copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn put_le_u32(b: &mut [u8], at: usize, v: u32) -> Result<(), &'static str> {
    let dst = b.get_mut(at..at + 4).ok_or("fat32: write u32 out of bounds")?;
    dst.copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn put_le_u8(b: &mut [u8], at: usize, v: u8) -> Result<(), &'static str> {
    *b.get_mut(at).ok_or("fat32: write u8 out of bounds")? = v;
    Ok(())
}

/// Read `count` sectors starting at `lba` (absolute) from a block device.
fn read_sectors(
    lba: u64,
    count: usize,
    sector_size: usize,
    io: &mut dyn BlockIo,
) -> Result<Vec<u8>, &'static str> {
    let mut buf = vec![0u8; count.saturating_mul(sector_size)];
    let mut scratch = vec![0u8; sector_size];
    for i in 0..count {
        io.read_sector(lba + i as u64, scratch.as_mut_slice())?;
        buf[i * sector_size..(i + 1) * sector_size].copy_from_slice(&scratch);
    }
    Ok(buf)
}

/// Write `data` (multiple of sector_size bytes) to consecutive sectors.
fn write_sectors(
    lba: u64,
    data: &[u8],
    sector_size: usize,
    io: &mut dyn BlockIo,
) -> Result<(), &'static str> {
    let count = data.len() / sector_size;
    for i in 0..count {
        let chunk = &data[i * sector_size..(i + 1) * sector_size];
        io.write_sector(lba + i as u64, chunk)?;
    }
    Ok(())
}

// ── probe / load superblock ───────────────────────────────────────────────

/// Probe a partition and return its parsed `Fat32Superblock`, or `None` if
/// the boot sector does not look like a FAT filesystem.
pub fn probe(
    part_start_lba: u64,
    sector_size: usize,
    io: &mut dyn BlockIo,
) -> Option<Fat32Superblock>
{
    let mut buf = vec![0u8; sector_size.max(512)];
    io.read_sector(part_start_lba, buf.as_mut_slice()).ok()?;

    // Must have boot signature.
    if buf.get(510) != Some(&0x55) || buf.get(511) != Some(&0xAA) {
        return None;
    }

    let bytes_per_sector = le_u16(&buf, 11)?;
    if bytes_per_sector == 0 || bytes_per_sector % 512 != 0 {
        return None;
    }
    let sectors_per_cluster = *buf.get(13)?;
    if sectors_per_cluster == 0 || (sectors_per_cluster & (sectors_per_cluster - 1)) != 0 {
        return None; // must be power of two
    }
    let reserved_sector_count = le_u16(&buf, 14)?;
    if reserved_sector_count == 0 {
        return None;
    }
    let num_fats = *buf.get(16)?;
    if num_fats == 0 {
        return None;
    }
    let root_entry_count = le_u16(&buf, 17)?;
    let total_sectors_16 = le_u16(&buf, 19)?;
    let fat_size_16 = le_u16(&buf, 22)?;
    let total_sectors_32 = le_u32(&buf, 32)?;
    let fat_size_32 = le_u32(&buf, 36)?;
    let root_cluster = le_u32(&buf, 44).unwrap_or(2);
    let fs_info_sector = le_u16(&buf, 48).unwrap_or(1);

    // Determine FAT type from cluster count (as per Microsoft FAT spec).
    let fat_size = if fat_size_16 != 0 { fat_size_16 as u64 } else { fat_size_32 as u64 };
    if fat_size == 0 {
        return None;
    }
    let root_dir_sectors = ((root_entry_count as u64 * 32)
        .saturating_add(bytes_per_sector as u64 - 1))
        / bytes_per_sector as u64;
    let total_sectors = if total_sectors_16 != 0 {
        total_sectors_16 as u64
    } else {
        total_sectors_32 as u64
    };
    let data_sectors = total_sectors
        .saturating_sub(reserved_sector_count as u64)
        .saturating_sub(num_fats as u64 * fat_size)
        .saturating_sub(root_dir_sectors);
    let count_of_clusters = data_sectors / sectors_per_cluster as u64;

    let fat_type = if count_of_clusters < 4085 {
        FatType::Fat12
    } else if count_of_clusters < 65525 {
        FatType::Fat16
    } else {
        FatType::Fat32
    };

    Some(Fat32Superblock {
        bytes_per_sector,
        sectors_per_cluster,
        reserved_sector_count,
        num_fats,
        root_entry_count,
        total_sectors_16,
        fat_size_16,
        total_sectors_32,
        fat_size_32,
        root_cluster,
        fs_info_sector,
        fat_type,
    })
}

// ── FAT table cache ───────────────────────────────────────────────────────

/// A per-volume FAT cache.  Holds the entire FAT1 table in memory when
/// first accessed.  At most ~2 MiB for a 512 MiB FAT32 volume.
#[derive(Clone)]
pub struct FatCache {
    /// Raw FAT sector data (FAT1).
    pub data: Vec<u8>,
    /// Whether any entry has been modified (dirty FAT needs writeback).
    pub dirty: bool,
}

impl FatCache {
    pub fn new() -> Self {
        Self { data: Vec::new(), dirty: false }
    }

    pub fn is_loaded(&self) -> bool {
        !self.data.is_empty()
    }

    pub fn load(
        &mut self,
        sb: &Fat32Superblock,
        part_start_lba: u64,
        io: &mut dyn BlockIo,
    ) -> Result<(), &'static str> {
        if self.is_loaded() {
            return Ok(());
        }
        let fat_sectors = sb.sectors_per_fat() as usize;
        let sector_size = sb.bytes_per_sector as usize;
        self.data = read_sectors(sb.fat1_start_sector() + part_start_lba, fat_sectors, sector_size, io)?;
        self.dirty = false;
        Ok(())
    }

    /// Flush FAT to both FAT1 and FAT2 on disk.
    pub fn flush(
        &mut self,
        sb: &Fat32Superblock,
        part_start_lba: u64,
        io: &mut dyn BlockIo,
    ) -> Result<(), &'static str> {
        if !self.dirty || self.data.is_empty() {
            return Ok(());
        }
        let sector_size = sb.bytes_per_sector as usize;
        write_sectors(sb.fat1_start_sector() + part_start_lba, &self.data, sector_size, io)?;
        if sb.num_fats >= 2 {
            write_sectors(sb.fat2_start_sector() + part_start_lba, &self.data, sector_size, io)?;
        }
        self.dirty = false;
        Ok(())
    }

    /// Read a FAT entry (returns the next-cluster value, masked to 28 bits for FAT32).
    pub fn get_entry(&self, sb: &Fat32Superblock, cluster: u32) -> Option<u32> {
        match sb.fat_type {
            FatType::Fat32 => {
                let offset = (cluster as usize) * 4;
                let raw = le_u32(&self.data, offset)?;
                Some(raw & FAT32_MASK)
            }
            FatType::Fat16 => {
                let offset = (cluster as usize) * 2;
                let raw = le_u16(&self.data, offset)? as u32;
                Some(raw)
            }
            FatType::Fat12 => {
                let offset = (cluster as usize) * 3 / 2;
                let lo = *self.data.get(offset)? as u32;
                let hi = *self.data.get(offset + 1)? as u32;
                let raw = (lo | (hi << 8)) as u32;
                if cluster & 1 == 0 {
                    Some(raw & 0x0FFF)
                } else {
                    Some((raw >> 4) & 0x0FFF)
                }
            }
        }
    }

    /// Write a FAT entry.
    pub fn set_entry(&mut self, sb: &Fat32Superblock, cluster: u32, value: u32) -> Result<(), &'static str> {
        match sb.fat_type {
            FatType::Fat32 => {
                let offset = (cluster as usize) * 4;
                // Preserve upper 4 reserved bits.
                let existing = le_u32(&self.data, offset).unwrap_or(0) & !FAT32_MASK;
                put_le_u32(&mut self.data, offset, existing | (value & FAT32_MASK))?;
            }
            FatType::Fat16 => {
                let offset = (cluster as usize) * 2;
                put_le_u16(&mut self.data, offset, (value & 0xFFFF) as u16)?;
            }
            FatType::Fat12 => {
                let offset = (cluster as usize) * 3 / 2;
                let existing_lo = *self.data.get(offset).unwrap_or(&0) as u32;
                let existing_hi = *self.data.get(offset + 1).unwrap_or(&0) as u32;
                let existing = (existing_lo | (existing_hi << 8)) as u32;
                let new_val = if cluster & 1 == 0 {
                    (existing & 0xF000) | (value & 0x0FFF)
                } else {
                    (existing & 0x000F) | ((value & 0x0FFF) << 4)
                };
                put_le_u8(&mut self.data, offset, (new_val & 0xFF) as u8)?;
                put_le_u8(&mut self.data, offset + 1, ((new_val >> 8) & 0xFF) as u8)?;
            }
        }
        self.dirty = true;
        Ok(())
    }

    pub fn is_eoc(&self, sb: &Fat32Superblock, value: u32) -> bool {
        match sb.fat_type {
            FatType::Fat32 => value >= (CLUSTER_EOC & FAT32_MASK),
            FatType::Fat16 => value >= 0xFFF8,
            FatType::Fat12 => value >= 0xFF8,
        }
    }

    pub fn is_free(&self, _sb: &Fat32Superblock, value: u32) -> bool {
        value == 0
    }

    pub fn eoc_value(&self, sb: &Fat32Superblock) -> u32 {
        match sb.fat_type {
            FatType::Fat32 => CLUSTER_EOC & FAT32_MASK,
            FatType::Fat16 => 0xFFFF,
            FatType::Fat12 => 0xFFF,
        }
    }

    pub fn max_cluster(&self, sb: &Fat32Superblock) -> u32 {
        match sb.fat_type {
            FatType::Fat32 => (self.data.len() / 4) as u32,
            FatType::Fat16 => (self.data.len() / 2) as u32,
            FatType::Fat12 => (self.data.len() * 2 / 3) as u32,
        }
    }
}

// ── cluster chain traversal ───────────────────────────────────────────────

/// Collect all clusters in the chain starting at `first_cluster`.
/// Returns at most `max_clusters` entries to prevent infinite loops on
/// corrupted FAT tables.
pub fn cluster_chain(
    fat: &FatCache,
    sb: &Fat32Superblock,
    first_cluster: u32,
) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut c = first_cluster;
    let max = fat.max_cluster(sb).max(2) as usize;
    while c >= 2 && chain.len() < max {
        chain.push(c);
        let next = match fat.get_entry(sb, c) {
            Some(n) => n,
            None => break,
        };
        if fat.is_eoc(sb, next) || fat.is_free(sb, next) {
            break;
        }
        c = next;
    }
    chain
}

/// Allocate `count` free clusters and link them into a chain.
/// Returns the first cluster, or `None` if not enough free clusters exist.
pub fn alloc_clusters(
    fat: &mut FatCache,
    sb: &Fat32Superblock,
    count: usize,
) -> Option<u32> {
    if count == 0 {
        return None;
    }
    let max = fat.max_cluster(sb) as usize;
    let mut allocated: Vec<u32> = Vec::with_capacity(count);
    for c in 2..max as u32 {
        if fat.get_entry(sb, c) == Some(0) {
            allocated.push(c);
            if allocated.len() == count {
                break;
            }
        }
    }
    if allocated.len() < count {
        return None;
    }
    let eoc = fat.eoc_value(sb);
    for i in 0..allocated.len() - 1 {
        fat.set_entry(sb, allocated[i], allocated[i + 1]).ok()?;
    }
    fat.set_entry(sb, *allocated.last()?, eoc).ok()?;
    Some(allocated[0])
}

/// Free the entire cluster chain starting at `first_cluster`.
pub fn free_chain(fat: &mut FatCache, sb: &Fat32Superblock, first_cluster: u32) {
    let chain = cluster_chain(fat, sb, first_cluster);
    for c in chain {
        let _ = fat.set_entry(sb, c, CLUSTER_FREE);
    }
}

// ── directory entry parsing ───────────────────────────────────────────────

/// A parsed directory entry (file or directory).
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub attr: u8,
    pub cluster: u32,
    pub file_size: u32,
    /// Absolute byte offset of the SFN (Short File Name) entry within the
    /// volume's data area (cluster-relative byte offset from `data_start_sector`).
    pub sfn_byte_offset: u64,
}

impl DirEntry {
    pub fn is_dir(&self) -> bool {
        self.attr & ATTR_DIRECTORY != 0
    }
    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }
}

/// Parse raw directory block bytes into `DirEntry` values.
/// `block_byte_offset` is the byte offset of the first byte of `data`
/// relative to the volume's `data_start_sector` (used to compute `sfn_byte_offset`).
pub fn parse_dir_entries(data: &[u8], block_byte_offset: u64) -> Vec<DirEntry> {
    let mut out = Vec::new();
    let mut pending_lfn: Vec<(u8, String)> = Vec::new();
    let mut i = 0usize;

    while i + DIR_ENTRY_SIZE <= data.len() {
        let entry = &data[i..i + DIR_ENTRY_SIZE];
        let first_byte = entry[0];

        if first_byte == DIR_ENTRY_END {
            break; // No more entries in this block.
        }

        if first_byte == DIR_ENTRY_DELETED {
            pending_lfn.clear();
            i += DIR_ENTRY_SIZE;
            continue;
        }

        let attr = entry[11];

        // LFN entry?
        if attr == ATTR_LFN {
            let ordinal = entry[0];
            if ordinal & 0x40 != 0 {
                // Last (highest-order) LFN entry – start collecting.
                pending_lfn.clear();
            }
            let seq = (ordinal & 0x1F) as u8;
            let chars = lfn_chars(entry);
            pending_lfn.push((seq, chars));
            i += DIR_ENTRY_SIZE;
            continue;
        }

        // Volume ID or system entries – skip.
        if attr & ATTR_VOLUME_ID != 0 {
            pending_lfn.clear();
            i += DIR_ENTRY_SIZE;
            continue;
        }

        // Short entry.
        let name = if !pending_lfn.is_empty() {
            // Reassemble from LFN entries (stored in reverse order by ordinal).
            pending_lfn.sort_by_key(|(ord, _)| *ord);
            let mut s = String::new();
            for (_, chunk) in &pending_lfn {
                s.push_str(chunk);
            }
            // Trim trailing NUL characters.
            s.trim_end_matches('\0').to_string()
        } else {
            // 8.3 name: first 8 bytes name + 3 bytes extension.
            sfn_to_str(entry)
        };

        let cluster_hi = le_u16(entry, 20).unwrap_or(0) as u32;
        let cluster_lo = le_u16(entry, 26).unwrap_or(0) as u32;
        let cluster = (cluster_hi << 16) | cluster_lo;
        let file_size = le_u32(entry, 28).unwrap_or(0);
        let sfn_byte_offset = block_byte_offset + i as u64;

        if !name.is_empty() && name != "." && name != ".." {
            out.push(DirEntry {
                name,
                attr,
                cluster,
                file_size,
                sfn_byte_offset,
            });
        }

        pending_lfn.clear();
        i += DIR_ENTRY_SIZE;
    }
    out
}

/// Decode the 13 UTF-16LE characters from a single LFN entry.
fn lfn_chars(entry: &[u8]) -> String {
    let mut chars = Vec::with_capacity(13);
    // chars 1-5 at offset 1 (5 UTF-16LE codeunits)
    for k in 0..5 {
        let lo = entry.get(1 + k * 2).copied().unwrap_or(0);
        let hi = entry.get(1 + k * 2 + 1).copied().unwrap_or(0);
        chars.push(u16::from_le_bytes([lo, hi]));
    }
    // chars 6-11 at offset 14 (6 UTF-16LE codeunits)
    for k in 0..6 {
        let lo = entry.get(14 + k * 2).copied().unwrap_or(0);
        let hi = entry.get(14 + k * 2 + 1).copied().unwrap_or(0);
        chars.push(u16::from_le_bytes([lo, hi]));
    }
    // chars 12-13 at offset 28 (2 UTF-16LE codeunits)
    for k in 0..2 {
        let lo = entry.get(28 + k * 2).copied().unwrap_or(0);
        let hi = entry.get(28 + k * 2 + 1).copied().unwrap_or(0);
        chars.push(u16::from_le_bytes([lo, hi]));
    }
    // Remove NUL and 0xFFFF terminators.
    let trimmed: Vec<u16> = chars.into_iter().take_while(|&c| c != 0 && c != 0xFFFF).collect();
    char::decode_utf16(trimmed.iter().copied())
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

/// Format an 8.3 short name (11 bytes) into a display string.
fn sfn_to_str(entry: &[u8]) -> String {
    let name_raw = &entry[..8];
    let ext_raw = &entry[8..11];

    let name: String = name_raw
        .iter()
        .take_while(|&&b| b != b' ')
        .map(|&b| b as char)
        .collect::<String>()
        .to_uppercase();

    let ext: String = ext_raw
        .iter()
        .take_while(|&&b| b != b' ')
        .map(|&b| b as char)
        .collect::<String>()
        .to_uppercase();

    if ext.is_empty() {
        name
    } else {
        alloc::format!("{}.{}", name, ext)
    }
}

// ── path-relative directory lookup ───────────────────────────────────────

/// Read all directory entries for a cluster chain starting at `first_cluster`.
pub fn read_dir(
    sb: &Fat32Superblock,
    fat: &FatCache,
    first_cluster: u32,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<Vec<DirEntry>, &'static str>
{
    let sector_size = sb.bytes_per_sector as usize;
    let cluster_sectors = sb.sectors_per_cluster as usize;
    let chain = cluster_chain(fat, sb, first_cluster);
    let mut entries = Vec::new();
    let data_start = sb.data_start_sector();

    for cluster in &chain {
        let cluster_lba = sb.cluster_to_sector(*cluster) + part_start_lba;
        let data = read_sectors(cluster_lba, cluster_sectors, sector_size, io)?;
        let byte_offset = (cluster_lba - data_start - part_start_lba) * sector_size as u64;
        let mut parsed = parse_dir_entries(&data, byte_offset);
        entries.append(&mut parsed);
    }
    Ok(entries)
}

/// Read the FAT16/12 fixed root directory (which has no cluster chain).
pub fn read_root_dir_fat16(
    sb: &Fat32Superblock,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<Vec<DirEntry>, &'static str>
{
    let sector_size = sb.bytes_per_sector as usize;
    let root_sectors = sb.fat16_root_sectors() as usize;
    let root_lba = sb.fat16_root_start_sector() + part_start_lba;
    let data = read_sectors(root_lba, root_sectors, sector_size, io)?;
    Ok(parse_dir_entries(&data, 0))
}

/// Resolve a path relative to the volume root.  Returns the `DirEntry` for
/// the last component, or `None` if not found.
pub fn lookup_path(
    sb: &Fat32Superblock,
    fat: &FatCache,
    path: &str,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<Option<DirEntry>, &'static str>
{
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if segments.is_empty() {
        // Root directory itself.
        return Ok(Some(DirEntry {
            name: "/".to_string(),
            attr: ATTR_DIRECTORY,
            cluster: root_cluster_for(sb),
            file_size: 0,
            sfn_byte_offset: 0,
        }));
    }

    let mut dir_cluster = root_cluster_for(sb);
    let mut result: Option<DirEntry> = None;

    for (idx, seg) in segments.iter().enumerate() {
        let entries = if sb.fat_type != FatType::Fat32 && idx == 0 && dir_cluster == 0 {
            read_root_dir_fat16(sb, part_start_lba, io)?
        } else {
            read_dir(sb, fat, dir_cluster, part_start_lba, io)?
        };
        let found = entries.into_iter().find(|e| e.name.eq_ignore_ascii_case(seg));
        match found {
            None => return Ok(None),
            Some(entry) => {
                if idx + 1 < segments.len() {
                    if !entry.is_dir() {
                        return Ok(None);
                    }
                    dir_cluster = entry.cluster;
                } else {
                    result = Some(entry);
                }
            }
        }
    }
    Ok(result)
}

fn root_cluster_for(sb: &Fat32Superblock) -> u32 {
    if sb.fat_type == FatType::Fat32 {
        sb.root_cluster
    } else {
        0 // signals fixed root dir
    }
}

// ── file read ─────────────────────────────────────────────────────────────

/// Read all bytes of a file given its starting cluster and `file_size`.
pub fn read_file(
    sb: &Fat32Superblock,
    fat: &FatCache,
    first_cluster: u32,
    file_size: u32,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<Vec<u8>, &'static str>
{
    if file_size == 0 {
        return Ok(Vec::new());
    }
    let sector_size = sb.bytes_per_sector as usize;
    let cluster_sectors = sb.sectors_per_cluster as usize;
    let chain = cluster_chain(fat, sb, first_cluster);
    let mut out = Vec::with_capacity(file_size as usize);

    for cluster in &chain {
        if out.len() >= file_size as usize { break; }
        let lba = sb.cluster_to_sector(*cluster) + part_start_lba;
        let data = read_sectors(lba, cluster_sectors, sector_size, io)?;
        let remaining = (file_size as usize).saturating_sub(out.len());
        let take = data.len().min(remaining);
        out.extend_from_slice(&data[..take]);
    }
    out.truncate(file_size as usize);
    Ok(out)
}

// ── directory read (names only) ───────────────────────────────────────────

/// Return all entry names in a directory (no `.` or `..`).
pub fn readdir(
    sb: &Fat32Superblock,
    fat: &FatCache,
    dir_cluster: u32,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<Vec<String>, &'static str>
{
    let entries = if sb.fat_type != FatType::Fat32 && dir_cluster == 0 {
        read_root_dir_fat16(sb, part_start_lba, io)?
    } else {
        read_dir(sb, fat, dir_cluster, part_start_lba, io)?
    };
    let mut names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

// ── file write ────────────────────────────────────────────────────────────

/// Overwrite or create a file's cluster chain with `data`.
/// `dir_cluster` is the cluster of the parent directory.
/// `entry_name` is the case-insensitive target filename.
///
/// Strategy:
///  1. Locate (or create) the 8.3 directory entry.
///  2. Free the old cluster chain (if any).
///  3. Allocate a new chain of exactly the right size.
///  4. Write data clusters.
///  5. Update directory entry (size + first cluster).
///  6. Flush FAT.
pub fn write_file(
    sb: &Fat32Superblock,
    fat: &mut FatCache,
    dir_cluster: u32,
    entry_name: &str,
    data: &[u8],
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<(), &'static str>
{
    let sector_size = sb.bytes_per_sector as usize;
    let cluster_bytes = sb.cluster_bytes() as usize;

    // Find the existing entry (if any).
    let existing = lookup_path(sb, fat, entry_name, part_start_lba, io)?;

    // Free old chain.
    if let Some(ref e) = existing {
        if e.cluster >= 2 {
            free_chain(fat, sb, e.cluster);
        }
    }

    // Allocate new chain.
    let clusters_needed = if data.is_empty() {
        0
    } else {
        data.len().div_ceil(cluster_bytes)
    };
    let first_cluster = if clusters_needed == 0 {
        0u32
    } else {
        alloc_clusters(fat, sb, clusters_needed)
            .ok_or("fat32: no free clusters for write")?
    };

    // Write data clusters.
    let chain = cluster_chain(fat, sb, first_cluster);
    for (idx, &cluster) in chain.iter().enumerate() {
        let lba = sb.cluster_to_sector(cluster) + part_start_lba;
        let start = idx * cluster_bytes;
        let end = (start + cluster_bytes).min(data.len());
        let mut buf = vec![0u8; cluster_bytes];
        if start < data.len() {
            buf[..end - start].copy_from_slice(&data[start..end]);
        }
        write_sectors(lba, &buf, sector_size, io)?;
    }

    // Write FAT.
    fat.flush(sb, part_start_lba, io)?;

    // Update directory entry.
    match existing {
        Some(ref e) => {
            update_dir_entry_cluster_size(
                sb, fat, e.sfn_byte_offset, first_cluster, data.len() as u32,
                part_start_lba, io,
            )?;
        }
        None => {
            create_dir_entry(
                sb, fat, dir_cluster, entry_name, first_cluster,
                data.len() as u32, false, part_start_lba, io,
            )?;
        }
    }
    Ok(())
}

/// Update the cluster and size fields of an existing SFN directory entry.
fn update_dir_entry_cluster_size(
    sb: &Fat32Superblock,
    _fat: &FatCache,
    sfn_byte_offset: u64,
    first_cluster: u32,
    file_size: u32,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<(), &'static str>
{
    let sector_size = sb.bytes_per_sector as usize;
    // sfn_byte_offset is relative to data_start_sector.
    let abs_byte = sb.data_start_sector() * sector_size as u64 + sfn_byte_offset;
    let sector_lba = abs_byte / sector_size as u64 + part_start_lba;
    let in_sector = (abs_byte % sector_size as u64) as usize;

    let mut buf = vec![0u8; sector_size];
    io.read_sector(sector_lba, buf.as_mut_slice())?;

    let e = buf.get_mut(in_sector..in_sector + 32).ok_or("fat32: sfn entry out of sector")?;
    put_le_u16(e, 20, ((first_cluster >> 16) & 0xFFFF) as u16)?;
    put_le_u16(e, 26, (first_cluster & 0xFFFF) as u16)?;
    put_le_u32(e, 28, file_size)?;

    io.write_sector(sector_lba, &buf)?;
    Ok(())
}

/// Append a new Short File Name (SFN) directory entry to a directory cluster chain.
pub fn create_dir_entry(
    sb: &Fat32Superblock,
    fat: &mut FatCache,
    dir_cluster: u32,
    name: &str,
    first_cluster: u32,
    file_size: u32,
    is_dir: bool,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<(), &'static str>
{
    let sector_size = sb.bytes_per_sector as usize;
    let cluster_sectors = sb.sectors_per_cluster as usize;
    let attr: u8 = if is_dir { ATTR_DIRECTORY } else { ATTR_ARCHIVE };

    let sfn = encode_sfn(name);
    let mut entry_bytes = [0u8; 32];
    entry_bytes[..11].copy_from_slice(&sfn);
    entry_bytes[11] = attr;
    // cluster hi/lo
    entry_bytes[20] = ((first_cluster >> 24) & 0xFF) as u8;
    entry_bytes[21] = ((first_cluster >> 16) & 0xFF) as u8;
    entry_bytes[26] = (first_cluster & 0xFF) as u8;
    entry_bytes[27] = ((first_cluster >> 8) & 0xFF) as u8;
    // file size (LE32)
    entry_bytes[28] = (file_size & 0xFF) as u8;
    entry_bytes[29] = ((file_size >> 8) & 0xFF) as u8;
    entry_bytes[30] = ((file_size >> 16) & 0xFF) as u8;
    entry_bytes[31] = ((file_size >> 24) & 0xFF) as u8;

    // Find a free slot in the directory cluster chain.
    let chain = cluster_chain(fat, sb, dir_cluster);
    for &cluster in &chain {
        let lba = sb.cluster_to_sector(cluster) + part_start_lba;
        let mut buf = read_sectors(lba, cluster_sectors, sector_size, io)?;
        let mut i = 0usize;
        while i + 32 <= buf.len() {
            let fb = buf[i];
            if fb == DIR_ENTRY_DELETED || fb == DIR_ENTRY_END {
                buf[i..i + 32].copy_from_slice(&entry_bytes);
                if fb == DIR_ENTRY_END && i + 64 <= buf.len() {
                    buf[i + 32] = DIR_ENTRY_END;
                }
                write_sectors(lba, &buf, sector_size, io)?;
                return Ok(());
            }
            i += 32;
        }
    }

    // No free slot: extend the directory chain.
    let new_cluster = alloc_clusters(fat, sb, 1).ok_or("fat32: no free cluster for directory expansion")?;
    if let Some(&last) = chain.last() {
        fat.set_entry(sb, last, new_cluster)?;
    }
    fat.flush(sb, part_start_lba, io)?;

    // Write the new cluster: entry + sentinel.
    let cluster_bytes = sb.cluster_bytes() as usize;
    let mut buf = vec![0u8; cluster_bytes];
    buf[..32].copy_from_slice(&entry_bytes);
    if buf.len() >= 64 { buf[32] = DIR_ENTRY_END; }
    write_sectors(sb.cluster_to_sector(new_cluster) + part_start_lba, &buf, sector_size, io)?;
    Ok(())
}

/// Encode a filename into the 11-byte SFN field (uppercase, padded with spaces).
/// Uses the first 8 characters as the name and up to 3 as the extension.
fn encode_sfn(name: &str) -> [u8; 11] {
    let mut sfn = [b' '; 11];
    let upper = name.to_uppercase();
    let (base, ext) = match upper.rfind('.') {
        Some(dot_pos) if dot_pos > 0 => (&upper[..dot_pos], &upper[dot_pos + 1..]),
        _ => (upper.as_str(), ""),
    };
    for (i, b) in base.bytes().take(8).enumerate() {
        sfn[i] = b;
    }
    for (i, b) in ext.bytes().take(3).enumerate() {
        sfn[8 + i] = b;
    }
    sfn
}

// ── delete ────────────────────────────────────────────────────────────────

/// Delete a file (mark its directory entry deleted, free its cluster chain).
pub fn delete_entry(
    sb: &Fat32Superblock,
    fat: &mut FatCache,
    sfn_byte_offset: u64,
    first_cluster: u32,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<(), &'static str>
{
    let sector_size = sb.bytes_per_sector as usize;
    let abs_byte = sb.data_start_sector() * sector_size as u64 + sfn_byte_offset;
    let sector_lba = abs_byte / sector_size as u64 + part_start_lba;
    let in_sector = (abs_byte % sector_size as u64) as usize;

    let mut buf = vec![0u8; sector_size];
    io.read_sector(sector_lba, buf.as_mut_slice())?;
    if let Some(slot) = buf.get_mut(in_sector) { *slot = DIR_ENTRY_DELETED; }
    io.write_sector(sector_lba, &buf)?;

    if first_cluster >= 2 {
        free_chain(fat, sb, first_cluster);
        fat.flush(sb, part_start_lba, io)?;
    }
    Ok(())
}

// ── directory create ──────────────────────────────────────────────────────

/// Create a new sub-directory inside `parent_cluster`.
pub fn create_directory(
    sb: &Fat32Superblock,
    fat: &mut FatCache,
    parent_cluster: u32,
    name: &str,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<(), &'static str>
{
    let sector_size = sb.bytes_per_sector as usize;
    let cluster_bytes = sb.cluster_bytes() as usize;

    // Allocate cluster for new directory.
    let new_cluster = alloc_clusters(fat, sb, 1).ok_or("fat32: no free cluster for new directory")?;
    fat.flush(sb, part_start_lba, io)?;

    // Write '.' and '..' entries.
    let mut buf = vec![0u8; cluster_bytes];

    let mut dot = [0u8; 32];
    dot[0..11].copy_from_slice(b".          ");
    dot[11] = ATTR_DIRECTORY;
    dot[26] = (new_cluster & 0xFF) as u8;
    dot[27] = ((new_cluster >> 8) & 0xFF) as u8;
    dot[20] = ((new_cluster >> 16) & 0xFF) as u8;
    dot[21] = ((new_cluster >> 24) & 0xFF) as u8;
    buf[0..32].copy_from_slice(&dot);

    let mut dotdot = [0u8; 32];
    dotdot[0..11].copy_from_slice(b"..         ");
    dotdot[11] = ATTR_DIRECTORY;
    dotdot[26] = (parent_cluster & 0xFF) as u8;
    dotdot[27] = ((parent_cluster >> 8) & 0xFF) as u8;
    dotdot[20] = ((parent_cluster >> 16) & 0xFF) as u8;
    dotdot[21] = ((parent_cluster >> 24) & 0xFF) as u8;
    buf[32..64].copy_from_slice(&dotdot);

    // End-of-directory sentinel.
    if buf.len() >= 96 {
        buf[64] = DIR_ENTRY_END;
    }

    write_sectors(sb.cluster_to_sector(new_cluster) + part_start_lba, &buf, sector_size, io)?;

    // Add entry in parent directory.
    create_dir_entry(sb, fat, parent_cluster, name, new_cluster, 0, true, part_start_lba, io)
}

// ── mkfs (format) ─────────────────────────────────────────────────────────

/// Format a partition as FAT32.
///
/// `total_sectors`: total sectors in the partition.
/// `sector_size`:   bytes per sector (must be 512 for FAT32).
pub fn format(
    part_start_lba: u64,
    total_sectors: u64,
    sector_size: usize,
    io: &mut dyn BlockIo,
) -> Result<Fat32Superblock, &'static str>
{
    if sector_size != 512 {
        return Err("fat32: mkfs requires 512 bytes/sector");
    }
    if total_sectors < 65536 {
        return Err("fat32: partition too small for FAT32");
    }

    // Choose sectors-per-cluster based on total size.
    let spc: u8 = match total_sectors {
        0..=524287 => 1,        // < 256 MB
        524288..=1048575 => 2,  // 256–512 MB
        1048576..=2097151 => 4, // 512 MB – 1 GB
        2097152..=4194303 => 8, // 1–2 GB
        _ => 16,                // > 2 GB
    };

    let reserved: u16 = 32;
    let num_fats: u8 = 2;
    let root_cluster: u32 = 2;

    // FAT size calculation: we need FAT sectors such that
    //   data_sectors >= 65525 (minimum for FAT32)
    // Iterate to convergence.
    let mut fat_size: u32 = {
        let data = total_sectors.saturating_sub(reserved as u64);
        let clusters = data / spc as u64;
        ((clusters * 4 + 511) / 512) as u32
    };
    // Refine once.
    let data_sectors = total_sectors
        .saturating_sub(reserved as u64)
        .saturating_sub(num_fats as u64 * fat_size as u64);
    let count_of_clusters = data_sectors / spc as u64;
    if count_of_clusters < 65525 {
        return Err("fat32: partition too small to satisfy FAT32 cluster count");
    }
    // Recompute fat_size with proper cluster count.
    fat_size = (((count_of_clusters + 2) * 4 + 511) / 512) as u32;

    let sb = Fat32Superblock {
        bytes_per_sector: 512,
        sectors_per_cluster: spc,
        reserved_sector_count: reserved,
        num_fats,
        root_entry_count: 0,
        total_sectors_16: 0,
        fat_size_16: 0,
        total_sectors_32: total_sectors as u32,
        fat_size_32: fat_size,
        root_cluster,
        fs_info_sector: 1,
        fat_type: FatType::Fat32,
    };

    // ── Write boot sector ─────────────────────────────────────────────────
    let mut boot = vec![0u8; 512];
    // jmp short + nop
    boot[0] = 0xEB; boot[1] = 0x58; boot[2] = 0x90;
    // OEM name
    boot[3..11].copy_from_slice(b"SAIOS   ");
    put_le_u16(&mut boot, 11, 512)?;        // bytes_per_sector
    put_le_u8(&mut boot, 13, spc)?;         // sectors_per_cluster
    put_le_u16(&mut boot, 14, reserved)?;   // reserved_sector_count
    put_le_u8(&mut boot, 16, num_fats)?;    // num_fats
    put_le_u16(&mut boot, 17, 0)?;          // root_entry_count (0 for FAT32)
    put_le_u16(&mut boot, 19, 0)?;          // total_sectors_16 (0 for large)
    boot[21] = 0xF8;                        // media type (fixed disk)
    put_le_u16(&mut boot, 22, 0)?;          // fat_size_16 (0 for FAT32)
    put_le_u16(&mut boot, 24, 63)?;         // sectors_per_track
    put_le_u16(&mut boot, 26, 255)?;        // number_of_heads
    put_le_u32(&mut boot, 28, 0)?;          // hidden_sectors
    put_le_u32(&mut boot, 32, total_sectors as u32)?; // total_sectors_32
    put_le_u32(&mut boot, 36, fat_size)?;   // fat_size_32
    put_le_u16(&mut boot, 40, 0)?;          // ext_flags
    put_le_u16(&mut boot, 42, 0)?;          // fs_version
    put_le_u32(&mut boot, 44, root_cluster)?; // root_cluster
    put_le_u16(&mut boot, 48, 1)?;          // fs_info_sector
    put_le_u16(&mut boot, 50, 6)?;          // backup boot sector
    put_le_u8(&mut boot, 64, 0x80)?;        // drive number
    put_le_u8(&mut boot, 66, 0x29)?;        // boot signature
    put_le_u32(&mut boot, 67, 0x00534149)?; // volume ID
    boot[71..82].copy_from_slice(b"SAIOS      ");
    boot[82..90].copy_from_slice(b"FAT32   ");
    boot[510] = 0x55; boot[511] = 0xAA;

    io.write_sector(part_start_lba, &boot)?;

    // ── Write FSInfo sector ───────────────────────────────────────────────
    let mut fsi = vec![0u8; 512];
    put_le_u32(&mut fsi, 0, FSINFO_LEAD_SIG)?;
    put_le_u32(&mut fsi, 484, FSINFO_STRUCT_SIG)?;
    // free count: total clusters minus root cluster
    let free = (count_of_clusters as u32).saturating_sub(1);
    put_le_u32(&mut fsi, 488, free)?;
    put_le_u32(&mut fsi, 492, root_cluster + 1)?; // next free hint
    put_le_u32(&mut fsi, 508, FSINFO_TRAIL_SIG >> 16)?;
    fsi[510] = 0x55; fsi[511] = 0xAA;
    io.write_sector(part_start_lba + 1, &fsi)?;

    // ── Write FAT1 and FAT2 ────────────────────────────────────────────────
    let fat_bytes = fat_size as usize * 512;
    let mut fat_data = vec![0u8; fat_bytes];
    fat_data[0] = 0xF8; fat_data[1] = 0xFF; fat_data[2] = 0xFF; fat_data[3] = 0x0F;
    fat_data[4] = 0xFF; fat_data[5] = 0xFF; fat_data[6] = 0xFF; fat_data[7] = 0x0F;
    fat_data[8] = 0xFF; fat_data[9] = 0xFF; fat_data[10] = 0xFF; fat_data[11] = 0x0F;

    write_sectors(sb.fat1_start_sector() + part_start_lba, &fat_data, 512, io)?;
    write_sectors(sb.fat2_start_sector() + part_start_lba, &fat_data, 512, io)?;

    // ── Write root directory cluster (zeroed) ─────────────────────────────
    let cluster_bytes = sb.cluster_bytes() as usize;
    let zero_cluster = vec![0u8; cluster_bytes];
    write_sectors(sb.cluster_to_sector(root_cluster) + part_start_lba, &zero_cluster, 512, io)?;

    Ok(sb)
}

// ── FSInfo update ─────────────────────────────────────────────────────────

/// Update the FSInfo sector with the current free cluster count and next
/// free hint.  Call after any FAT mutation that changes the free count.
pub fn update_fsinfo(
    sb: &Fat32Superblock,
    fat: &FatCache,
    part_start_lba: u64,
    io: &mut dyn BlockIo,
) -> Result<(), &'static str>
{
    if sb.fat_type != FatType::Fat32 || sb.fs_info_sector == 0 {
        return Ok(());
    }
    let sector_size = sb.bytes_per_sector as usize;
    let mut buf = vec![0u8; sector_size];
    io.read_sector(part_start_lba + sb.fs_info_sector as u64, buf.as_mut_slice())?;

    // Verify signatures.
    let lead = le_u32(&buf, 0).unwrap_or(0);
    let struc = le_u32(&buf, 484).unwrap_or(0);
    if lead != FSINFO_LEAD_SIG || struc != FSINFO_STRUCT_SIG {
        return Ok(()); // Not a valid FSInfo sector; skip.
    }

    // Count free clusters.
    let max = fat.max_cluster(sb) as u32;
    let mut free_count: u32 = 0;
    let mut next_free: u32 = 0;
    for c in 2..max {
        if fat.get_entry(sb, c) == Some(0) {
            if next_free == 0 {
                next_free = c;
            }
            free_count += 1;
        }
    }

    put_le_u32(&mut buf, 488, free_count)?;
    put_le_u32(&mut buf, 492, if next_free == 0 { 0xFFFFFFFF } else { next_free })?;
    io.write_sector(part_start_lba + sb.fs_info_sector as u64, &buf)?;
    Ok(())
}
