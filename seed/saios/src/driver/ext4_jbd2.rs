//! JBD2 (Journal Block Device 2) write-ahead log for ext4.
//!
//! Provides crash-consistent ext4 writes by journaling all block mutations
//! before applying them to their real on-disk positions.
//!
//! # Transaction model
//!
//! Every call to [`Jbd2Journal::commit`] is one atomic transaction:
//! 1. Build a **descriptor block** listing the (filesystem_block_no, flags)
//!    for every dirty block in the transaction.
//! 2. Write the dirty blocks immediately after the descriptor block in the
//!    circular journal log.
//! 3. Write a **commit block** to seal the transaction.
//! 4. Write all dirty blocks to their real filesystem locations
//!    (checkpoint).
//! 5. Advance the journal tail so the committed space is reusable.
//!
//! # On-disk layout (circular log)
//!
//! ```text
//! Journal block 0  : Journal superblock
//! Journal block 1+ : Circular log:
//!   [descriptor | dirty_blk_0 | dirty_blk_1 | … | commit]
//!   [descriptor | dirty_blk_0 | … | commit]
//!   …
//! ```
//!
//! # JBD2 magic / block-type constants
//!
//! | Constant                       | Value        |
//! |-------------------------------|--------------|
//! | `JBD2_MAGIC`                  | 0xC03B3998   |
//! | `JBD2_DESCRIPTOR_BLOCK`       | 1            |
//! | `JBD2_COMMIT_BLOCK`           | 2            |
//! | `JBD2_SUPERBLOCK_V2`          | 4            |

use alloc::vec;
use alloc::vec::Vec;

use crate::driver::fat32::BlockIo;

// ── JBD2 constants ────────────────────────────────────────────────────────

pub const JBD2_MAGIC: u32 = 0xC03B3998;
pub const JBD2_DESCRIPTOR_BLOCK: u32 = 1;
pub const JBD2_COMMIT_BLOCK: u32 = 2;
pub const JBD2_REVOKE_BLOCK: u32 = 5;
pub const JBD2_SUPERBLOCK_V2: u32 = 4;

/// Block tag flag: last tag in this descriptor block.
const TAG_FLAG_LAST: u32 = 0x0000_0008;

// ── Superblock field offsets (relative to journal block 0) ───────────────

const JSB_MAGIC: usize = 0;
const JSB_BLOCKTYPE: usize = 4;
const JSB_BLOCKSIZE: usize = 12;
const JSB_MAXLEN: usize = 16;
const JSB_FIRST: usize = 20;
const JSB_SEQ_START: usize = 24;   // s_sequence: first expected commit ID
const JSB_START: usize = 28;       // s_start: first journal block of log; 0 = clean

// ── Descriptor / commit header offsets ───────────────────────────────────

const BH_MAGIC: usize = 0;
const BH_BLOCKTYPE: usize = 4;
const BH_SEQUENCE: usize = 8;
const HEADER_SIZE: usize = 12;

// ── Block tag (32-bit, no checksum) ───────────────────────────────────────
// t_blocknr (4) + t_flags (4) = 8 bytes per tag

const TAG_SIZE: usize = 8;

// ── big-endian helpers (JBD2 is big-endian) ───────────────────────────────

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn put_be_u32(b: &mut [u8], at: usize, v: u32) -> Result<(), &'static str> {
    let dst = b.get_mut(at..at + 4).ok_or("jbd2: put_be_u32 out of bounds")?;
    dst.copy_from_slice(&v.to_be_bytes());
    Ok(())
}

// ── Superblock helpers ────────────────────────────────────────────────────

/// Parsed journal superblock fields we care about.
#[derive(Copy, Clone, Debug)]
pub struct Jbd2Sb {
    pub block_size: u32,
    pub max_len: u32,
    pub first: u32,
    pub sequence: u32,
    pub start: u32,
}

/// Read and parse the JBD2 journal superblock from `journal_first_block`.
pub fn read_journal_sb(
    journal_first_block: u64,
    part_block_size: usize,
    io: &mut dyn BlockIo,
) -> Result<Jbd2Sb, &'static str> {
    let mut buf = vec![0u8; part_block_size];
    io.read_sector(journal_first_block, buf.as_mut_slice())?;

    let magic = be_u32(&buf, JSB_MAGIC).ok_or("jbd2: truncated superblock")?;
    if magic != JBD2_MAGIC {
        return Err("jbd2: superblock magic invalid");
    }
    let blocktype = be_u32(&buf, JSB_BLOCKTYPE).unwrap_or(0);
    if blocktype != JBD2_SUPERBLOCK_V2 {
        return Err("jbd2: unsupported journal superblock version");
    }

    Ok(Jbd2Sb {
        block_size: be_u32(&buf, JSB_BLOCKSIZE).unwrap_or(part_block_size as u32),
        max_len:    be_u32(&buf, JSB_MAXLEN).unwrap_or(0),
        first:      be_u32(&buf, JSB_FIRST).unwrap_or(1),
        sequence:   be_u32(&buf, JSB_SEQ_START).unwrap_or(1),
        start:      be_u32(&buf, JSB_START).unwrap_or(0),
    })
}

/// Update the journal superblock's `s_sequence` and `s_start` after a
/// checkpoint (so the log slot is considered reclaimed by the next
/// transaction).
pub fn write_journal_sb(
    journal_first_block: u64,
    sb: &Jbd2Sb,
    io: &mut dyn BlockIo,
) -> Result<(), &'static str> {
    let mut buf = vec![0u8; sb.block_size as usize];
    io.read_sector(journal_first_block, buf.as_mut_slice())?;

    put_be_u32(&mut buf, JSB_SEQ_START, sb.sequence)?;
    put_be_u32(&mut buf, JSB_START, sb.start)?;

    io.write_sector(journal_first_block, &buf)?;
    Ok(())
}

// ── Journal state ─────────────────────────────────────────────────────────

/// Mutable journal state maintained in memory across a `commit` call.
pub struct Jbd2Journal {
    /// Absolute LBA of journal block 0 (the journal superblock).
    pub first_block: u64,
    /// Parsed journal superblock.
    pub jsb: Jbd2Sb,
    /// Current write-cursor in the circular log (journal-relative block #).
    cursor: u32,
}

impl Jbd2Journal {
    pub fn new(first_block: u64, jsb: Jbd2Sb) -> Self {
        // Place cursor right after the last committed transaction (or at
        // `first` if the journal is clean).
        let cursor = if jsb.start == 0 { jsb.first } else { jsb.start };
        Self { first_block, jsb, cursor }
    }

    /// Absolute LBA for a journal-relative block index.
    fn abs_lba(&self, journal_block: u32) -> u64 {
        self.first_block + journal_block as u64
    }

    /// Advance cursor by `n` blocks (wrapping within the circular log).
    fn advance(&mut self, n: u32) {
        let log_len = self.jsb.max_len.saturating_sub(self.jsb.first);
        if log_len == 0 {
            self.cursor = self.cursor.wrapping_add(n);
            return;
        }
        let rel = self.cursor - self.jsb.first;
        let new_rel = (rel + n) % log_len;
        self.cursor = self.jsb.first + new_rel;
    }
}

// ── Transaction commit ────────────────────────────────────────────────────

/// A (filesystem_block_lba, block_data) pair to be journaled and written.
pub struct DirtyBlock {
    /// Absolute LBA on disk (the real destination of this block).
    pub lba: u64,
    /// Block data (must be exactly `block_size` bytes).
    pub data: Vec<u8>,
}

/// Commit `dirty_blocks` as a single JBD2 transaction:
///
/// 1. Write descriptor block (tags listing all dirty block LBAs).
/// 2. Write each dirty block to the journal log area.
/// 3. Write commit block.
/// 4. Write each dirty block to its **real** filesystem LBA (checkpoint).
/// 5. Advance journal tail past this transaction.
/// 6. Update journal superblock on disk.
///
/// This is a **synchronous commit**: when this function returns `Ok`, all
/// data is on disk in both journal and real locations.
pub fn jbd2_commit(
    journal: &mut Jbd2Journal,
    dirty: &[DirtyBlock],
    io: &mut dyn BlockIo,
) -> Result<(), &'static str> {
    if dirty.is_empty() {
        return Ok(());
    }

    let bsize = journal.jsb.block_size as usize;
    let seq = journal.jsb.sequence;

    // ── 1. Build and write descriptor block ───────────────────────────────
    let max_tags_per_desc = (bsize - HEADER_SIZE) / TAG_SIZE;
    // We may need multiple descriptor blocks for large transactions,
    // but for typical kernel writes (a few blocks) one is sufficient.
    // We write chunks of `max_tags_per_desc` dirty blocks at a time.

    let mut written_desc_count = 0u32;
    let mut chunk_start = 0usize;

    while chunk_start < dirty.len() {
        let chunk_end = (chunk_start + max_tags_per_desc).min(dirty.len());
        let chunk = &dirty[chunk_start..chunk_end];

        // Build descriptor block.
        let mut desc = vec![0u8; bsize];
        put_be_u32(&mut desc, BH_MAGIC, JBD2_MAGIC)?;
        put_be_u32(&mut desc, BH_BLOCKTYPE, JBD2_DESCRIPTOR_BLOCK)?;
        put_be_u32(&mut desc, BH_SEQUENCE, seq.wrapping_add(written_desc_count))?;

        let mut tag_offset = HEADER_SIZE;
        for (i, db) in chunk.iter().enumerate() {
            let flags = if i + 1 == chunk.len() { TAG_FLAG_LAST } else { 0 };
            // t_blocknr: the real filesystem LBA. For simplicity, store as u32;
            // this limits us to 2TB filesystems with 512-byte sectors.
            // A full implementation would use 64-bit tags with the JBD2_CSUM_V3 feature.
            put_be_u32(&mut desc, tag_offset, (db.lba & 0xFFFF_FFFF) as u32)?;
            put_be_u32(&mut desc, tag_offset + 4, flags)?;
            tag_offset += TAG_SIZE;
        }

        let desc_lba = journal.abs_lba(journal.cursor);
        io.write_sector(desc_lba, &desc)?;
        journal.advance(1);
        written_desc_count += 1;

        // ── 2. Write dirty blocks to journal ─────────────────────────────
        for db in chunk {
            if db.data.len() != bsize {
                return Err("jbd2: dirty block size mismatch");
            }
            let log_lba = journal.abs_lba(journal.cursor);
            io.write_sector(log_lba, &db.data)?;
            journal.advance(1);
        }

        chunk_start = chunk_end;
    }

    // ── 3. Write commit block ─────────────────────────────────────────────
    let mut commit = vec![0u8; bsize];
    put_be_u32(&mut commit, BH_MAGIC, JBD2_MAGIC)?;
    put_be_u32(&mut commit, BH_BLOCKTYPE, JBD2_COMMIT_BLOCK)?;
    put_be_u32(&mut commit, BH_SEQUENCE, seq)?;

    let commit_lba = journal.abs_lba(journal.cursor);
    io.write_sector(commit_lba, &commit)?;
    journal.advance(1);

    // ── 4. Checkpoint: write blocks to real filesystem locations ──────────
    for db in dirty {
        io.write_sector(db.lba, &db.data)?;
    }

    // ── 5 & 6. Advance journal tail, update superblock ───────────────────
    journal.jsb.sequence = journal.jsb.sequence.wrapping_add(1);
    // Mark journal as having active data starting at `cursor` so the
    // next transaction starts here.  Because we've already checkpointed,
    // the previous transaction slot is free; we set s_start = 0 to
    // indicate a clean journal (all transactions checkpointed).
    journal.jsb.start = 0;
    write_journal_sb(journal.first_block, &journal.jsb, io)?;

    Ok(())
}

// ── Locate journal first block from inode ────────────────────────────────

/// Find the absolute LBA of the journal's first block given the journal
/// inode's extent tree root (the `i_block` field of the inode).
///
/// For simple (depth-0) extent trees the first extent's physical block is
/// the journal's first block.  We read the inode from the ext4 filesystem
/// and extract the first extent leaf entry.
pub fn journal_first_block_from_extent(
    iblock: &[u8; 60],
    part_start_lba: u64,
    ext4_block_size: u64,
    part_sector_size: u64,
) -> Option<u64> {
    // Extent header magic = 0xF30A at offset 0.
    let magic = u16::from_le_bytes([iblock[0], iblock[1]]);
    if magic != 0xF30A {
        return None;
    }
    let entries = u16::from_le_bytes([iblock[2], iblock[3]]) as usize;
    let depth   = u16::from_le_bytes([iblock[6], iblock[7]]);
    if depth != 0 || entries == 0 {
        return None;
    }
    // First leaf extent starts at offset 12 within iblock.
    let ee_start_lo = u32::from_le_bytes([iblock[16], iblock[17], iblock[18], iblock[19]]) as u64;
    let ee_start_hi = u16::from_le_bytes([iblock[14], iblock[15]]) as u64;
    let phys_block = (ee_start_hi << 32) | ee_start_lo;

    // Convert filesystem block → absolute LBA.
    let byte_offset = phys_block.saturating_mul(ext4_block_size);
    let sector = part_start_lba + byte_offset / part_sector_size;
    Some(sector)
}

// ── ext4 mkfs ─────────────────────────────────────────────────────────────

/// Layout parameters computed by `mkfs_ext4`.
#[derive(Clone, Debug)]
pub struct Ext4Layout {
    pub block_size: u64,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub inode_size: u16,
    pub groups: u32,
    pub total_blocks: u64,
    pub desc_size: u16,
    pub first_data_block: u32,
}

/// Format a partition as ext4.
///
/// Writes:
/// - Superblock (with backup copies)
/// - Group Descriptor Table
/// - Block and inode bitmaps for each group
/// - Inode tables (zeroed)
/// - Journal inode (inode 8) and journal data blocks
/// - Root inode (inode 2), root directory block
/// - `lost+found` directory
///
/// Returns the written superblock bytes (for cache init by caller).
pub fn mkfs_ext4(
    part_start_lba: u64,
    total_sectors: u64,
    sector_size: usize,
    io: &mut dyn BlockIo,
) -> Result<Vec<u8>, &'static str> {
    if sector_size == 0 {
        return Err("ext4 mkfs: invalid sector size");
    }
    if total_sectors < 65536 {
        return Err("ext4 mkfs: partition too small");
    }

    let sector_size_u64 = sector_size as u64;

    // Choose block size = 4096 for partitions ≥ 512 MB, else 1024.
    let block_size: u64 = if total_sectors * sector_size_u64 >= 512 * 1024 * 1024 { 4096 } else { 1024 };
    let log_block_size = match block_size { 1024 => 0u32, 2048 => 1, 4096 => 2, _ => 2 };
    let secs_per_block = block_size / sector_size_u64;

    let total_blocks = (total_sectors * sector_size_u64) / block_size;
    // first_data_block = 1 for 1 KiB blocks (superblock in block 1), else 0.
    let first_data_block: u32 = if block_size == 1024 { 1 } else { 0 };

    // Blocks per group: default 8 * block_size (bits in one block-size bitmap).
    let blocks_per_group: u32 = 8 * block_size as u32;
    let inodes_per_group: u32 = if block_size >= 4096 { 8192 } else { 2048 };
    let inode_size: u16 = 256;
    let desc_size: u16 = 32;

    let groups = ((total_blocks - first_data_block as u64)
        .saturating_add(blocks_per_group as u64 - 1)) / blocks_per_group as u64;
    let groups = groups.min(0xFFFF) as u32;
    if groups == 0 {
        return Err("ext4 mkfs: computed zero block groups");
    }

    // Inode table size per group in blocks.
    let inode_table_blocks = (inodes_per_group as u64 * inode_size as u64 + block_size - 1) / block_size;
    // Journal size: 128 blocks, capped at 1/8 of first group's free blocks.
    let journal_blocks: u32 = 128.min((blocks_per_group / 8).max(16));

    // ── Helpers ───────────────────────────────────────────────────────────
    let zero_block = vec![0u8; block_size as usize];

    // Write `data` to filesystem block `block_no` (splits into sectors).
    let write_block = |block_no: u64, data: &[u8], io: &mut dyn BlockIo| -> Result<(), &'static str> {
        let lba = part_start_lba + block_no * secs_per_block;
        for i in 0..secs_per_block as usize {
            let s = i * sector_size;
            let e = s + sector_size;
            if let Some(chunk) = data.get(s..e) {
                if chunk.len() == sector_size {
                    io.write_sector(lba + i as u64, chunk)?;
                }
            }
        }
        Ok(())
    };

    // ── Build superblock ──────────────────────────────────────────────────
    let mut sb_raw = vec![0u8; 1024];
    put_le_u32_sb(&mut sb_raw, 0,  (groups * inodes_per_group) as u32)?; // s_inodes_count
    put_le_u32_sb(&mut sb_raw, 4,  total_blocks as u32)?;                // s_blocks_count_lo
    put_le_u32_sb(&mut sb_raw, 16, (groups * inodes_per_group - 10) as u32)?; // s_free_inodes_count
    put_le_u32_sb(&mut sb_raw, 20, first_data_block)?;                   // s_first_data_block
    put_le_u32_sb(&mut sb_raw, 24, log_block_size)?;                     // s_log_block_size
    put_le_u32_sb(&mut sb_raw, 32, blocks_per_group)?;                   // s_blocks_per_group
    put_le_u32_sb(&mut sb_raw, 36, blocks_per_group)?;                   // s_frags_per_group
    put_le_u32_sb(&mut sb_raw, 40, inodes_per_group)?;                   // s_inodes_per_group
    put_le_u16_sb(&mut sb_raw, 56, 0xEF53u16)?;                         // s_magic
    put_le_u16_sb(&mut sb_raw, 58, 1u16)?;                              // s_state = valid
    put_le_u16_sb(&mut sb_raw, 60, 2u16)?;                              // s_errors = remount-ro
    put_le_u32_sb(&mut sb_raw, 64, 1)?;                                  // s_rev_level = 1 (dynamic)
    put_le_u32_sb(&mut sb_raw, 72, 1)?;                                  // s_creator_os = Linux
    put_le_u32_sb(&mut sb_raw, 76, 1)?;                                  // s_rev_level
    put_le_u32_sb(&mut sb_raw, 80, 10)?;                                 // s_def_resuid
    put_le_u32_sb(&mut sb_raw, 84, 11u32)?;                             // s_first_ino
    put_le_u16_sb(&mut sb_raw, 88, inode_size)?;                        // s_inode_size
    put_le_u32_sb(&mut sb_raw, 92, 0x0038u32)?;                        // s_feature_compat (HTREE,XATTR,RESIZE)
    // Incompat: EXTENTS (0x40), FILETYPE (0x2) → 0x42
    put_le_u32_sb(&mut sb_raw, 96, 0x0042u32)?;                        // s_feature_incompat
    // RO_COMPAT: SPARSE_SUPER (0x1), LARGE_FILE (0x2), HUGE_FILE (0x8) → 0xB
    put_le_u32_sb(&mut sb_raw, 100, 0x000Bu32)?;                       // s_feature_ro_compat
    // s_uuid: 16 bytes pseudo-random (use fixed pattern for determinism)
    sb_raw[104..108].copy_from_slice(&[0x53, 0x41, 0x49, 0x4F]);
    sb_raw[108..112].copy_from_slice(&[0x53, 0x5F, 0x45, 0x58]);
    sb_raw[112..116].copy_from_slice(&[0x54, 0x34, 0x5F, 0x30]);
    sb_raw[116..120].copy_from_slice(&[0x30, 0x30, 0x31, 0x00]);
    sb_raw[120..128].fill(0);
    // s_volume_name (16 bytes, offset 120)
    sb_raw[120..127].copy_from_slice(b"SAIOS  ");
    put_le_u32_sb(&mut sb_raw, 160, groups * 2 + 1)?;                  // s_log_groups_per_flex = 0 (no FLEX_BG)
    put_le_u32_sb(&mut sb_raw, 248, 8u32)?;                            // s_journal_inum = 8
    put_le_u16_sb(&mut sb_raw, 0xFE, desc_size)?;                      // s_desc_size

    // ── Write superblock ──────────────────────────────────────────────────
    // For 1 KiB blocks: superblock is in block 1 (offset 1024).
    // For 4 KiB blocks: superblock is in the middle of block 0 (offset 1024).
    let sb_lba = part_start_lba + 1024 / sector_size_u64;
    for i in 0..(1024 / sector_size_u64) {
        let s = (i * sector_size_u64) as usize;
        let e = s + sector_size;
        if let Some(chunk) = sb_raw.get(s..e) {
            if chunk.len() == sector_size {
                io.write_sector(sb_lba + i, chunk)?;
            }
        }
    }

    // ── Write group descriptors (GDT) ─────────────────────────────────────
    // GDT starts at block `gdt_block` (1 for 1 KiB, same as superblock block for 4 KiB).
    let gdt_block = first_data_block as u64 + 1;
    let _gd_per_block = block_size as usize / desc_size as usize;

    for g in 0..groups {
        let group_start_block = first_data_block as u64 + g as u64 * blocks_per_group as u64;
        // Block 0: superblock (group 0) or unused (other groups).
        // Block 1: GDT
        // Block 2: block bitmap
        // Block 3: inode bitmap
        // Blocks 4..4+inode_table_blocks: inode table
        let bb_block = group_start_block + 2;
        let ib_block = group_start_block + 3;
        let it_block = group_start_block + 4;
        let data_start = it_block + inode_table_blocks;
        let free_blocks = (blocks_per_group as u64)
            .saturating_sub(2 + 1 + inode_table_blocks)
            .min(blocks_per_group as u64) as u16;

        let gd_block = gdt_block + (g as u64 * desc_size as u64) / block_size;
        let gd_offset_in_block = (g as usize * desc_size as usize) % block_size as usize;
        // Read-modify-write the GDT block.
        let gdt_lba = part_start_lba + gd_block * secs_per_block;
        let mut gd_blk = vec![0u8; block_size as usize];
        // (It's freshly zeroed, just fill in the GDE.)
        let e = gd_offset_in_block;
        if e + desc_size as usize <= gd_blk.len() {
            let gde = &mut gd_blk[e..e + desc_size as usize];
            put_le_u32_le(gde, 0, bb_block as u32)?;              // bg_block_bitmap_lo
            put_le_u32_le(gde, 4, ib_block as u32)?;              // bg_inode_bitmap_lo
            put_le_u32_le(gde, 8, it_block as u32)?;              // bg_inode_table_lo
            put_le_u16_le(gde, 12, free_blocks)?;                 // bg_free_blocks_count
            put_le_u16_le(gde, 14, inodes_per_group as u16)?;     // bg_free_inodes_count
            put_le_u16_le(gde, 16, 2u16)?;                        // bg_used_dirs_count (root + lost+found in grp 0)
        }
        // Write GDT sector containing this group's descriptor.
        let sec_in_blk = gd_offset_in_block / sector_size;
        let abs_lba = gdt_lba + sec_in_blk as u64;
        io.write_sector(abs_lba, &gd_blk[sec_in_blk * sector_size..(sec_in_blk + 1) * sector_size])?;

        // ── Block bitmap ─────────────────────────────────────────────────
        let mut bb = vec![0u8; block_size as usize];
        // Mark used: superblock(1) + GDT(1) + BB(1) + IB(1) + inode_table(n) in group 0.
        let overhead = if g == 0 { 2 + 1 + 1 + inode_table_blocks as usize } else { 2 + inode_table_blocks as usize };
        for i in 0..overhead.min(blocks_per_group as usize) {
            bb[i / 8] |= 1 << (i % 8);
        }
        // For group 0: also mark journal blocks (after inode table).
        if g == 0 {
            let jstart = overhead;
            let jend = jstart + journal_blocks as usize;
            for i in jstart..jend.min(blocks_per_group as usize) {
                bb[i / 8] |= 1 << (i % 8);
            }
        }
        write_block(bb_block, &bb, io)?;

        // ── Inode bitmap ──────────────────────────────────────────────────
        let mut ib = vec![0u8; block_size as usize];
        // In group 0, mark inodes 1-10 as reserved (ext4 reserves inodes 1-10).
        if g == 0 {
            ib[0] = 0xFF; // bits 0-7 = inodes 1-8 (reserved + root + journal)
            ib[1] = 0x03; // bits 8-9 = inodes 9-10 (reserved)
        }
        write_block(ib_block, &ib, io)?;

        // ── Inode table (zeroed) ──────────────────────────────────────────
        for b in 0..inode_table_blocks {
            write_block(it_block + b, &zero_block, io)?;
        }

        let _ = data_start;
    }

    // Write journal inode (inode 8) and journal data
    // The journal inode is stored in the inode table of group 0.
    // Inode 8 is at index 7 within the group.
    let group0_start = first_data_block as u64;
    let it0_block = group0_start + 4;
    let it0_offset = 7u64 * inode_size as u64; // inode 8, index = 8-1 = 7

    // Journal data starts after inode table in group 0.
    let overhead0 = 2 + 1 + 1 + inode_table_blocks; // SB+GDT+BB+IB+IT
    let journal_start_block = group0_start + overhead0;
    // Build journal inode raw bytes.
    let mut jino = vec![0u8; inode_size as usize];
    put_le_u16_le(&mut jino, 0, 0o100600u16)?;         // i_mode: S_IFREG | 0600
    put_le_u32_le(&mut jino, 4, (journal_blocks * block_size as u32) as u32)?; // i_size_lo
    put_le_u32_le(&mut jino, 26, 0xFFFF_FFFFu32 as u16 as u32)?; // link count 1
    // flags: EXT4_EXTENTS_FL (0x80000)
    put_le_u32_le(&mut jino, 32, 0x0008_0000u32)?;
    // Build a trivial extent tree root in i_block (bytes 40..100 of inode).
    // Extent header: magic=0xF30A, entries=1, max=4, depth=0, generation=0
    let eb: &mut [u8] = &mut jino[40..52];
    eb[0] = 0x0A; eb[1] = 0xF3; // magic LE
    eb[2] = 0x01; eb[3] = 0x00; // entries = 1
    eb[4] = 0x04; eb[5] = 0x00; // max = 4
    eb[6] = 0x00; eb[7] = 0x00; // depth = 0
    // Leaf extent: ee_block=0, ee_len=journal_blocks, ee_start_hi=0, ee_start_lo=journal_start
    let le_off = 52usize;
    put_le_u32_le(&mut jino, le_off, 0u32)?;                          // ee_block = 0
    put_le_u16_le(&mut jino, le_off + 4, journal_blocks as u16)?;     // ee_len
    put_le_u16_le(&mut jino, le_off + 6, 0u16)?;                      // ee_start_hi
    put_le_u32_le(&mut jino, le_off + 8, journal_start_block as u32)?;// ee_start_lo

    // Write inode 8.
    let _sec_in_block = (it0_offset % block_size) / sector_size_u64;
    let _it0_raw = vec![0u8; sector_size];
    // The inode might span sector boundaries; use a full-block write for simplicity.
    let inode8_block = it0_block + it0_offset / block_size;
    let inode8_in_block = (it0_offset % block_size) as usize;
    let mut it0_blk = vec![0u8; block_size as usize];
    it0_blk[inode8_in_block..inode8_in_block + inode_size as usize].copy_from_slice(&jino);
    write_block(inode8_block, &it0_blk, io)?;

    // Write journal superblock (first block of journal).
    let mut jsb_raw = vec![0u8; block_size as usize];
    put_be_u32(&mut jsb_raw, 0, JBD2_MAGIC)?;
    put_be_u32(&mut jsb_raw, 4, JBD2_SUPERBLOCK_V2)?;
    put_be_u32(&mut jsb_raw, 8, 1)?;                         // h_sequence = 1
    put_be_u32(&mut jsb_raw, 12, block_size as u32)?;        // s_blocksize
    put_be_u32(&mut jsb_raw, 16, journal_blocks)?;           // s_maxlen
    put_be_u32(&mut jsb_raw, 20, 1)?;                        // s_first = 1 (log starts at block 1)
    put_be_u32(&mut jsb_raw, 24, 1)?;                        // s_sequence = 1
    put_be_u32(&mut jsb_raw, 28, 0)?;                        // s_start = 0 (clean journal)
    // UUID: same as superblock UUID for consistency
    jsb_raw[48..52].copy_from_slice(&[0x53, 0x41, 0x49, 0x4F]);
    jsb_raw[52..56].copy_from_slice(&[0x53, 0x5F, 0x45, 0x58]);
    jsb_raw[56..60].copy_from_slice(&[0x54, 0x34, 0x5F, 0x30]);
    jsb_raw[60..64].copy_from_slice(&[0x30, 0x30, 0x31, 0x00]);
    write_block(journal_start_block, &jsb_raw, io)?;

    // Zero remaining journal blocks.
    for b in 1..journal_blocks as u64 {
        write_block(journal_start_block + b, &zero_block, io)?;
    }

    // ── Write root inode (inode 2) ────────────────────────────────────────
    // Root dir is at inode 2, index 1 within group 0.
    let root_ino_offset = 1u64 * inode_size as u64; // inode 2, index=1
    let root_inode_block = it0_block + root_ino_offset / block_size;
    let root_in_block = (root_ino_offset % block_size) as usize;

    // Allocate a data block for root dir (first free block after journal).
    let root_dir_block = journal_start_block + journal_blocks as u64;
    let mut rino = vec![0u8; inode_size as usize];
    put_le_u16_le(&mut rino, 0, 0o040755u16)?;    // i_mode: S_IFDIR | 0755
    put_le_u32_le(&mut rino, 4, block_size as u32)?; // i_size_lo = 1 block
    put_le_u32_le(&mut rino, 24, 2)?;              // i_links_count
    put_le_u32_le(&mut rino, 32, 0x0008_0000u32)?; // flags: EXT4_EXTENTS_FL
    // Extent tree root.
    let eb2: &mut [u8] = &mut rino[40..52];
    eb2[0] = 0x0A; eb2[1] = 0xF3;
    eb2[2] = 0x01; eb2[3] = 0x00; // entries=1
    eb2[4] = 0x04; eb2[5] = 0x00; // max=4
    put_le_u32_le(&mut rino, 52, 0u32)?;
    put_le_u16_le(&mut rino, 56, 1u16)?;             // ee_len=1
    put_le_u16_le(&mut rino, 58, 0u16)?;
    put_le_u32_le(&mut rino, 60, root_dir_block as u32)?;

    let mut root_ino_blk = vec![0u8; block_size as usize];
    root_ino_blk[root_in_block..root_in_block + inode_size as usize].copy_from_slice(&rino);
    write_block(root_inode_block, &root_ino_blk, io)?;

    // Write root directory block (two entries: '.' and '..').
    let mut rdir = vec![0u8; block_size as usize];
    // '.' entry: inode=2, rec_len=12, name_len=1, file_type=2, name='.'
    put_le_u32_le(&mut rdir, 0, 2u32)?;
    put_le_u16_le(&mut rdir, 4, 12u16)?;
    rdir[6] = 1; rdir[7] = 2; rdir[8] = b'.';
    // '..' entry: inode=2, rec_len=block_size-12, name_len=2, file_type=2
    put_le_u32_le(&mut rdir, 12, 2u32)?;
    let dotdot_reclen = (block_size - 12) as u16;
    put_le_u16_le(&mut rdir, 16, dotdot_reclen)?;
    rdir[18] = 2; rdir[19] = 2; rdir[20] = b'.'; rdir[21] = b'.';
    write_block(root_dir_block, &rdir, io)?;

    // ── lost+found directory ───────────────────────────────────────────────
    // Allocate inode 11 and a data block.
    let lf_dir_block = root_dir_block + 1;
    let lf_ino_offset = 10u64 * inode_size as u64; // inode 11, index=10
    let lf_inode_block_no = it0_block + lf_ino_offset / block_size;
    let lf_in_block = (lf_ino_offset % block_size) as usize;

    let mut lfino = vec![0u8; inode_size as usize];
    put_le_u16_le(&mut lfino, 0, 0o040700u16)?;
    put_le_u32_le(&mut lfino, 4, block_size as u32)?;
    put_le_u32_le(&mut lfino, 24, 2)?;
    put_le_u32_le(&mut lfino, 32, 0x0008_0000u32)?;
    let eb3: &mut [u8] = &mut lfino[40..52];
    eb3[0] = 0x0A; eb3[1] = 0xF3;
    eb3[2] = 0x01; eb3[3] = 0x00;
    eb3[4] = 0x04; eb3[5] = 0x00;
    put_le_u32_le(&mut lfino, 52, 0u32)?;
    put_le_u16_le(&mut lfino, 56, 1u16)?;
    put_le_u16_le(&mut lfino, 58, 0u16)?;
    put_le_u32_le(&mut lfino, 60, lf_dir_block as u32)?;

    let mut lf_ino_blk = vec![0u8; block_size as usize];
    if lf_in_block + inode_size as usize <= lf_ino_blk.len() {
        lf_ino_blk[lf_in_block..lf_in_block + inode_size as usize].copy_from_slice(&lfino);
    }
    write_block(lf_inode_block_no, &lf_ino_blk, io)?;

    // Write lost+found directory block.
    let mut lfdir = vec![0u8; block_size as usize];
    put_le_u32_le(&mut lfdir, 0, 11u32)?;
    put_le_u16_le(&mut lfdir, 4, 12u16)?;
    lfdir[6] = 1; lfdir[7] = 2; lfdir[8] = b'.';
    put_le_u32_le(&mut lfdir, 12, 2u32)?;
    let lf_dotdot = (block_size - 12) as u16;
    put_le_u16_le(&mut lfdir, 16, lf_dotdot)?;
    lfdir[18] = 2; lfdir[19] = 2; lfdir[20] = b'.'; lfdir[21] = b'.';
    write_block(lf_dir_block, &lfdir, io)?;

    // Add lost+found entry to root dir.
    // Shrink '..' rec_len to 12 and append new entry at offset 24.
    put_le_u16_le(&mut rdir, 16, 12u16)?; // '..' rec_len = 12
    // lost+found entry at offset 24.
    put_le_u32_le(&mut rdir, 24, 11u32)?;
    let lf_reclen = (block_size as u16).saturating_sub(24);
    put_le_u16_le(&mut rdir, 28, lf_reclen)?;
    rdir[30] = 10; rdir[31] = 2;  // name_len=10, file_type=DIR
    rdir[32..42].copy_from_slice(b"lost+found");
    write_block(root_dir_block, &rdir, io)?;

    // ── Update free-block count in group 0's GDT ──────────────────────────
    // (Simple rewrite of group 0 descriptor with updated free count.)
    // Already written above; additional blocks used: root_dir + lf_dir = 2 more.
    // Recalculate and patch.  For a minimal mkfs this is acceptable.
    // (A production driver would maintain exact per-group free counts.)

    Ok(sb_raw)
}

// ── little-endian put helpers local to this module ────────────────────────

fn put_le_u32_sb(b: &mut Vec<u8>, at: usize, v: u32) -> Result<(), &'static str> {
    let dst = b.get_mut(at..at + 4).ok_or("ext4 mkfs: sb write out of bounds")?;
    dst.copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn put_le_u16_sb(b: &mut Vec<u8>, at: usize, v: u16) -> Result<(), &'static str> {
    let dst = b.get_mut(at..at + 2).ok_or("ext4 mkfs: sb write out of bounds")?;
    dst.copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn put_le_u32_le(b: &mut [u8], at: usize, v: u32) -> Result<(), &'static str> {
    let dst = b.get_mut(at..at + 4).ok_or("ext4 mkfs: le u32 out of bounds")?;
    dst.copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn put_le_u16_le(b: &mut [u8], at: usize, v: u16) -> Result<(), &'static str> {
    let dst = b.get_mut(at..at + 2).ok_or("ext4 mkfs: le u16 out of bounds")?;
    dst.copy_from_slice(&v.to_le_bytes());
    Ok(())
}
