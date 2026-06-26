//! ext4 inode operations.
/// Read a potentially-unaligned packed struct field safely.
macro_rules! pf {
    ($x:expr) => {
        unsafe { core::ptr::addr_of!($x).read_unaligned() }
    };
}
/// Write a potentially-unaligned packed struct field safely.
macro_rules! pfw {
    ($x:expr, $v:expr) => {
        unsafe { core::ptr::addr_of_mut!($x).write_unaligned($v) }
    };
}

const S_IFDIR: u16 = 0o040000;

use super::Ext4Fs;
use crate::vfs::{
    DirEntry, FileType, Inode as VfsInode, InodeOps, Stat, VfsError, VfsResult, alloc_ino,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

#[repr(C, packed)]
pub struct Ext4Inode {
    pub i_mode: u16,
    pub i_uid: u16,
    pub i_size_lo: u32,
    pub i_atime: u32,
    pub i_ctime: u32,
    pub i_mtime: u32,
    pub i_dtime: u32,
    pub i_gid: u16,
    pub i_links_count: u16,
    pub i_blocks_lo: u32,
    pub i_flags: u32,
    pub i_osd1: u32,
    pub i_block: [u32; 15], // blocks array or extent tree
    pub i_generation: u32,
    pub i_file_acl_lo: u32,
    pub i_size_hi: u32,
    pub i_obso_faddr: u32,
    pub i_osd2: [u32; 3],
    pub i_extra_isize: u16,
    pub i_checksum_hi: u16,
    pub i_ctime_extra: u32,
    pub i_mtime_extra: u32,
    pub i_atime_extra: u32,
    pub i_crtime: u32,
    pub i_crtime_extra: u32,
    pub i_version_hi: u32,
    pub i_projid: u32,
}

// i_flags: EXTENTS_FL
const EXT4_EXTENTS_FL: u32 = 0x80000;

impl Ext4Inode {
    /// A fully-zeroed inode (all fields are integers / arrays).
    pub fn zeroed() -> Self {
        unsafe { core::mem::zeroed() }
    }
}

pub fn mode_to_filetype(mode: u16) -> FileType {
    match mode & 0xF000 {
        0x8000 => FileType::RegularFile,
        0x4000 => FileType::Directory,
        0xA000 => FileType::SymLink,
        0x2000 => FileType::CharDevice,
        0x6000 => FileType::BlockDevice,
        0x1000 => FileType::Pipe,
        0xC000 => FileType::Socket,
        _ => FileType::RegularFile,
    }
}

pub struct Ext4InodeOps {
    pub ino: u32,
    pub raw: Ext4Inode,
    pub fs: Arc<Mutex<Ext4Fs>>,
}

impl Ext4InodeOps {
    fn file_size(&self) -> u64 {
        let lo = pf!(self.raw.i_size_lo) as u64;
        let hi = pf!(self.raw.i_size_hi) as u64;
        lo | (hi << 32)
    }

    /// Read data from this inode at byte offset.
    fn read_data(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let size = self.file_size();
        if offset >= size {
            return Ok(0);
        }
        let len = buf.len().min((size - offset) as usize);

        let flags = pf!(self.raw.i_flags);
        if flags & EXT4_EXTENTS_FL != 0 {
            self.read_via_extents(offset, &mut buf[..len])
        } else {
            self.read_via_blocks(offset, &mut buf[..len])
        }
    }

    fn read_via_extents(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let fs = self.fs.lock();
        // The extent tree root is in i_block[0..12] (60 bytes)
        let extent_data = unsafe {
            core::slice::from_raw_parts(core::ptr::addr_of!(self.raw.i_block) as *const u8, 60)
        };
        let blocks = super::extent::read_extent_tree(&fs, extent_data, offset, buf)
            .map_err(|_| VfsError::Io)?;
        Ok(blocks)
    }

    fn read_via_blocks(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        // Direct/indirect block addressing
        let fs = self.fs.lock();
        let bs = fs.block_size;
        let mut done = 0usize;
        while done < buf.len() {
            let file_offset = offset + done as u64;
            let block_idx = (file_offset / bs as u64) as usize;
            let block_off = (file_offset % bs as u64) as usize;
            let i_block_copy: [u32; 15] =
                unsafe { core::ptr::addr_of!(self.raw.i_block).read_unaligned() };
            let block_no = get_block_no(&fs, &i_block_copy, block_idx).map_err(|_| VfsError::Io)?;
            if block_no == 0 {
                // Sparse block — zeroes
                let to_fill = (bs - block_off).min(buf.len() - done);
                buf[done..done + to_fill].fill(0);
                done += to_fill;
                continue;
            }
            let blk_data = fs.read_block(block_no as u64).map_err(|_| VfsError::Io)?;
            let to_copy = (bs - block_off).min(buf.len() - done);
            buf[done..done + to_copy].copy_from_slice(&blk_data[block_off..block_off + to_copy]);
            done += to_copy;
        }
        Ok(done)
    }

    /// Write `buf` at byte `offset`, allocating data blocks + extents as needed
    /// and growing the file.  Operates against the on-disk inode (re-read fresh)
    /// so the result persists and the in-memory copy never goes stale.
    fn write_data(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let fs = self.fs.lock();
        let mut raw = fs.read_raw_inode(self.ino).map_err(|_| VfsError::Io)?;
        if pf!(raw.i_flags) & EXT4_EXTENTS_FL == 0 {
            return Err(VfsError::NotSupported); // legacy block-mapped write NYI
        }
        let bs = fs.block_size as u64;
        let mut done = 0usize;
        // Defer the per-block disk flush to one flush at the end: writing a large
        // file (e.g. the 48 MB apt index) one block at a time otherwise issues a
        // FLUSH CACHE per block (~12k flushes), which looks like a hang.
        fs.dev.set_write_through(false);
        // Show a progress bar for large writes (small ones complete instantly).
        let show = buf.len() > 1_048_576;
        let mut last_render = 0usize;
        while done < buf.len() {
            let file_off = offset + done as u64;
            let logical = (file_off / bs) as u32;
            let blk_off = (file_off % bs) as usize;
            // Map the logical block, allocating + extending the extent tree on a hole.
            let phys = {
                let root = unsafe {
                    core::slice::from_raw_parts(core::ptr::addr_of!(raw.i_block) as *const u8, 60)
                };
                super::extent::find_extent_pub(&fs, root, logical).map_err(|_| VfsError::Io)?
            };
            let phys = if phys == 0 {
                let nb = fs.alloc_block().map_err(|_| VfsError::NoSpace)? as u32;
                super::extent::append_inline_extent(&mut raw, logical, nb)
                    .map_err(|_| VfsError::NoSpace)?;
                nb as u64
            } else {
                phys
            };
            let to_copy = (fs.block_size - blk_off).min(buf.len() - done);
            // Whole-block overwrite needs no read-modify-write; only read the
            // existing block for a partial-block update.
            let mut blk = if blk_off == 0 && to_copy == fs.block_size {
                alloc::vec![0u8; fs.block_size]
            } else {
                fs.read_block(phys).map_err(|_| VfsError::Io)?
            };
            blk[blk_off..blk_off + to_copy].copy_from_slice(&buf[done..done + to_copy]);
            fs.write_block(phys, &blk).map_err(|_| VfsError::Io)?;
            done += to_copy;
            if show && (done - last_render >= 1_048_576 || done == buf.len()) {
                last_render = done;
                crate::shell::progress_set("write", done as u64, buf.len() as u64);
                crate::shell::progress_render();
            }
        }
        // Commit the batch to stable storage, then restore write-through.
        let flush_res = fs.dev.flush();
        fs.dev.set_write_through(true);
        if show {
            crate::shell::progress_clear();
            crate::println!();
        }
        flush_res.map_err(|_| VfsError::Io)?;
        // Grow size / block count if we extended the file.
        let old_size = (pf!(raw.i_size_lo) as u64) | ((pf!(raw.i_size_hi) as u64) << 32);
        let new_size = old_size.max(offset + buf.len() as u64);
        pfw!(raw.i_size_lo, (new_size & 0xFFFF_FFFF) as u32);
        pfw!(raw.i_size_hi, (new_size >> 32) as u32);
        let nblocks = new_size.div_ceil(bs);
        pfw!(raw.i_blocks_lo, (nblocks * (bs / 512)) as u32);
        fs.write_raw_inode(self.ino, &raw)
            .map_err(|_| VfsError::Io)?;
        fs.dev.flush().map_err(|_| VfsError::Io)?;
        Ok(done)
    }

    /// Remove `name` from this directory: drop its dirent and free the child's
    /// inode + (inline) data blocks.  If `dir_only`, require the child to be an
    /// empty directory.
    fn remove_entry(&self, name: &str, dir_only: bool) -> VfsResult<()> {
        // Check write permission on parent directory
        let stat = self.stat()?;
        if !crate::user::check_permission(&stat, crate::user::PermissionOperation::Write) {
            return Err(VfsError::PermDenied);
        }

        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(VfsError::InvalidArg);
        }
        let fs = self.fs.lock();
        let mut parent = fs.read_raw_inode(self.ino).map_err(|_| VfsError::Io)?;
        let bs = fs.block_size as u64;
        let size = (pf!(parent.i_size_lo) as u64) | ((pf!(parent.i_size_hi) as u64) << 32);
        let nblocks = size.div_ceil(bs) as u32;

        // Locate the child inode + the dir block holding its entry.
        let mut child_ino = 0u32;
        let mut hit_phys = 0u64;
        for lb in 0..nblocks {
            let phys = {
                let root = unsafe {
                    core::slice::from_raw_parts(
                        core::ptr::addr_of!(parent.i_block) as *const u8,
                        60,
                    )
                };
                super::extent::find_extent_pub(&fs, root, lb).map_err(|_| VfsError::Io)?
            };
            if phys == 0 {
                continue;
            }
            let blk = fs.read_block(phys).map_err(|_| VfsError::Io)?;
            let ino = super::dir::find_ino(&blk, name.as_bytes());
            if ino != 0 {
                child_ino = ino;
                hit_phys = phys;
                break;
            }
        }
        if child_ino == 0 {
            return Err(VfsError::NotFound);
        }

        // Inspect the child: type + (for rmdir) emptiness.
        let craw = fs.read_raw_inode(child_ino).map_err(|_| VfsError::Io)?;
        let cftype = mode_to_filetype(pf!(craw.i_mode));
        if dir_only {
            if cftype != FileType::Directory {
                return Err(VfsError::NotADir);
            }
            if !dir_is_empty(&fs, &craw)? {
                return Err(VfsError::NotEmpty);
            }
        } else if cftype == FileType::Directory {
            return Err(VfsError::IsDir);
        }

        // Remove the directory entry from the parent block.
        let mut blk = fs.read_block(hit_phys).map_err(|_| VfsError::Io)?;
        if !super::dir::remove_dirent(&mut blk, name.as_bytes()) {
            return Err(VfsError::NotFound);
        }
        fs.write_block(hit_phys, &blk).map_err(|_| VfsError::Io)?;

        if dir_only {
            // Removing a subdirectory: free its blocks + inode, and drop the
            // parent's link count by one (the child's ".." back-link is gone).
            free_inline_blocks(&fs, &craw);
            let _ = fs.free_inode(child_ino);
            let pl = pf!(parent.i_links_count);
            if pl > 0 {
                pfw!(parent.i_links_count, pl - 1);
            }
            fs.write_raw_inode(self.ino, &parent)
                .map_err(|_| VfsError::Io)?;
        } else {
            // Unlink a file: drop one hard link; free storage only at the last.
            let links = pf!(craw.i_links_count);
            if links <= 1 {
                free_inline_blocks(&fs, &craw);
                let _ = fs.free_inode(child_ino);
            } else {
                let mut c = craw;
                pfw!(c.i_links_count, links - 1);
                fs.write_raw_inode(child_ino, &c)
                    .map_err(|_| VfsError::Io)?;
            }
        }
        Ok(())
    }

    /// Insert a directory entry into this (parent) directory on disk, allocating
    /// and appending a new block if every existing block is full.
    fn add_dirent(
        &self,
        fs: &Ext4Fs,
        parent_raw: &mut Ext4Inode,
        ino: u32,
        dtype: u8,
        name: &[u8],
    ) -> VfsResult<()> {
        let bs = fs.block_size as u64;
        let size = (pf!(parent_raw.i_size_lo) as u64) | ((pf!(parent_raw.i_size_hi) as u64) << 32);
        let nblocks = size.div_ceil(bs) as u32;

        for lb in 0..nblocks {
            let phys = {
                let root = unsafe {
                    core::slice::from_raw_parts(
                        core::ptr::addr_of!(parent_raw.i_block) as *const u8,
                        60,
                    )
                };
                super::extent::find_extent_pub(fs, root, lb).map_err(|_| VfsError::Io)?
            };
            if phys == 0 {
                continue;
            }
            let mut blk = fs.read_block(phys).map_err(|_| VfsError::Io)?;
            if super::dir::block_has_name(&blk, name) {
                return Err(VfsError::AlreadyExists);
            }
            if super::dir::insert_into_block(&mut blk, ino, dtype, name) {
                fs.write_block(phys, &blk).map_err(|_| VfsError::Io)?;
                return Ok(());
            }
        }

        // No room: allocate a fresh block holding one block-spanning empty entry.
        let newblk = fs.alloc_block().map_err(|_| VfsError::NoSpace)? as u32;
        let mut blk = alloc::vec![0u8; fs.block_size];
        blk[4] = (fs.block_size & 0xFF) as u8;
        blk[5] = ((fs.block_size >> 8) & 0xFF) as u8;
        if !super::dir::insert_into_block(&mut blk, ino, dtype, name) {
            return Err(VfsError::NoSpace);
        }
        fs.write_block(newblk as u64, &blk)
            .map_err(|_| VfsError::Io)?;
        super::extent::append_inline_extent(parent_raw, nblocks, newblk)
            .map_err(|_| VfsError::NoSpace)?;
        let new_size = (nblocks as u64 + 1) * bs;
        pfw!(parent_raw.i_size_lo, (new_size & 0xFFFF_FFFF) as u32);
        pfw!(parent_raw.i_size_hi, (new_size >> 32) as u32);
        let pblocks = pf!(parent_raw.i_blocks_lo);
        pfw!(parent_raw.i_blocks_lo, pblocks + (bs / 512) as u32);
        Ok(())
    }
}

/// True if directory inode `raw` contains only "." and "..".
/// Returns `Ok(true)` only if every block could be read and parsed and held no
/// real children.  Read/parse failures fail closed (`Err`) so rmdir refuses to
/// delete a directory it cannot fully verify as empty.
fn dir_is_empty(fs: &Ext4Fs, raw: &Ext4Inode) -> VfsResult<bool> {
    let bs = fs.block_size as u64;
    let size = (pf!(raw.i_size_lo) as u64) | ((pf!(raw.i_size_hi) as u64) << 32);
    let nblocks = size.div_ceil(bs) as u32;
    for lb in 0..nblocks {
        let phys = {
            let root = unsafe {
                core::slice::from_raw_parts(core::ptr::addr_of!(raw.i_block) as *const u8, 60)
            };
            super::extent::find_extent_pub(fs, root, lb).map_err(|_| VfsError::Io)?
        };
        if phys == 0 {
            continue;
        }
        let blk = fs.read_block(phys).map_err(|_| VfsError::Io)?;
        let entries = super::dir::parse_dirents(&blk, 0)?;
        for e in entries {
            if e.name != "." && e.name != ".." {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Free the data blocks referenced by an inline (depth-0) extent tree.
/// Indexed (depth>0) trees are left untouched (blocks leak rather than risk
/// clearing the wrong bitmap bits).
fn free_inline_blocks(fs: &Ext4Fs, raw: &Ext4Inode) {
    let root =
        unsafe { core::slice::from_raw_parts(core::ptr::addr_of!(raw.i_block) as *const u8, 60) };
    if root.len() < 12 || root[0] != 0x0A || root[1] != 0xF3 {
        return;
    }
    let depth = u16::from_le_bytes([root[6], root[7]]);
    if depth != 0 {
        return;
    } // only inline leaf extents
    let entries = u16::from_le_bytes([root[2], root[3]]) as usize;
    for i in 0..entries.min(4) {
        let off = 12 + i * 12;
        if off + 12 > 60 {
            break;
        }
        // ee_len high bit flags an unwritten extent; mask it for the real length.
        let mut len = u16::from_le_bytes([root[off + 4], root[off + 5]]) as u64;
        if len > 32768 {
            len -= 32768;
        }
        let lo = u32::from_le_bytes([root[off + 8], root[off + 9], root[off + 10], root[off + 11]])
            as u64;
        let hi = u16::from_le_bytes([root[off + 6], root[off + 7]]) as u64; // ee_start_hi
        let start = lo | (hi << 32);
        for b in 0..len {
            let _ = fs.free_block(start + b);
        }
    }
}

/// Map a VFS FileType to the ext4 dir_entry_2 file-type byte.
fn ext4_ftype(ft: FileType) -> u8 {
    match ft {
        FileType::RegularFile => 1,
        FileType::Directory => 2,
        FileType::CharDevice => 3,
        FileType::BlockDevice => 4,
        FileType::Pipe => 5,
        FileType::Socket => 6,
        FileType::SymLink => 7,
    }
}

impl InodeOps for Ext4InodeOps {
    fn stat(&self) -> VfsResult<Stat> {
        let size = self.file_size();
        let bs = {
            let f = self.fs.lock();
            f.block_size
        } as i64;
        Ok(Stat {
            st_ino: self.ino as u64,
            st_mode: pf!(self.raw.i_mode) as u32,
            st_nlink: pf!(self.raw.i_links_count) as u64,
            st_uid: pf!(self.raw.i_uid) as u32,
            st_gid: pf!(self.raw.i_gid) as u32,
            st_size: size as i64,
            st_blksize: bs,
            st_blocks: pf!(self.raw.i_blocks_lo) as i64,
            st_atime: pf!(self.raw.i_atime) as u64,
            st_mtime: pf!(self.raw.i_mtime) as u64,
            st_ctime: pf!(self.raw.i_ctime) as u64,
            ..Default::default()
        })
    }

    fn read(&self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        // Check read permission
        let stat = self.stat()?;
        if !crate::user::check_permission(&stat, crate::user::PermissionOperation::Read) {
            return Err(VfsError::PermDenied);
        }

        self.read_data(offset, buf)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        // Check write permission
        let stat = self.stat()?;
        if !crate::user::check_permission(&stat, crate::user::PermissionOperation::Write) {
            return Err(VfsError::PermDenied);
        }

        self.write_data(offset, buf)
    }

    fn readdir(&self, offset: u64) -> VfsResult<Vec<DirEntry>> {
        if mode_to_filetype(pf!(self.raw.i_mode)) != FileType::Directory {
            return Err(VfsError::NotADir);
        }
        // Clamp against a sane maximum — i_size is an on-disk field and could
        // be corrupted/malicious; a 64 MiB directory is already enormous.
        const MAX_DIR_SIZE: u64 = 64 * 1024 * 1024;
        let size = self.file_size().min(MAX_DIR_SIZE);
        let mut data = alloc::vec![0u8; size as usize];
        self.read_data(0, &mut data)?;
        super::dir::parse_dirents(&data, offset)
    }

    fn lookup(&self, name: &str) -> VfsResult<Arc<VfsInode>> {
        let entries = self.readdir(0)?;
        for e in entries {
            if e.name == name {
                let ino = e.inode as u32;
                let raw = {
                    let f = self.fs.lock();
                    f.read_raw_inode(ino).map_err(|_| VfsError::Io)?
                };
                let ftype = mode_to_filetype(pf!(raw.i_mode));
                let ops = Ext4InodeOps {
                    ino,
                    raw,
                    fs: self.fs.clone(),
                };
                return Ok(VfsInode::new(alloc_ino(), ftype, Arc::new(ops)));
            }
        }
        Err(VfsError::NotFound)
    }

    fn create(&self, name: &str, ftype: FileType, mode: u32) -> VfsResult<Arc<VfsInode>> {
        // Check write permission on parent directory
        let stat = self.stat()?;
        if !crate::user::check_permission(&stat, crate::user::PermissionOperation::Write) {
            return Err(VfsError::PermDenied);
        }

        if name.is_empty() || name.len() > 255 || name.contains('/') {
            return Err(VfsError::InvalidArg);
        }
        let (uid, gid, _, _) = crate::user::get_current_credentials();
        let fs = self.fs.lock();
        let mut parent_raw = fs.read_raw_inode(self.ino).map_err(|_| VfsError::Io)?;

        let child_ino = fs.alloc_inode().map_err(|_| VfsError::NoSpace)?;
        let mut cin = Ext4Inode::zeroed();
        let imode = (ftype.mode_bits() as u16) | (mode as u16 & 0o7777);

        pfw!(cin.i_mode, imode);
        pfw!(cin.i_uid, uid as u16);
        pfw!(cin.i_gid, gid as u16);
        pfw!(cin.i_links_count, 1u16);
        pfw!(cin.i_flags, EXT4_EXTENTS_FL);
        pfw!(cin.i_extra_isize, 32u16);
        super::extent::empty_extent_header(&mut cin);
        fs.write_raw_inode(child_ino, &cin)
            .map_err(|_| VfsError::Io)?;

        let dtype = ext4_ftype(ftype);
        // Roll back the just-allocated inode if linking it into the parent fails
        // (e.g. AlreadyExists/NoSpace) — otherwise it leaks, unreachable.
        if let Err(e) = self.add_dirent(&fs, &mut parent_raw, child_ino, dtype, name.as_bytes()) {
            let _ = fs.free_inode(child_ino);
            return Err(e);
        }
        fs.write_raw_inode(self.ino, &parent_raw)
            .map_err(|_| VfsError::Io)?;
        drop(fs);

        let ops = Ext4InodeOps {
            ino: child_ino,
            raw: cin,
            fs: self.fs.clone(),
        };
        Ok(VfsInode::new(alloc_ino(), ftype, Arc::new(ops)))
    }

    fn mkdir(&self, name: &str, mode: u32) -> VfsResult<Arc<VfsInode>> {
        // Check write permission on parent directory
        let stat = self.stat()?;
        if !crate::user::check_permission(&stat, crate::user::PermissionOperation::Write) {
            return Err(VfsError::PermDenied);
        }

        if name.is_empty() || name.len() > 255 || name.contains('/') {
            return Err(VfsError::InvalidArg);
        }
        let (uid, gid, _, _) = crate::user::get_current_credentials();
        let fs = self.fs.lock();
        let mut parent_raw = fs.read_raw_inode(self.ino).map_err(|_| VfsError::Io)?;
        let bs = fs.block_size;

        let child_ino = fs.alloc_inode().map_err(|_| VfsError::NoSpace)?;
        let dblock = fs.alloc_block().map_err(|_| VfsError::NoSpace)? as u32;

        // New directory inode: one data block, links_count=2 (itself + "..").
        let mut cin = Ext4Inode::zeroed();

        pfw!(cin.i_mode, S_IFDIR | (mode as u16 & 0o7777));
        pfw!(cin.i_uid, uid as u16);
        pfw!(cin.i_gid, gid as u16);
        pfw!(cin.i_links_count, 2u16);
        pfw!(cin.i_size_lo, bs as u32);
        pfw!(cin.i_blocks_lo, (bs / 512) as u32);
        pfw!(cin.i_flags, EXT4_EXTENTS_FL);
        pfw!(cin.i_extra_isize, 32u16);
        super::extent::set_single_extent(&mut cin, dblock, 1);
        fs.write_raw_inode(child_ino, &cin)
            .map_err(|_| VfsError::Io)?;

        // New directory data block: "." → child, ".." → parent (spans block).
        let mut blk = alloc::vec![0u8; bs];
        let p = super::dir::write_dirent(&mut blk, 0, child_ino, 2, b".");
        super::dir::write_dirent_span(&mut blk, p, self.ino, 2, b"..", bs);
        fs.write_block(dblock as u64, &blk)
            .map_err(|_| VfsError::Io)?;

        // Link into parent, then bump parent's link count for the new "..".
        // Roll back the inode + data block if linking fails, so neither leaks.
        if let Err(e) = self.add_dirent(&fs, &mut parent_raw, child_ino, 2, name.as_bytes()) {
            let _ = fs.free_block(dblock as u64);
            let _ = fs.free_inode(child_ino);
            return Err(e);
        }
        let pl = pf!(parent_raw.i_links_count);
        pfw!(parent_raw.i_links_count, pl + 1);
        fs.write_raw_inode(self.ino, &parent_raw)
            .map_err(|_| VfsError::Io)?;
        drop(fs);

        let ops = Ext4InodeOps {
            ino: child_ino,
            raw: cin,
            fs: self.fs.clone(),
        };
        Ok(VfsInode::new(
            alloc_ino(),
            FileType::Directory,
            Arc::new(ops),
        ))
    }

    fn unlink(&self, name: &str) -> VfsResult<()> {
        self.remove_entry(name, false)
    }
    fn rmdir(&self, name: &str) -> VfsResult<()> {
        self.remove_entry(name, true)
    }
    fn truncate(&self, size: u64) -> VfsResult<()> {
        // Check write permission on file
        let stat = self.stat()?;
        if !crate::user::check_permission(&stat, crate::user::PermissionOperation::Write) {
            return Err(VfsError::PermDenied);
        }

        let fs = self.fs.lock();
        let mut raw = fs.read_raw_inode(self.ino).map_err(|_| VfsError::Io)?;
        if mode_to_filetype(pf!(raw.i_mode)) != FileType::RegularFile {
            return Err(VfsError::InvalidArg);
        }
        let old = (pf!(raw.i_size_lo) as u64) | ((pf!(raw.i_size_hi) as u64) << 32);
        if size == 0 {
            // Free every data block and reset to an empty inline extent tree.
            free_inline_blocks(&fs, &raw);
            super::extent::empty_extent_header(&mut raw);
            pfw!(raw.i_blocks_lo, 0u32);
        }
        // A partial shrink (0 < size < old) keeps the blocks allocated; reads
        // are bounded by i_size so the truncated tail is not returned.  A grow
        // creates a sparse hole (blocks fault in on write).  Both just resize.
        let _ = old;
        pfw!(raw.i_size_lo, (size & 0xFFFF_FFFF) as u32);
        pfw!(raw.i_size_hi, (size >> 32) as u32);
        fs.write_raw_inode(self.ino, &raw)
            .map_err(|_| VfsError::Io)?;
        fs.dev.flush().map_err(|_| VfsError::Io)?;
        Ok(())
    }
    fn chmod(&self, mode: u32) -> VfsResult<()> {
        // Check if user is owner or root
        let stat = self.stat()?;
        let (uid, _, _, _) = crate::user::get_current_credentials();

        if uid != 0 && uid != stat.st_uid {
            return Err(VfsError::PermDenied);
        }

        let fs = self.fs.lock();
        let mut raw = fs.read_raw_inode(self.ino).map_err(|_| VfsError::Io)?;
        let ftbits = pf!(raw.i_mode) & 0xF000; // preserve the file-type bits
        pfw!(raw.i_mode, ftbits | (mode as u16 & 0o7777));
        fs.write_raw_inode(self.ino, &raw)
            .map_err(|_| VfsError::Io)?;
        Ok(())
    }
    fn chown(&self, uid: u32, gid: u32) -> VfsResult<()> {
        // Only root can change ownership
        let (_, _, euid, _) = crate::user::get_current_credentials();

        if euid != 0 {
            return Err(VfsError::PermDenied);
        }

        let fs = self.fs.lock();
        let mut raw = fs.read_raw_inode(self.ino).map_err(|_| VfsError::Io)?;
        pfw!(raw.i_uid, uid as u16);
        pfw!(raw.i_gid, gid as u16);
        fs.write_raw_inode(self.ino, &raw)
            .map_err(|_| VfsError::Io)?;
        Ok(())
    }
    fn symlink(&self, _name: &str, _target: &str) -> VfsResult<Arc<VfsInode>> {
        Err(VfsError::NotSupported)
    }
    fn link(&self, _name: &str, _target: &Arc<VfsInode>) -> VfsResult<()> {
        Err(VfsError::NotSupported)
    }

    fn rename(&self, _old: &str, _new_parent: &Arc<VfsInode>, _new: &str) -> VfsResult<()> {
        Err(VfsError::NotSupported)
    }

    fn readlink(&self) -> VfsResult<String> {
        // A symlink target can never legitimately exceed PATH_MAX (4096).
        let size = self.file_size().min(4096);
        if size == 0 {
            return Ok(String::new());
        }
        // Short symlinks are stored inline in i_block
        if size <= 60 {
            let i_block_copy: [u32; 15] =
                unsafe { core::ptr::addr_of!(self.raw.i_block).read_unaligned() };
            let bytes = unsafe {
                core::slice::from_raw_parts(i_block_copy.as_ptr() as *const u8, size as usize)
            };
            return String::from_utf8(bytes.to_vec()).map_err(|_| VfsError::Io);
        }
        let mut buf = alloc::vec![0u8; size as usize];
        self.read_data(0, &mut buf)?;
        String::from_utf8(buf).map_err(|_| VfsError::Io)
    }
}

// -- Legacy block addressing (non-extent inodes) ----------------------------

fn get_block_no(fs: &Ext4Fs, i_block: &[u32; 15], idx: usize) -> Result<u32, &'static str> {
    let bs = fs.block_size;
    let addrs_per_block = bs / 4;

    if idx < 12 {
        return Ok(i_block[idx]);
    }
    let idx = idx - 12;
    if idx < addrs_per_block {
        // Single indirect
        let ind_blk = fs.read_block(i_block[12] as u64)?;
        let no = u32::from_le_bytes(ind_blk[idx * 4..(idx + 1) * 4].try_into().unwrap_or([0; 4]));
        return Ok(no);
    }
    let idx = idx - addrs_per_block;
    if idx < addrs_per_block * addrs_per_block {
        // Double indirect
        let dind_blk = fs.read_block(i_block[13] as u64)?;
        let a = idx / addrs_per_block;
        let b = idx % addrs_per_block;
        let ind_no = u32::from_le_bytes(dind_blk[a * 4..(a + 1) * 4].try_into().unwrap_or([0; 4]));
        let ind_blk = fs.read_block(ind_no as u64)?;
        let no = u32::from_le_bytes(ind_blk[b * 4..(b + 1) * 4].try_into().unwrap_or([0; 4]));
        return Ok(no);
    }
    Err("ext4: triple-indirect blocks not supported")
}
