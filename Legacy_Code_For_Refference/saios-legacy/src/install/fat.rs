//! Minimal FAT16 filesystem builder — assembles a fresh FAT16 image in memory.
//!
//! Used to create an EFI System Partition (ESP) for UEFI install: a UEFI VM
//! boots `/EFI/BOOT/BOOTX64.EFI` from a FAT partition, so the SAIOS Live
//! Environment formats a FAT16 ESP containing the signed GRUB EFI, grub.cfg,
//! and the kernel.
//!
//! FAT16 (not 32) is chosen for simplicity: a fixed-size root directory and
//! 16-bit FAT entries, no FSInfo / cluster-chained root.  Layout (sectors,
//! relative to the partition start):
//!   [0]                  boot sector (BPB)
//!   [1 .. 1+F]           FAT #1            (F = fat_sectors)
//!   [1+F .. 1+2F]        FAT #2
//!   [1+2F .. +R]         root directory    (R = root_dir_sectors)
//!   [.. end]             data region (cluster 2 = first data cluster)
//!
//! Files/dirs are allocated *contiguous* cluster runs (a fresh image never
//! fragments), chained in both FATs, and referenced by 8.3 directory entries.

use crate::diag::watchdog;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const SECTOR: usize = 512;
const SPC: usize = 8; // sectors per cluster → 4 KiB clusters
// (keeps a ~32-48 MiB ESP comfortably inside
//  the FAT16 4085..65524-cluster range)
const CLUSTER: usize = SPC * SECTOR; // 16384
const RESERVED: usize = 1;
const NUM_FATS: usize = 2;
const ROOT_ENTRIES: usize = 512;
const ROOT_DIR_SECTORS: usize = (ROOT_ENTRIES * 32).div_ceil(SECTOR); // 32

const ATTR_DIR: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;

// -- In-memory tree (same shape as ext4_mk) ----------------------------------

enum Entry {
    Dir(BTreeMap<String, Entry>),
    File(Vec<u8>),
}

pub struct FatBuilder {
    size: usize,
    tree: Entry,
}

impl FatBuilder {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            tree: Entry::Dir(BTreeMap::new()),
        }
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), &'static str> {
        let parts = split(path);
        let mut cur = &mut self.tree;
        for p in parts {
            if let Entry::Dir(map) = cur {
                cur = map
                    .entry(String::from(p))
                    .or_insert_with(|| Entry::Dir(BTreeMap::new()));
            } else {
                return Err("fat: mkdir: not a directory");
            }
        }
        Ok(())
    }

    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str> {
        let parts = split(path);
        if parts.is_empty() {
            return Err("fat: empty path");
        }
        let (dirs, name) = parts.split_at(parts.len() - 1);
        let mut cur = &mut self.tree;
        for p in dirs {
            if let Entry::Dir(map) = cur {
                cur = map
                    .entry(String::from(*p))
                    .or_insert_with(|| Entry::Dir(BTreeMap::new()));
            } else {
                return Err("fat: path component not a directory");
            }
        }
        if let Entry::Dir(map) = cur {
            map.insert(String::from(name[0]), Entry::File(Vec::from(data)));
            Ok(())
        } else {
            Err("fat: parent not a directory")
        }
    }

    /// Serialise to a FAT16 image of `self.size` bytes.
    pub fn finish(self) -> Result<Vec<u8>, &'static str> {
        let total_sectors = self.size / SECTOR;
        // Iterate fat_sectors to a fixed point (it depends on cluster count,
        // which depends on fat_sectors).
        let mut fat_sectors = 1usize;
        for _ in 0..8 {
            let data_sectors =
                total_sectors.saturating_sub(RESERVED + NUM_FATS * fat_sectors + ROOT_DIR_SECTORS);
            let clusters = data_sectors / SPC;
            let need = ((clusters + 2) * 2).div_ceil(SECTOR);
            if need == fat_sectors {
                break;
            }
            fat_sectors = need;
        }
        let data_start = RESERVED + NUM_FATS * fat_sectors + ROOT_DIR_SECTORS;
        let total_clusters = (total_sectors - data_start) / SPC;
        if !(4085..=65524).contains(&total_clusters) {
            return Err("fat: cluster count out of FAT16 range — adjust ESP size");
        }

        let mut img = FatImage {
            data: vec![0u8; self.size],
            fat: vec![0u16; total_clusters + 2],
            fat_offset: RESERVED * SECTOR,
            fat2_offset: (RESERVED + fat_sectors) * SECTOR,
            root_offset: (RESERVED + NUM_FATS * fat_sectors) * SECTOR,
            data_offset: data_start * SECTOR,
            next_cluster: 2,
            total_clusters,
        };

        // Boot sector / BPB
        write_bpb(&mut img.data, total_sectors as u32, fat_sectors as u16);

        // Reserved FAT entries
        img.fat[0] = 0xFFF8;
        img.fat[1] = 0xFFFF;

        // Build the root directory and everything beneath it.
        if let Entry::Dir(map) = &self.tree {
            let root_entries = img.build_dir_entries(map, 0 /* root has no cluster */)?;
            img.write_root_dir(&root_entries);
            watchdog::note_progress(); // Progress after directory building
        }

        // Flush the FAT (both copies)
        img.flush_fat();
        watchdog::note_progress(); // Progress after FAT flush
        Ok(img.data)
    }
}

// -- Image serialiser ---------------------------------------------------------

struct FatImage {
    data: Vec<u8>,
    fat: Vec<u16>,
    fat_offset: usize,
    fat2_offset: usize,
    root_offset: usize,
    data_offset: usize,
    next_cluster: u16,
    total_clusters: usize,
}

impl FatImage {
    /// Allocate `n` contiguous clusters, chain them, return the first cluster.
    fn alloc_clusters(&mut self, n: usize) -> Result<u16, &'static str> {
        if n == 0 {
            return Ok(0);
        }
        let first = self.next_cluster;
        if (first as usize - 2) + n > self.total_clusters {
            return Err("fat: out of clusters (ESP too small)");
        }
        for i in 0..n {
            let c = first + i as u16;
            self.fat[c as usize] = if i + 1 < n { c + 1 } else { 0xFFFF };
        }
        self.next_cluster += n as u16;
        Ok(first)
    }

    fn cluster_byte_offset(&self, cluster: u16) -> usize {
        self.data_offset + (cluster as usize - 2) * CLUSTER
    }

    /// Write raw bytes spanning a contiguous cluster run starting at `first`.
    fn write_clusters(&mut self, first: u16, bytes: &[u8]) {
        let off = self.cluster_byte_offset(first);
        let end = (off + bytes.len()).min(self.data.len());
        self.data[off..end].copy_from_slice(&bytes[..end - off]);
    }

    /// Recursively materialise a directory's children (allocating clusters and
    /// writing their data), returning this directory's own packed 32-byte
    /// entries (WITHOUT "." / ".." — the caller adds those for subdirs).
    fn build_dir_entries(
        &mut self,
        map: &BTreeMap<String, Entry>,
        _self_cluster: u16,
    ) -> Result<Vec<u8>, &'static str> {
        let mut entries: Vec<u8> = Vec::new();
        for (name, entry) in map {
            match entry {
                Entry::File(data) => {
                    let nclust = data.len().div_ceil(CLUSTER);
                    let start = if nclust > 0 {
                        self.alloc_clusters(nclust)?
                    } else {
                        0
                    };
                    if nclust > 0 {
                        self.write_clusters(start, data);
                    }
                    push_dirent(&mut entries, name, ATTR_ARCHIVE, start, data.len() as u32);
                }
                Entry::Dir(child) => {
                    // Allocate one cluster for this subdirectory's entries.
                    let dir_cluster = self.alloc_clusters(1)?;
                    push_dirent(&mut entries, name, ATTR_DIR, dir_cluster, 0);
                    // Build children first (they allocate their own clusters).
                    let child_entries = self.build_dir_entries(child, dir_cluster)?;
                    // Compose this dir's cluster: "." , ".." , children.
                    let mut dir_data: Vec<u8> = Vec::new();
                    push_dot_entries(
                        &mut dir_data,
                        dir_cluster,
                        0, /* parent fixup below not needed for FAT */
                    );
                    dir_data.extend_from_slice(&child_entries);
                    self.write_clusters(dir_cluster, &dir_data);
                }
            }
        }
        Ok(entries)
    }

    fn write_root_dir(&mut self, entries: &[u8]) {
        let n = entries.len().min(ROOT_DIR_SECTORS * SECTOR);
        self.data[self.root_offset..self.root_offset + n].copy_from_slice(&entries[..n]);
    }

    fn flush_fat(&mut self) {
        for (i, &v) in self.fat.iter().enumerate() {
            let b = v.to_le_bytes();
            let o1 = self.fat_offset + i * 2;
            let o2 = self.fat2_offset + i * 2;
            if o1 + 2 <= self.data.len() {
                self.data[o1] = b[0];
                self.data[o1 + 1] = b[1];
            }
            if o2 + 2 <= self.data.len() {
                self.data[o2] = b[0];
                self.data[o2 + 1] = b[1];
            }
        }
    }
}

// -- BPB ----------------------------------------------------------------------

fn write_bpb(img: &mut [u8], total_sectors: u32, fat_sectors: u16) {
    let b = &mut img[..512];
    b[0] = 0xEB;
    b[1] = 0x3C;
    b[2] = 0x90; // jmp
    b[3..11].copy_from_slice(b"SAIOS   "); // OEM
    le16(b, 0x0B, SECTOR as u16); // bytes/sector
    b[0x0D] = SPC as u8; // sectors/cluster
    le16(b, 0x0E, RESERVED as u16); // reserved sectors
    b[0x10] = NUM_FATS as u8; // num FATs
    le16(b, 0x11, ROOT_ENTRIES as u16); // root entries
    le16(b, 0x13, 0); // total sectors 16 (use 32)
    b[0x15] = 0xF8; // media descriptor
    le16(b, 0x16, fat_sectors); // sectors/FAT (16)
    le16(b, 0x18, 32); // sectors/track
    le16(b, 0x1A, 64); // heads
    le32(b, 0x1C, 2048); // hidden sectors (part LBA)
    le32(b, 0x20, total_sectors); // total sectors 32
    b[0x24] = 0x80; // drive number
    b[0x26] = 0x29; // extended boot sig
    le32(b, 0x27, 0x5A105A10); // volume id
    b[0x2B..0x36].copy_from_slice(b"SAIOS BOOT "); // volume label (11)
    b[0x36..0x3E].copy_from_slice(b"FAT16   "); // fs type (8)
    b[510] = 0x55;
    b[511] = 0xAA; // boot signature
}

// -- Directory entries (8.3) ---------------------------------------------------

/// Append a 32-byte directory entry for `name` (converted to 8.3 uppercase).
fn push_dirent(out: &mut Vec<u8>, name: &str, attr: u8, cluster: u16, size: u32) {
    let mut e = [0u8; 32];
    fill_83(&mut e[..11], name);
    e[11] = attr;
    le16(&mut e, 26, cluster); // first cluster low (FAT16 has no high word)
    le32(&mut e, 28, size);
    out.extend_from_slice(&e);
}

/// Write the "." and ".." entries at the start of a subdirectory cluster.
fn push_dot_entries(out: &mut Vec<u8>, self_cluster: u16, parent_cluster: u16) {
    let mut dot = [0u8; 32];
    dot[..11].copy_from_slice(b".          ");
    dot[11] = ATTR_DIR;
    le16(&mut dot, 26, self_cluster);
    out.extend_from_slice(&dot);

    let mut dotdot = [0u8; 32];
    dotdot[..11].copy_from_slice(b"..         ");
    dotdot[11] = ATTR_DIR;
    le16(&mut dotdot, 26, parent_cluster); // 0 => root (correct for FAT)
    out.extend_from_slice(&dotdot);
}

/// Convert a name to an 11-byte 8.3 field (uppercased, space-padded).
fn fill_83(out: &mut [u8], name: &str) {
    for b in out.iter_mut() {
        *b = b' ';
    }
    let upper = name.to_ascii_uppercase();
    let (stem, ext) = match upper.rsplit_once('.') {
        Some((s, e)) => (s, e),
        None => (upper.as_str(), ""),
    };
    for (i, c) in stem.bytes().take(8).enumerate() {
        out[i] = c;
    }
    for (i, c) in ext.bytes().take(3).enumerate() {
        out[8 + i] = c;
    }
}

// -- helpers -------------------------------------------------------------------

fn split(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}
fn le16(b: &mut [u8], off: usize, v: u16) {
    b[off] = v as u8;
    b[off + 1] = (v >> 8) as u8;
}
fn le32(b: &mut [u8], off: usize, v: u32) {
    b[off] = v as u8;
    b[off + 1] = (v >> 8) as u8;
    b[off + 2] = (v >> 16) as u8;
    b[off + 3] = (v >> 24) as u8;
}
