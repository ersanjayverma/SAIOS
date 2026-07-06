# ADR-0019 — Complete Native FAT32 and ext4 Filesystem Support

**Status:** Active  
**Date:** 2026-07-06

---

## Context

SAIOS previously used a custom "managed store" serialisation format
(`SAFAT32\0` / `SAEXT4\0\0` magic + a flat node tree written to the first
256 KiB of the partition) to persist filesystem trees.  This let the kernel
mount/read/write virtual volumes, but meant:

- **Real Linux/Windows disks are partially broken** — ext4 native read is
  partially implemented but has a file-inode-load failure for regular files;
  FAT32 native read/write is completely absent.
- **No journaling** — ext4 writes had no JBD2 transaction log, risking
  corruption on power-loss.
- **No format** — `format_volume` wrote the managed-store header, not a
  real `mkfs`-equivalent on-disk layout.
- **Execute is unimplemented** — VFS paths cannot be directly executed
  (ELF loader is not wired to storage-backed files).

---

## Decision

Replace or augment the managed-store layer with complete native FAT32 and
ext4 support across the full lifecycle: detect → probe → mount → read →
write → format, with JBD2 journaling for ext4 and proper FAT chain
management for FAT32, and with execute wired through the ELF loader.

---

## Architecture

### Layers

```
User commands (shell: cat, cp, mkfs, fsck)
        │
        ▼
    saifs namespace  (path resolution, object model)
        │
        ▼
    vfs.rs           (open, read, write, readdir, stat, mkdir, rm …)
        │
        ▼
    storage.rs       (volume registry, partition table, block I/O, FS dispatch)
        │
     ┌──┴───────────────────┐
     ▼                      ▼
  fat32 subsystem        ext4 subsystem
  (BPB, FAT chain,       (superblock, GDT, inodes,
   dir entries, LFN,      extents, HTree, JBD2)
   mkfs, fsck)
        │                      │
        └─────────┬────────────┘
                  ▼
           block layer  (read_sector / write_sector / flush)
                  │
                  ▼
           AHCI / NVMe driver
```

### FAT32 subsystem

| Capability         | Approach |
|--------------------|----------|
| Probe              | Check BPB signature `0x55 0xAA`, `jmp` at offset 0, OEM ID |
| Mount              | Parse BPB: sector size, sectors/cluster, FAT offset, root cluster |
| FAT table          | Read FAT1 (or FAT2 if FAT1 read fails); cache per volume |
| Cluster chain      | Follow `FAT[cluster]` entries until `EOC (≥0x0FFFFFF8)` |
| Directory entries  | 32-byte short entries (8.3); LFN (Long File Name) VFAT extension |
| File read          | Walk cluster chain; read each cluster's sectors; slice to file size |
| File write         | Allocate clusters from free space in FAT; update chain; write data |
| Directory create   | Allocate new directory cluster; initialise `.` and `..` entries |
| Directory delete   | Mark all LFN + SFN entries deleted (`0xE5`); free FAT chain |
| File truncate      | Free tail clusters; update size in directory entry |
| Format (`mkfs`)    | Write BPB (FAT32), FSInfo sector, FAT1+FAT2, root cluster (all zero), MBR update |
| Cache invalidation | On unmount or rescan |

### ext4 subsystem

| Capability         | Approach |
|--------------------|----------|
| Probe              | Superblock magic `0xEF53` at offset 1024 |
| Mount              | Read superblock → GDT → cache per volume; check incompat features |
| Inode load         | `bg_inode_table_lo/hi` → `inode_table_block`; `read_partition_at` |
| File read          | Extent tree (`EXT4_EXTENTS_FL`) or direct blocks; inline data (`EXT4_INLINE_DATA_FL`) |
| Directory read     | Linear or HTree (`EXT4_INDEX_FL`) with correct `dx_root` offset layout |
| Symlink            | Fast symlink (data in `i_block`); slow symlink (single data block) |
| **Write**          | Journal descriptor + commit; block bitmap; inode bitmap; inode table; extent tree |
| **Journaling (JBD2)** | `journal_superblock_s` at journal inode's first block; descriptor blocks; commit blocks; checksum |
| **Format (`mkfs`)** | Block groups; superblock + backup copies; GDT; block/inode bitmaps; inode table; journal inode; lost+found |
| Feature flags      | Support: `EXTENTS`, `HTree`, `64BIT`, `FILETYPE`, `FLEX_BG`, `EXTATTR`, `SPARSE_SUPER`, `LARGE_FILE` |
| Reject flags       | `COMPRESSION`, `JOURNAL_DEV`, `ENCRYPT` → mount error |

### JBD2 Journal (ext4 write path)

```
begin_transaction()
  → reserve descriptor block slot
  → tag each dirty block with (fsblk, sequence)

commit_transaction()
  → write descriptor block (JBD2_DESCRIPTOR_BLOCK)
  → write all dirty data blocks to journal area
  → write commit block (JBD2_COMMIT_BLOCK)
  → flush

checkpoint()
  → write dirty blocks to their real disk locations
  → update journal tail (s_sequence, s_start)
```

Dirty blocks are written to the journal first (write-ahead logging), then
asynchronously checkpointed to their on-disk positions.  This guarantees
atomicity of multi-block operations (e.g. create file = alloc inode + update
bitmap + write directory entry).

### Execute support

`vfs::open(path, OpenOptions { execute: true, … })` on a storage-backed path:
1. Reads the file bytes via `storage::fs_read`.
2. Passes the byte slice to `kernel::elf_loader::load_and_exec(bytes, args)`.
3. The ELF loader maps segments into a new VMM address space (ring-3 pages).
4. Jumps to ring-3 entry point via `sysretq`.

---

## Implementation Phases

### Phase 1 — Diagnose and fix ext4 regular-file read failure (immediate)

**Root-cause investigation:**  
Add diagnostic serial output in `ext4_load_inode` when it returns `Err`,
printing the inode number, group, inode table offset, and the error string.
This will identify whether the failure is:
- AHCI sector read error (timeout / task file error)
- `read_partition_at` bounds check failure
- Wrong `bg_inode_table_lo` value (corrupted GDT)
- Some other decode error

Also fix the `indirect_levels` offset bug: currently reads from byte offset 26
(inside `reserved_zero`) instead of the correct offset 30 (`dx_root_info.indirect_levels`).

### Phase 2 — Native FAT32 read

Implement `fat32_mount`, `fat32_readdir`, `fat32_read_file` using the BPB
and cluster-chain model.  Wire into `fs_stat`, `fs_readdir`, `fs_read` for
volumes whose `probe_filesystem` returns `Fat16` / `Fat32`.

Key structures:
```rust
struct Fat32Superblock { bytes_per_sector, sectors_per_cluster, 
                         reserved_sectors, fat_count, root_cluster,
                         fat_size_32, total_sectors_32 }
struct Fat32DirEntry  { name: [u8;11], attr, cluster_hi, cluster_lo,
                        file_size, … }
```

### Phase 3 — Native FAT32 write

`fat32_create_file`, `fat32_write_file`, `fat32_delete_file`,
`fat32_create_dir`.  Update FAT and FSInfo sector atomically (FAT written
before directory entry; both FAT copies written).

### Phase 4 — FAT32 format (`mkfs.fat32`)

Write BPB (boot sector), FSInfo (sector 1), FAT1 + FAT2, root cluster.
Update MBR partition type to `0x0C` (FAT32 LBA).

### Phase 5 — ext4 write with JBD2

Enable `EXT4_NATIVE_STAGE8_EXPERIMENTAL`.  Implement:
- `jbd2_begin` / `jbd2_commit` / `jbd2_checkpoint`
- Block alloc (`ext4_alloc_block_jbd2`) with journal logging
- Inode alloc / update with journal logging
- Directory entry append / delete with journal logging

### Phase 6 — ext4 format (`mkfs.ext4`)

Compute block-group layout, write all on-disk structures, create journal
inode, write root inode (inode 2), create `lost+found`.

### Phase 7 — Execute support

Add `OpenOptions::execute` flag; wire VFS to ELF loader; create ring-3
address space; `sysretq` to entry point.

---

## On-Disk Format Summary

### FAT32 BPB (key fields)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 3 | `jmp_boot` |
| 3 | 8 | OEM name |
| 11 | 2 | `bytes_per_sector` |
| 13 | 1 | `sectors_per_cluster` |
| 14 | 2 | `reserved_sector_count` |
| 16 | 1 | `num_fats` |
| 28 | 4 | `hidden_sectors` |
| 32 | 4 | `total_sectors_32` |
| 36 | 4 | `fat_size_32` |
| 44 | 4 | `root_cluster` |
| 48 | 2 | `fs_info` sector |
| 510 | 2 | `0x55 0xAA` |

FAT32 cluster values: `0x00000000` = free; `0x0FFFFFFF` = EOC; `0x0FFFFFF7` = bad.

### ext4 Key Superblock Offsets (relative to partition start + 1024)

| Offset | Field |
|--------|-------|
| 0 | `s_inodes_count` |
| 4 | `s_blocks_count_lo` |
| 20 | `s_first_data_block` |
| 24 | `s_log_block_size` |
| 32 | `s_blocks_per_group` |
| 40 | `s_inodes_per_group` |
| 56 | `s_magic` (0xEF53) |
| 84 | `s_first_ino` |
| 88 | `s_inode_size` |
| 92 | `s_feature_compat` |
| 96 | `s_feature_incompat` |
| 100 | `s_feature_ro_compat` |
| 0xFE | `s_desc_size` |

### JBD2 Journal Superblock Offsets

| Offset | Field |
|--------|-------|
| 0 | `s_header.h_magic` (0xC03B3998) |
| 4 | `s_header.h_blocktype` (4 = superblock v2) |
| 12 | `s_blocksize` |
| 16 | `s_maxlen` (total journal blocks) |
| 20 | `s_first` (first usable block) |
| 24 | `s_sequence` (first commit ID expected) |
| 28 | `s_start` (blocknr of first transaction) |

---

## Consequences

- Real Linux ext4 and FAT32 volumes become fully mountable and writable.
- JBD2 journaling provides crash consistency for ext4.
- Execute enables running ELF binaries directly from mounted volumes.
- The "managed store" code path remains as a fallback for SAIOS-native
  scratch volumes that are not formatted with a real filesystem.
- Code size of `storage.rs` will grow significantly; a future ADR should
  split it into `driver/fat32.rs` and `driver/ext4.rs` sub-modules.
