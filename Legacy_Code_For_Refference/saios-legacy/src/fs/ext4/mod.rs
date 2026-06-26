//! ext4 filesystem driver — read/write support.
//! Supports: extents, dir_htree, 64-bit block numbers, metadata checksums.

pub mod block;
pub mod dir;
pub mod extent;
pub mod inode;

use crate::block::BlockDevice;
use crate::vfs::{
    self, DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

// -- Superblock -------------------------------------------------------------

#[repr(C, packed)]
pub struct Ext4SuperBlock {
    pub s_inodes_count: u32,
    pub s_blocks_count_lo: u32,
    pub s_r_blocks_count_lo: u32,
    pub s_free_blocks_lo: u32,
    pub s_free_inodes: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,
    pub s_log_cluster_size: u32,
    pub s_blocks_per_group: u32,
    pub s_clusters_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    pub s_magic: u16,
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_lastcheck: u32,
    pub s_checkinterval: u32,
    pub s_creator_os: u32,
    pub s_rev_level: u32,
    pub s_def_resuid: u16,
    pub s_def_resgid: u16,
    // EXT4 dynamic fields start here (rev_level >= 1)
    pub s_first_ino: u32,
    pub s_inode_size: u16,
    pub s_block_group_nr: u16,
    pub s_feature_compat: u32,
    pub s_feature_incompat: u32,
    pub s_feature_ro_compat: u32,
    pub s_uuid: [u8; 16],
    pub s_volume_name: [u8; 16],
    pub s_last_mounted: [u8; 64],
    pub s_algorithm_usage_bitmap: u32,
    pub s_prealloc_blocks: u8,
    pub s_prealloc_dir_blocks: u8,
    pub s_reserved_gdt_blocks: u16,
    pub s_journal_uuid: [u8; 16],
    pub s_journal_inum: u32,
    pub s_journal_dev: u32,
    pub s_last_orphan: u32,
    pub s_hash_seed: [u32; 4],
    pub s_def_hash_version: u8,
    pub s_jnl_backup_type: u8,
    pub s_desc_size: u16,
    pub s_default_mount_opts: u32,
    pub s_first_meta_bg: u32,
    pub s_mkfs_time: u32,
    pub s_jnl_blocks: [u32; 17],
    pub s_blocks_count_hi: u32,
    pub s_r_blocks_count_hi: u32,
    pub s_free_blocks_hi: u32,
    pub s_min_extra_isize: u16,
    pub s_want_extra_isize: u16,
    pub s_flags: u32,
    // padding to 1024 bytes
    pub _pad: [u8; 800],
}

const EXT4_MAGIC: u16 = 0xEF53;

// Feature flags
const INCOMPAT_EXTENTS: u32 = 0x40;
const INCOMPAT_64BIT: u32 = 0x80;
const INCOMPAT_FLEX_BG: u32 = 0x200;
const INCOMPAT_HTREE: u32 = 0x02;
const INCOMPAT_FILETYPE: u32 = 0x02; // in compat actually
pub const RO_COMPAT_SPARSE_SUPER: u32 = 0x01;
pub const RO_COMPAT_LARGE_FILE: u32 = 0x02;
pub const RO_COMPAT_HUGE_FILE: u32 = 0x08;
pub const RO_COMPAT_METADATA_CSUM: u32 = 0x400;

// -- Block group descriptor -------------------------------------------------

#[repr(C, packed)]
pub struct Ext4GroupDesc {
    pub bg_block_bitmap_lo: u32,
    pub bg_inode_bitmap_lo: u32,
    pub bg_inode_table_lo: u32,
    pub bg_free_blocks_lo: u16,
    pub bg_free_inodes_lo: u16,
    pub bg_used_dirs_lo: u16,
    pub bg_flags: u16,
    pub bg_exclude_bitmap_lo: u32,
    pub bg_block_bitmap_csum_lo: u16,
    pub bg_inode_bitmap_csum_lo: u16,
    pub bg_itable_unused_lo: u16,
    pub bg_checksum: u16,
    // 64-bit extensions
    pub bg_block_bitmap_hi: u32,
    pub bg_inode_bitmap_hi: u32,
    pub bg_inode_table_hi: u32,
    pub bg_free_blocks_hi: u16,
    pub bg_free_inodes_hi: u16,
    pub bg_used_dirs_hi: u16,
    pub bg_itable_unused_hi: u16,
    pub bg_exclude_bitmap_hi: u32,
    pub bg_block_bitmap_csum_hi: u16,
    pub bg_inode_bitmap_csum_hi: u16,
    pub bg_reserved: u32,
}

// -- Filesystem state -------------------------------------------------------

pub struct Ext4Fs {
    pub dev: Arc<dyn BlockDevice>,
    /// Byte offset of the ext4 partition from the start of the disk.  All disk
    /// access must be relative to this — the superblock lives 1024 bytes into
    /// the *partition*, not the disk (the installer puts the partition at LBA
    /// 2048).  Forgetting this made the mount read the MBR gap → "bad magic" →
    /// tmpfs root → nothing persisted.
    pub part_offset: u64,
    pub block_size: usize,
    pub inodes_per_group: u32,
    pub blocks_per_group: u32,
    pub inode_size: usize,
    pub desc_size: usize,
    pub group_count: u32,
    pub total_inodes: u32,
    pub feature_incompat: u32,
    pub feature_ro: u32,
}

impl Ext4Fs {
    pub fn mount(dev: Arc<dyn BlockDevice>) -> Result<Arc<Mutex<Self>>, &'static str> {
        // The ext4 superblock sits 1024 bytes into the *partition*.  Probe the
        // likely partition starts and use the first whose superblock has the
        // ext4 magic: the MBR partition-0 LBA, the installer's fixed LBA 2048,
        // and 0 (a whole-disk / unpartitioned ext4).
        // Scan ALL four MBR partition entries for an ext4 superblock (the UEFI
        // installer puts the ext4 partition AFTER the FAT ESP, i.e. not entry 0),
        // then fall back to the fixed LBA 2048 and a whole-disk ext4 at 0.
        let mut candidates: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
        let mut mbr = [0u8; 512];
        if dev.read_bytes(0, &mut mbr).is_ok() && mbr[510] == 0x55 && mbr[511] == 0xAA {
            for i in 0..4usize {
                let e = 0x1BE + i * 16;
                let lba =
                    u32::from_le_bytes([mbr[e + 8], mbr[e + 9], mbr[e + 10], mbr[e + 11]]) as u64;
                if lba > 0 {
                    candidates.push(lba * 512);
                }
            }
        }
        candidates.push(2048u64 * 512);
        candidates.push(0);

        let mut sb_data = alloc::vec![0u8; 1024];
        let mut part_offset = u64::MAX;
        for &cand in &candidates {
            if dev.read_bytes(cand + 1024, &mut sb_data).is_err() {
                continue;
            }
            let m = u16::from_le_bytes([sb_data[56], sb_data[57]]); // s_magic at sb offset 0x38
            if m == EXT4_MAGIC {
                part_offset = cand;
                break;
            }
        }
        if part_offset == u64::MAX {
            return Err("ext4: bad magic");
        }

        let sb = unsafe { &*(sb_data.as_ptr() as *const Ext4SuperBlock) };
        let magic = { sb.s_magic };
        if magic != EXT4_MAGIC {
            return Err("ext4: bad magic");
        }

        let block_size =
            1024usize << unsafe { core::ptr::addr_of!(sb.s_log_block_size).read_unaligned() };
        let inodes_per_group =
            unsafe { core::ptr::addr_of!(sb.s_inodes_per_group).read_unaligned() };
        let blocks_per_group =
            unsafe { core::ptr::addr_of!(sb.s_blocks_per_group).read_unaligned() };
        let rev_level = unsafe { core::ptr::addr_of!(sb.s_rev_level).read_unaligned() };
        let inode_size = if rev_level >= 1 {
            (unsafe { core::ptr::addr_of!(sb.s_inode_size).read_unaligned() }) as usize
        } else {
            128
        };
        let feat_incompat = unsafe { core::ptr::addr_of!(sb.s_feature_incompat).read_unaligned() };
        let feat_ro = unsafe { core::ptr::addr_of!(sb.s_feature_ro_compat).read_unaligned() };
        let total_inodes = unsafe { core::ptr::addr_of!(sb.s_inodes_count).read_unaligned() };
        let total_blocks_lo =
            (unsafe { core::ptr::addr_of!(sb.s_blocks_count_lo).read_unaligned() }) as u64;
        let total_blocks_hi =
            (unsafe { core::ptr::addr_of!(sb.s_blocks_count_hi).read_unaligned() }) as u64;
        let total_blocks = total_blocks_lo | (total_blocks_hi << 32);
        let group_count = total_blocks.div_ceil(blocks_per_group as u64) as u32;

        let desc_size = if feat_incompat & INCOMPAT_64BIT != 0 {
            (unsafe { core::ptr::addr_of!(sb.s_desc_size).read_unaligned() }) as usize
        } else {
            32
        };

        crate::println!(
            "[ext4] block_size={} groups={} inodes={} inode_size={}",
            block_size,
            group_count,
            total_inodes,
            inode_size
        );

        Ok(Arc::new(Mutex::new(Self {
            dev,
            part_offset,
            block_size,
            inodes_per_group,
            blocks_per_group,
            inode_size,
            desc_size,
            group_count,
            total_inodes,
            feature_incompat: feat_incompat,
            feature_ro: feat_ro,
        })))
    }

    /// Read a raw block by block number (relative to the partition).
    pub fn read_block(&self, block: u64) -> Result<alloc::vec::Vec<u8>, &'static str> {
        let mut buf = alloc::vec![0u8; self.block_size];
        self.dev
            .read_bytes(self.part_offset + block * self.block_size as u64, &mut buf)?;
        Ok(buf)
    }

    /// Write a raw block (relative to the partition).
    pub fn write_block(&self, block: u64, data: &[u8]) -> Result<(), &'static str> {
        self.dev
            .write_bytes(self.part_offset + block * self.block_size as u64, data)
    }

    /// Read the group descriptor for group `g`.
    pub fn read_group_desc(&self, g: u32) -> Result<Ext4GroupDesc, &'static str> {
        // GDT starts at block 1 (for 1 KiB blocks) or block 1 (for larger)
        let gdt_block = if self.block_size == 1024 { 2u64 } else { 1u64 };
        let offset = gdt_block * self.block_size as u64 + g as u64 * self.desc_size as u64;
        let mut buf = alloc::vec![0u8; self.desc_size];
        self.dev.read_bytes(self.part_offset + offset, &mut buf)?;
        let gd = unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const Ext4GroupDesc) };
        Ok(gd)
    }

    /// Write back a group descriptor for group `g` (mirror of read_group_desc).
    pub fn write_group_desc(&self, g: u32, gd: &Ext4GroupDesc) -> Result<(), &'static str> {
        let gdt_block = if self.block_size == 1024 { 2u64 } else { 1u64 };
        let offset = gdt_block * self.block_size as u64 + g as u64 * self.desc_size as u64;
        let bytes = unsafe {
            core::slice::from_raw_parts((gd as *const Ext4GroupDesc) as *const u8, self.desc_size)
        };
        self.dev.write_bytes(self.part_offset + offset, bytes)
    }

    /// Apply a signed delta to the superblock's free-block / free-inode counters
    /// (read-modify-write of the two u32s at the fixed ext4 offsets 12 and 16).
    /// Keeps the on-disk totals consistent for fsck / a stricter ext4 mount.
    pub fn adjust_super_counts(&self, dblocks: i64, dinodes: i64) -> Result<(), &'static str> {
        let base = self.part_offset + 1024; // superblock is at byte 1024
        if dblocks != 0 {
            let mut b = [0u8; 4];
            self.dev.read_bytes(base + 12, &mut b)?;
            let v = (u32::from_le_bytes(b) as i64 + dblocks).max(0) as u32;
            self.dev.write_bytes(base + 12, &v.to_le_bytes())?;
        }
        if dinodes != 0 {
            let mut b = [0u8; 4];
            self.dev.read_bytes(base + 16, &mut b)?;
            let v = (u32::from_le_bytes(b) as i64 + dinodes).max(0) as u32;
            self.dev.write_bytes(base + 16, &v.to_le_bytes())?;
        }
        Ok(())
    }

    /// Get inode table block for group.
    pub fn inode_table_block(&self, g: u32) -> Result<u64, &'static str> {
        let gd = self.read_group_desc(g)?;
        let lo = (unsafe { core::ptr::addr_of!(gd.bg_inode_table_lo).read_unaligned() }) as u64;
        let hi = if self.feature_incompat & INCOMPAT_64BIT != 0 {
            (unsafe { core::ptr::addr_of!(gd.bg_inode_table_hi).read_unaligned() }) as u64
        } else {
            0
        };
        Ok(lo | (hi << 32))
    }

    /// Read a raw inode by inode number (1-based).
    pub fn read_raw_inode(&self, ino: u32) -> Result<inode::Ext4Inode, &'static str> {
        if ino == 0 {
            return Err("ext4: inode 0 is invalid");
        }
        let group = (ino - 1) / self.inodes_per_group;
        let index = (ino - 1) % self.inodes_per_group;
        let table = self.inode_table_block(group)?;
        let offset = table * self.block_size as u64 + index as u64 * self.inode_size as u64;
        let mut buf = alloc::vec![0u8; self.inode_size];
        self.dev.read_bytes(self.part_offset + offset, &mut buf)?;
        let raw_inode =
            unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const inode::Ext4Inode) };
        Ok(raw_inode)
    }

    /// Write a raw inode back to the inode table (1-based inode number).
    pub fn write_raw_inode(&self, ino: u32, raw: &inode::Ext4Inode) -> Result<(), &'static str> {
        if ino == 0 {
            return Err("ext4: inode 0 is invalid");
        }
        let group = (ino - 1) / self.inodes_per_group;
        let index = (ino - 1) % self.inodes_per_group;
        let table = self.inode_table_block(group)?;
        let offset = table * self.block_size as u64 + index as u64 * self.inode_size as u64;
        // Serialise into a zeroed inode_size buffer so stale on-disk fields in a
        // recycled inode slot are cleared.
        let mut buf = alloc::vec![0u8; self.inode_size];
        let sz = core::mem::size_of::<inode::Ext4Inode>().min(self.inode_size);
        let src =
            unsafe { core::slice::from_raw_parts(raw as *const inode::Ext4Inode as *const u8, sz) };
        buf[..sz].copy_from_slice(src);
        self.dev.write_bytes(self.part_offset + offset, &buf)
    }

    /// Get the VFS root inode (inode 2 in ext4).
    pub fn root_inode(fs: Arc<Mutex<Self>>) -> VfsResult<Arc<VfsInode>> {
        let raw = {
            let f = fs.lock();
            f.read_raw_inode(2).map_err(|_| VfsError::Io)?
        };
        let ftype =
            inode::mode_to_filetype(unsafe { core::ptr::addr_of!(raw.i_mode).read_unaligned() });
        let ops = inode::Ext4InodeOps { ino: 2, raw, fs };
        Ok(VfsInode::new(alloc_ino(), ftype, Arc::new(ops)))
    }
}

struct Ext4Driver;

impl vfs::FileSystemDriver for Ext4Driver {
    fn fs_type(&self) -> &'static str {
        "ext4"
    }

    fn mount(&self, request: &vfs::MountRequest) -> Result<Arc<VfsInode>, &'static str> {
        match &request.source {
            vfs::MountSource::BlockDevice(dev) => {
                let fs = Ext4Fs::mount(dev.clone())?;
                Ext4Fs::root_inode(fs).map_err(|_| "ext4: failed to get root inode")
            }
            _ => Err("ext4: block device mount source required"),
        }
    }
}

pub fn register_driver() -> Result<(), &'static str> {
    match vfs::register_filesystem(Arc::new(Ext4Driver)) {
        Ok(()) | Err(VfsError::AlreadyExists) => Ok(()),
        Err(_) => Err("ext4: failed to register driver"),
    }
}

/// Mount ext4 on the given block device and register it at `mountpoint`.
pub fn mount(dev: Arc<dyn BlockDevice>, mountpoint: &str) -> Result<(), &'static str> {
    crate::vfs_contract::VfsContract::mount_fs(
        "ext4",
        &vfs::MountRequest::new(mountpoint, vfs::MountSource::BlockDevice(dev)),
    )
}
