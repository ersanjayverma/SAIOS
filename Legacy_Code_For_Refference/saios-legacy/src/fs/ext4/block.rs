//! ext4 block allocator (for write support).

use super::Ext4Fs;

impl Ext4Fs {
    /// Adjust a group descriptor's free-block counter by `delta` and write back.
    fn adjust_group_free_blocks(&self, g: u32, delta: i32) {
        if let Ok(mut gd) = self.read_group_desc(g) {
            let cur = unsafe { core::ptr::addr_of!(gd.bg_free_blocks_lo).read_unaligned() } as i32;
            let nv = (cur + delta).max(0) as u16;
            unsafe {
                core::ptr::addr_of_mut!(gd.bg_free_blocks_lo).write_unaligned(nv);
            }
            let _ = self.write_group_desc(g, &gd);
        }
    }

    /// Adjust a group descriptor's free-inode counter by `delta` and write back.
    fn adjust_group_free_inodes(&self, g: u32, delta: i32) {
        if let Ok(mut gd) = self.read_group_desc(g) {
            let cur = unsafe { core::ptr::addr_of!(gd.bg_free_inodes_lo).read_unaligned() } as i32;
            let nv = (cur + delta).max(0) as u16;
            unsafe {
                core::ptr::addr_of_mut!(gd.bg_free_inodes_lo).write_unaligned(nv);
            }
            let _ = self.write_group_desc(g, &gd);
        }
    }

    /// Allocate a new block, returns block number or error.
    pub fn alloc_block(&self) -> Result<u64, &'static str> {
        for g in 0..self.group_count {
            let gd = self.read_group_desc(g)?;
            let free = unsafe { core::ptr::addr_of!(gd.bg_free_blocks_lo).read_unaligned() } as u32;
            if free == 0 {
                continue;
            }

            let bitmap_lo =
                (unsafe { core::ptr::addr_of!(gd.bg_block_bitmap_lo).read_unaligned() }) as u64;
            let bitmap_hi = if self.feature_incompat & super::INCOMPAT_64BIT != 0 {
                (unsafe { core::ptr::addr_of!(gd.bg_block_bitmap_hi).read_unaligned() }) as u64
            } else {
                0
            };
            let bitmap_block = bitmap_lo | (bitmap_hi << 32);

            let mut bitmap = self.read_block(bitmap_block)?;
            for (byte_idx, byte) in bitmap.iter_mut().enumerate() {
                if *byte == 0xFF {
                    continue;
                }
                let bit = (!*byte).trailing_zeros() as usize;
                let block_idx =
                    g as u64 * self.blocks_per_group as u64 + (byte_idx * 8 + bit) as u64;
                *byte |= 1 << bit;
                self.write_block(bitmap_block, &bitmap)?;
                // Keep free counters consistent on disk.
                self.adjust_group_free_blocks(g, -1);
                let _ = self.adjust_super_counts(-1, 0);
                // Zero the new block
                let zeroes = alloc::vec![0u8; self.block_size];
                self.write_block(block_idx, &zeroes)?;
                return Ok(block_idx);
            }
        }
        Err("ext4: disk full")
    }

    /// Free a data block: clear its bit in the owning group's block bitmap.
    pub fn free_block(&self, block: u64) -> Result<(), &'static str> {
        let bpg = self.blocks_per_group as u64;
        let g = (block / bpg) as u32;
        let idx = (block % bpg) as usize;
        let gd = self.read_group_desc(g)?;
        let lo = (unsafe { core::ptr::addr_of!(gd.bg_block_bitmap_lo).read_unaligned() }) as u64;
        let hi = if self.feature_incompat & super::INCOMPAT_64BIT != 0 {
            (unsafe { core::ptr::addr_of!(gd.bg_block_bitmap_hi).read_unaligned() }) as u64
        } else {
            0
        };
        let bb = lo | (hi << 32);
        let mut bm = self.read_block(bb)?;
        if idx / 8 >= bm.len() {
            return Ok(());
        }
        if bm[idx / 8] & (1 << (idx % 8)) == 0 {
            return Ok(());
        } // already free
        bm[idx / 8] &= !(1 << (idx % 8));
        self.write_block(bb, &bm)?;
        self.adjust_group_free_blocks(g, 1);
        let _ = self.adjust_super_counts(1, 0);
        Ok(())
    }

    /// Free an inode: clear its bit in the owning group's inode bitmap (1-based).
    pub fn free_inode(&self, ino: u32) -> Result<(), &'static str> {
        if ino == 0 {
            return Ok(());
        }
        let ipg = self.inodes_per_group;
        let g = (ino - 1) / ipg;
        let idx = ((ino - 1) % ipg) as usize;
        let gd = self.read_group_desc(g)?;
        let ib = (unsafe { core::ptr::addr_of!(gd.bg_inode_bitmap_lo).read_unaligned() }) as u64;
        let mut bm = self.read_block(ib)?;
        if idx / 8 >= bm.len() {
            return Ok(());
        }
        if bm[idx / 8] & (1 << (idx % 8)) == 0 {
            return Ok(());
        }
        bm[idx / 8] &= !(1 << (idx % 8));
        self.write_block(ib, &bm)?;
        self.adjust_group_free_inodes(g, 1);
        let _ = self.adjust_super_counts(0, 1);
        Ok(())
    }

    /// Allocate a new inode number.
    pub fn alloc_inode(&self) -> Result<u32, &'static str> {
        for g in 0..self.group_count {
            let gd = self.read_group_desc(g)?;
            if unsafe { core::ptr::addr_of!(gd.bg_free_inodes_lo).read_unaligned() } == 0 {
                continue;
            }

            let bitmap_block =
                (unsafe { core::ptr::addr_of!(gd.bg_inode_bitmap_lo).read_unaligned() }) as u64;
            let mut bitmap = self.read_block(bitmap_block)?;
            for (byte_idx, byte) in bitmap.iter_mut().enumerate() {
                if *byte == 0xFF {
                    continue;
                }
                let bit = (!*byte).trailing_zeros() as usize;
                let ino = g * self.inodes_per_group + (byte_idx * 8 + bit) as u32 + 1;
                *byte |= 1 << bit;
                self.write_block(bitmap_block, &bitmap)?;
                self.adjust_group_free_inodes(g, -1);
                let _ = self.adjust_super_counts(0, -1);
                return Ok(ino);
            }
        }
        Err("ext4: inode table full")
    }
}
