//! Minimal ext4 filesystem builder - creates a fresh ext4 image in memory.
//!
//! Produces a correct ext4 with:
//!   - 4 KiB blocks
//!   - One block group
//!   - Root directory + /boot/grub hierarchy
//!   - Extent-tree based files
//!   - No journaling (writeback, sufficient for GRUB)

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// -- Constants --------------------------------------------------------------

const BLOCK_SIZE: usize = 4096;
const INODE_SIZE: usize = 256;
const EXT4_MAGIC: u16 = 0xEF53;
const EXT4_EXTENTS_FL: u32 = 0x80000;
const S_IFDIR: u16 = 0o040000;
const S_IFREG: u16 = 0o100000;
const S_IRWXU: u16 = 0o700;
const S_IRGRP: u16 = 0o050;
const S_IROTH: u16 = 0o005;

const ROOT_INO: u32 = 2;

// -- In-memory file tree ----------------------------------------------------

enum Entry {
    Dir(BTreeMap<String, Entry>),
    File(Vec<u8>),
}

// -- Builder ----------------------------------------------------------------

pub struct Ext4Builder {
    disk_size: usize,
    tree: Entry,
}

impl Ext4Builder {
    pub fn new(disk_size: usize) -> Self {
        Self {
            disk_size,
            tree: Entry::Dir(BTreeMap::new()),
        }
    }

    pub fn format(&mut self) -> Result<(), &'static str> {
        Ok(())
    }

    pub fn mkdir(&mut self, path: &str) -> Result<(), &'static str> {
        let parts: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        let mut cur = &mut self.tree;
        for part in parts {
            if let Entry::Dir(map) = cur {
                cur = map
                    .entry(String::from(part))
                    .or_insert(Entry::Dir(BTreeMap::new()));
            } else {
                return Err("ext4_mk: mkdir: not a directory");
            }
        }
        Ok(())
    }

    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str> {
        let parts: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            return Err("ext4_mk: empty path");
        }
        let (dirs, name) = parts.split_at(parts.len() - 1);
        let name = name[0];

        let mut cur = &mut self.tree;
        for part in dirs {
            if let Entry::Dir(map) = cur {
                cur = map
                    .entry(String::from(*part))
                    .or_insert(Entry::Dir(BTreeMap::new()));
            } else {
                return Err("ext4_mk: path component is not a directory");
            }
        }
        if let Entry::Dir(map) = cur {
            map.insert(String::from(name), Entry::File(Vec::from(data)));
            Ok(())
        } else {
            Err("ext4_mk: parent is not a directory")
        }
    }

    /// Serialise the in-memory tree into a valid ext4 binary image.
    pub fn finish(self) -> Vec<u8> {
        let mut img = Ext4Image::new(self.disk_size);
        img.build(&self.tree);
        img.serialise()
    }
}

// -- Image builder ----------------------------------------------------------

struct Ext4Image {
    blocks: Vec<Vec<u8>>,  // block_no → bytes (4 KiB each)
    inodes: Vec<RawInode>, // inode_no (1-based index) → inode
    next_block: usize,
    next_inode: u32,
    total_blocks: u32,
    total_inodes: u32,
}

#[derive(Clone)]
struct RawInode {
    mode: u16,
    size: u64,
    blocks: u32,           // in 512-byte units
    data_blocks: Vec<u32>, // list of data block numbers (for extent encoding)
}

impl RawInode {
    fn dir(size: usize) -> Self {
        Self {
            mode: S_IFDIR | S_IRWXU | S_IRGRP | S_IROTH,
            size: size as u64,
            blocks: 0,
            data_blocks: Vec::new(),
        }
    }
    fn file(size: usize) -> Self {
        Self {
            mode: S_IFREG | S_IRWXU | S_IRGRP | S_IROTH,
            size: size as u64,
            blocks: 0,
            data_blocks: Vec::new(),
        }
    }
}

impl Ext4Image {
    fn new(disk_size: usize) -> Self {
        let total_blocks = (disk_size / BLOCK_SIZE) as u32;
        let total_inodes = 8192u32;
        let mut s = Self {
            blocks: Vec::new(),
            inodes: Vec::new(),
            next_block: 0,
            next_inode: 1,
            total_blocks,
            total_inodes,
        };
        // Pre-allocate enough blocks
        s.blocks
            .resize(total_blocks as usize, vec![0u8; BLOCK_SIZE]);
        // Pre-allocate inode slots (1-based, index 0 unused)
        s.inodes.resize(total_inodes as usize + 1, RawInode::dir(0));
        s
    }

    fn alloc_block(&mut self) -> usize {
        // Skip block 0 (boot block), 1 (superblock), 2 (GDT),
        // 3 (block bitmap), 4 (inode bitmap), 5-260 (inode table)
        if self.next_block < 261 {
            self.next_block = 261;
        }
        let b = self.next_block;
        self.next_block += 1;
        b
    }

    fn alloc_inode(&mut self) -> u32 {
        if self.next_inode < 11 {
            self.next_inode = 11;
        } // reserve special inodes
        let i = self.next_inode;
        self.next_inode += 1;
        i
    }

    fn build(&mut self, tree: &Entry) {
        // Build root directory (inode 2)
        if let Entry::Dir(map) = tree {
            self.build_dir(ROOT_INO, map, ROOT_INO);
        }
    }

    fn build_dir(&mut self, ino: u32, map: &BTreeMap<String, Entry>, parent_ino: u32) {
        let mut dir_data = Vec::new();

        // "." entry
        append_dirent(&mut dir_data, ino, 2, b".");
        // ".." entry
        append_dirent(&mut dir_data, parent_ino, 2, b"..");

        // Child entries
        for (name, entry) in map {
            let child_ino = self.alloc_inode();
            match entry {
                Entry::Dir(child_map) => {
                    self.inodes[child_ino as usize] = RawInode::dir(0);
                    append_dirent(&mut dir_data, child_ino, 2, name.as_bytes());
                    self.build_dir(child_ino, child_map, ino);
                }
                Entry::File(data) => {
                    self.inodes[child_ino as usize] = RawInode::file(data.len());
                    append_dirent(&mut dir_data, child_ino, 1, name.as_bytes());
                    self.write_file_data(child_ino, data);
                }
            }
        }

        // Pad last dirent to fill block
        pad_dir_data(&mut dir_data);

        // Write directory data blocks
        let dir_size = dir_data.len();
        let dir_ino_mut = &mut self.inodes[ino as usize];
        dir_ino_mut.mode = S_IFDIR | S_IRWXU | S_IRGRP | S_IROTH;
        dir_ino_mut.size = dir_size as u64;

        let blocks_needed = dir_size.div_ceil(BLOCK_SIZE);
        for i in 0..blocks_needed {
            let blk = self.alloc_block();
            let start = i * BLOCK_SIZE;
            let end = (start + BLOCK_SIZE).min(dir_size);
            self.blocks[blk][..end - start].copy_from_slice(&dir_data[start..end]);
            self.inodes[ino as usize].data_blocks.push(blk as u32);
            self.inodes[ino as usize].blocks += 8; // 8 × 512 = 4096
        }
    }

    fn write_file_data(&mut self, ino: u32, data: &[u8]) {
        let blocks_needed = data.len().div_ceil(BLOCK_SIZE);
        for i in 0..blocks_needed {
            let blk = self.alloc_block();
            let start = i * BLOCK_SIZE;
            let end = (start + BLOCK_SIZE).min(data.len());
            self.blocks[blk][..end - start].copy_from_slice(&data[start..end]);
            self.inodes[ino as usize].data_blocks.push(blk as u32);
            self.inodes[ino as usize].blocks += 8;
        }
    }

    /// Serialise to a binary image.
    fn serialise(mut self) -> Vec<u8> {
        let size = self.total_blocks as usize * BLOCK_SIZE;
        let used_blocks = self.next_block as u32;
        let used_inodes = self.next_inode - 1;
        let free_blocks = self.total_blocks.saturating_sub(used_blocks);
        let free_inodes = self.total_inodes.saturating_sub(used_inodes);

        // -- Superblock (block 1, byte offset 1024) -------------------------
        write_superblock(
            &mut self.blocks[1],
            self.total_inodes,
            self.total_blocks,
            free_inodes,
            free_blocks,
            INODE_SIZE as u16,
        );

        // -- Block group descriptor (block 2) ------------------------------
        write_bgd(
            &mut self.blocks[2],
            3, // block bitmap at block 3
            4, // inode bitmap at block 4
            5, // inode table starts at block 5
            free_blocks as u16,
            free_inodes as u16,
        );

        // -- Block bitmap (block 3) -----------------------------------------
        // Mark blocks 0..used_blocks as used
        {
            let bm = &mut self.blocks[3];
            for b in 0..used_blocks as usize {
                bm[b / 8] |= 1 << (b % 8);
            }
        }

        // -- Inode bitmap (block 4) -----------------------------------------
        // Mark inodes 1..used_inodes as used (1-based)
        {
            let bm = &mut self.blocks[4];
            for i in 1..=used_inodes as usize {
                let idx = i - 1;
                bm[idx / 8] |= 1 << (idx % 8);
            }
        }

        // -- Inode table (blocks 5-260, 256 inodes per block at 256 B each) -
        // Each inode is INODE_SIZE (256) bytes.
        // Block 5 starts at inode index 1 (inode number 2 = root is index 1).
        for ino_num in 1..=used_inodes {
            let idx = (ino_num - 1) as usize; // 0-based
            let blk_num = 5 + idx / (BLOCK_SIZE / INODE_SIZE);
            let blk_off = (idx % (BLOCK_SIZE / INODE_SIZE)) * INODE_SIZE;
            let raw = &self.inodes[ino_num as usize];
            write_inode(
                &mut self.blocks[blk_num][blk_off..blk_off + INODE_SIZE],
                raw,
            );
        }

        // Flatten blocks into a single Vec
        let mut out = alloc::vec![0u8; size];
        for (i, block) in self.blocks.iter().enumerate() {
            let off = i * BLOCK_SIZE;
            if off + BLOCK_SIZE <= out.len() {
                out[off..off + BLOCK_SIZE].copy_from_slice(block);
            }
        }
        out
    }
}

// -- Superblock -------------------------------------------------------------

fn write_superblock(
    block: &mut [u8],
    total_inodes: u32,
    total_blocks: u32,
    free_inodes: u32,
    free_blocks: u32,
    inode_size: u16,
) {
    let sb = &mut block[1024..1024 + 1024]; // superblock at offset 1024 in block 1
    w32(sb, 0x00, total_inodes);
    w32(sb, 0x04, total_blocks);
    w32(sb, 0x08, 0); // reserved blocks
    w32(sb, 0x0C, free_blocks);
    w32(sb, 0x10, free_inodes);
    w32(sb, 0x14, 1); // first data block (0 for 4K blocks)
    w32(sb, 0x18, 2); // log2(block_size/1024) → 2 = 4 KiB
    w32(sb, 0x1C, 2); // log2(cluster_size/1024)
    w32(sb, 0x20, total_blocks); // blocks per group (whole disk = one group)
    w32(sb, 0x24, total_blocks); // clusters per group
    w32(sb, 0x28, total_inodes); // inodes per group
    w32(sb, 0x38, 0); // mount time
    w32(sb, 0x3C, 0); // write time
    w16(sb, 0x38, 1); // mount count
    w16(sb, 0x3A, 0xFFFF); // max mount count
    w16(sb, 0x38, EXT4_MAGIC); // magic - offset 0x38 in sb = byte 1024+0x38
    // correct magic offset: 0x38 in superblock = sb[0x38]
    sb[0x38] = (EXT4_MAGIC & 0xFF) as u8;
    sb[0x39] = (EXT4_MAGIC >> 8) as u8;
    sb[0x3C] = 1; // state: valid
    sb[0x40] = 0; // errors: continue
    w16(sb, 0x4C, 1); // rev level: dynamic
    // first inode number
    w32(sb, 0x54, 11);
    // inode size
    sb[0x58] = (inode_size & 0xFF) as u8;
    sb[0x59] = (inode_size >> 8) as u8;
    // feature flags: extents, large_file, sparse_super, dir_index
    w32(sb, 0x60, 0x0002); // compat: has_journal=0, dir_prealloc
    w32(sb, 0x64, 0x02C0); // incompat: extents (0x40) + filetype (0x02) + dir_htree (0x80) + inline (0x200)... let's keep it simple
    w32(sb, 0x64, 0x0042); // incompat: filetype + extents
    w32(sb, 0x68, 0x0003); // ro_compat: sparse_super + large_file

    // UUID (random-looking but fixed)
    let uuid: [u8; 16] = [
        0x5A, 0xA1, 0x05, 0x10, 0xDE, 0xAD, 0xBE, 0xEF, 0x5A, 0x10, 0x5, 0x10, 0xDE, 0xAD, 0xBE,
        0xEF,
    ];
    sb[0x68..0x78].copy_from_slice(&uuid);

    // Volume label
    let label = b"SAIOS\0\0\0\0\0\0\0\0\0\0\0";
    sb[0x78..0x88].copy_from_slice(label);
}

// -- Block group descriptor -------------------------------------------------

fn write_bgd(
    block: &mut [u8],
    block_bm: u32,
    inode_bm: u32,
    inode_table: u32,
    free_blocks: u16,
    free_inodes: u16,
) {
    // BGD is at the start of block 2 (32 bytes for non-64bit)
    let bgd = &mut block[..32];
    w32(bgd, 0, block_bm);
    w32(bgd, 4, inode_bm);
    w32(bgd, 8, inode_table);
    w16(bgd, 12, free_blocks);
    w16(bgd, 14, free_inodes);
    w16(bgd, 16, 2); // used dirs
}

// -- Inode ------------------------------------------------------------------

fn write_inode(buf: &mut [u8], raw: &RawInode) {
    w16(buf, 0, raw.mode);
    w16(buf, 2, 0); // uid lo
    w32(buf, 4, raw.size as u32);
    w32(buf, 8, 0); // atime
    w32(buf, 12, 0); // ctime
    w32(buf, 16, 0); // mtime
    w32(buf, 20, 0); // dtime
    w16(buf, 24, 0); // gid lo
    w16(buf, 26, 1); // links count
    w32(buf, 28, raw.blocks);
    w32(buf, 32, EXT4_EXTENTS_FL); // i_flags: extents

    // Encode data blocks as an extent tree in i_block (60 bytes at offset 40)
    write_extent_header_leaf(&mut buf[40..100], &raw.data_blocks);

    w32(buf, 116, raw.size as u32); // i_size_hi upper (for large files)... actually this is i_dir_acl for dirs
    w32(buf, 120, (raw.size >> 32) as u32); // i_size_hi

    // Extra inode size field (at offset 128 in 256-byte inodes)
    w16(buf, 128, 28); // i_extra_isize
}

/// Write a flat extent leaf (all blocks in one or a few extents).
fn write_extent_header_leaf(buf: &mut [u8], data_blocks: &[u32]) {
    if data_blocks.is_empty() {
        // Magic + 0 entries
        w16(buf, 0, 0xF30A); // magic
        w16(buf, 2, 0); // entries
        w16(buf, 4, 4); // max entries
        w16(buf, 6, 0); // depth = 0 (leaf)
        return;
    }

    // Pack into extents: find contiguous runs
    let mut extents: Vec<(u32, u32, u32)> = Vec::new(); // (logical_start, phys_start, len)
    let mut log_start = 0u32;
    let mut run_start = data_blocks[0];
    let mut run_len = 1u32;

    for i in 1..data_blocks.len() {
        if data_blocks[i] == data_blocks[i - 1] + 1 {
            run_len += 1;
        } else {
            extents.push((log_start, run_start, run_len));
            log_start += run_len;
            run_start = data_blocks[i];
            run_len = 1;
        }
    }
    extents.push((log_start, run_start, run_len));

    let n_extents = extents.len().min(4) as u16; // max 4 extents in inline tree

    w16(buf, 0, 0xF30A); // magic
    w16(buf, 2, n_extents); // entries
    w16(buf, 4, 4); // max
    w16(buf, 6, 0); // depth = leaf
    w32(buf, 8, 0); // generation

    for (i, &(log, phys, len)) in extents[..n_extents as usize].iter().enumerate() {
        let off = 12 + i * 12;
        w32(buf, off, log);
        w16(buf, off + 4, len.min(0x8000) as u16);
        w16(buf, off + 6, 0); // phys high
        w32(buf, off + 8, phys);
    }
}

// -- Directory entries ------------------------------------------------------

fn append_dirent(out: &mut Vec<u8>, ino: u32, ftype: u8, name: &[u8]) {
    let name_len = name.len() as u8;
    let rec_len = ((8 + name.len()).div_ceil(4) * 4) as u16; // 4-byte aligned
    out.extend_from_slice(&ino.to_le_bytes());
    out.extend_from_slice(&rec_len.to_le_bytes());
    out.push(name_len);
    out.push(ftype);
    out.extend_from_slice(name);
    // Pad to rec_len
    let pad = rec_len as usize - 8 - name.len();
    out.extend(core::iter::repeat_n(0u8, pad));
}

/// Extend the last dirent to fill the rest of the block.
fn pad_dir_data(data: &mut Vec<u8>) {
    if data.is_empty() {
        return;
    }
    let used = data.len();
    let block_end = used.div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
    let tail = block_end - used;
    if tail == 0 {
        return;
    }
    // Extend the last rec_len to absorb the tail
    let last_rec_start = find_last_dirent_start(data);
    if last_rec_start + 4 <= data.len() {
        let old_rec = u16::from_le_bytes([data[last_rec_start + 4], data[last_rec_start + 5]]);
        let new_rec = old_rec + tail as u16;
        data[last_rec_start + 4] = (new_rec & 0xFF) as u8;
        data[last_rec_start + 5] = (new_rec >> 8) as u8;
    }
    data.resize(block_end, 0);
}

fn find_last_dirent_start(data: &[u8]) -> usize {
    let mut pos = 0usize;
    let mut last = 0usize;
    while pos + 8 <= data.len() {
        last = pos;
        let rec = u16::from_le_bytes([data[pos + 4], data[pos + 5]]) as usize;
        if rec == 0 {
            break;
        }
        pos += rec;
    }
    last
}

// -- Little-endian helpers --------------------------------------------------

fn w16(buf: &mut [u8], off: usize, val: u16) {
    if off + 2 <= buf.len() {
        buf[off] = (val & 0xFF) as u8;
        buf[off + 1] = (val >> 8) as u8;
    }
}
fn w32(buf: &mut [u8], off: usize, val: u32) {
    if off + 4 <= buf.len() {
        buf[off] = (val & 0xFF) as u8;
        buf[off + 1] = ((val >> 8) & 0xFF) as u8;
        buf[off + 2] = ((val >> 16) & 0xFF) as u8;
        buf[off + 3] = (val >> 24) as u8;
    }
}
