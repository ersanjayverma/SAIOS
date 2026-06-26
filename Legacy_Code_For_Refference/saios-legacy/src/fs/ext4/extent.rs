//! ext4 extent tree reader/writer.
macro_rules! pf {
    ($x:expr) => {
        unsafe { core::ptr::addr_of!($x).read_unaligned() }
    };
}

use super::Ext4Fs;
use super::inode::Ext4Inode;
use alloc::vec::Vec;

const EXT4_EXT_MAGIC: u16 = 0xF30A;

#[repr(C, packed)]
struct ExtentHeader {
    eh_magic: u16,
    eh_entries: u16,
    eh_max: u16,
    eh_depth: u16,
    eh_generation: u32,
}

#[repr(C, packed)]
struct ExtentIdx {
    ei_block: u32,
    ei_leaf_lo: u32,
    ei_leaf_hi: u16,
    ei_unused: u16,
}

#[repr(C, packed)]
struct Extent {
    ee_block: u32,
    ee_len: u16,
    ee_start_hi: u16,
    ee_start_lo: u32,
}

/// Read data from an extent tree.
/// `extent_root` is the 60-byte i_block data.
/// Returns number of bytes read.
pub fn read_extent_tree(
    fs: &Ext4Fs,
    extent_root: &[u8],
    file_offset: u64,
    buf: &mut [u8],
) -> Result<usize, &'static str> {
    let hdr = unsafe { core::ptr::read_unaligned(extent_root.as_ptr() as *const ExtentHeader) };
    if pf!(hdr.eh_magic) != EXT4_EXT_MAGIC {
        return Err("ext4: bad extent magic");
    }

    let bs = fs.block_size;
    let mut done = 0usize;

    while done < buf.len() {
        let file_off = file_offset + done as u64;
        let logical_block = (file_off / bs as u64) as u32;
        let block_off = (file_off % bs as u64) as usize;

        let phys = find_extent(fs, extent_root, logical_block, 0)?;
        if phys == 0 {
            // Sparse hole
            let to_fill = (bs - block_off).min(buf.len() - done);
            buf[done..done + to_fill].fill(0);
            done += to_fill;
            continue;
        }
        let blk_data = fs.read_block(phys).map_err(|_| "ext4: block read error")?;
        let to_copy = (bs - block_off).min(buf.len() - done);
        buf[done..done + to_copy].copy_from_slice(&blk_data[block_off..block_off + to_copy]);
        done += to_copy;
    }
    Ok(done)
}

fn find_extent(fs: &Ext4Fs, data: &[u8], logical: u32, depth: u32) -> Result<u64, &'static str> {
    if depth > 5 {
        return Err("ext4: extent tree too deep");
    }
    let hdr = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const ExtentHeader) };
    let entries = pf!(hdr.eh_entries) as usize;
    let tree_depth = pf!(hdr.eh_depth);

    if tree_depth == 0 {
        for i in 0..entries {
            let off = 12 + i * 12;
            if off + 12 > data.len() {
                break;
            }
            let ext = unsafe { core::ptr::read_unaligned(data[off..].as_ptr() as *const Extent) };
            let start = ext.ee_block;
            let raw_len = ext.ee_len as u32;
            let len = if raw_len > 0x8000 {
                raw_len - 0x8000
            } else {
                raw_len
            };
            if logical >= start && logical < start + len {
                let phys_lo = ext.ee_start_lo as u64;
                let phys_hi = ext.ee_start_hi as u64;
                return Ok((phys_lo | (phys_hi << 32)) + (logical - start) as u64);
            }
        }
        return Ok(0);
    }

    for i in (0..entries).rev() {
        let off = 12 + i * 12;
        if off + 12 > data.len() {
            break;
        }
        let idx = unsafe { core::ptr::read_unaligned(data[off..].as_ptr() as *const ExtentIdx) };
        if logical >= idx.ei_block {
            let leaf_lo = idx.ei_leaf_lo as u64;
            let leaf_hi = idx.ei_leaf_hi as u64;
            let leaf_block = leaf_lo | (leaf_hi << 32);
            let child_data = fs
                .read_block(leaf_block)
                .map_err(|_| "ext4: index read fail")?;
            return find_extent(fs, &child_data, logical, depth + 1);
        }
    }
    Ok(0)
}

/// Public wrapper: map a logical block to a physical block via the inline
/// extent tree rooted in `data` (the 60-byte i_block).  Returns 0 for a hole.
pub fn find_extent_pub(fs: &Ext4Fs, data: &[u8], logical: u32) -> Result<u64, &'static str> {
    find_extent(fs, data, logical, 0)
}

/// Initialise a leaf extent header with zero entries in the 60-byte i_block.
pub fn empty_extent_header(raw: &mut Ext4Inode) {
    let root = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(raw.i_block) as *mut u8, 60)
    };
    for b in root.iter_mut() {
        *b = 0;
    }
    root[0] = 0x0A;
    root[1] = 0xF3; // eh_magic = 0xF30A
    root[4] = 4; // eh_max = 4
}

/// Write a single-extent leaf into the 60-byte i_block: logical 0 → `phys`,
/// length `len` blocks.
pub fn set_single_extent(raw: &mut Ext4Inode, phys: u32, len: u16) {
    let root = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(raw.i_block) as *mut u8, 60)
    };
    for b in root.iter_mut() {
        *b = 0;
    }
    root[0] = 0x0A;
    root[1] = 0xF3; // eh_magic
    root[2] = 1; // eh_entries = 1
    root[4] = 4; // eh_max = 4
    // extent record at offset 12: ee_block(4)=0, ee_len(2), ee_start_hi(2)=0, ee_start_lo(4)
    root[16..18].copy_from_slice(&len.to_le_bytes());
    root[20..24].copy_from_slice(&phys.to_le_bytes());
}

/// Append one physical block at `logical` to the inline leaf extent tree,
/// coalescing with the last extent when contiguous.  Errors if the inline tree
/// is full (max 4 extents) or has interior nodes.
pub fn append_inline_extent(
    raw: &mut Ext4Inode,
    logical: u32,
    phys: u32,
) -> Result<(), &'static str> {
    let root = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(raw.i_block) as *mut u8, 60)
    };
    if root[0] != 0x0A || root[1] != 0xF3 {
        for b in root.iter_mut() {
            *b = 0;
        }
        root[0] = 0x0A;
        root[1] = 0xF3;
        root[4] = 4;
    }
    let depth = u16::from_le_bytes([root[6], root[7]]);
    if depth != 0 {
        return Err("ext4: cannot extend an indexed extent tree");
    }
    let mut entries = u16::from_le_bytes([root[2], root[3]]) as usize;

    if entries > 0 {
        let off = 12 + (entries - 1) * 12;
        let e_block = u32::from_le_bytes([root[off], root[off + 1], root[off + 2], root[off + 3]]);
        let e_len = u16::from_le_bytes([root[off + 4], root[off + 5]]);
        let e_phys =
            u32::from_le_bytes([root[off + 8], root[off + 9], root[off + 10], root[off + 11]]);
        if e_block + e_len as u32 == logical && e_phys + e_len as u32 == phys && e_len < 0x7FFF {
            let nl = e_len + 1;
            root[off + 4] = (nl & 0xFF) as u8;
            root[off + 5] = (nl >> 8) as u8;
            return Ok(());
        }
    }

    let max = u16::from_le_bytes([root[4], root[5]]) as usize;
    if entries >= max {
        return Err("ext4: inline extent tree full");
    }
    let off = 12 + entries * 12;
    root[off..off + 4].copy_from_slice(&logical.to_le_bytes());
    root[off + 4] = 1;
    root[off + 5] = 0; // ee_len = 1
    root[off + 6] = 0;
    root[off + 7] = 0; // ee_start_hi = 0
    root[off + 8..off + 12].copy_from_slice(&phys.to_le_bytes());
    entries += 1;
    root[2] = (entries & 0xFF) as u8;
    root[3] = (entries >> 8) as u8;
    Ok(())
}

/// Write data via the extent tree (allocates new extents as needed).
pub fn write_extent_tree(
    fs: &Ext4Fs,
    raw_inode: &Ext4Inode,
    ino: u32,
    offset: u64,
    buf: &[u8],
) -> Result<usize, &'static str> {
    // For Phase 1 write support — just write to existing extents
    // Full allocation (creating new extents) is TODO
    let i_block_copy: [u32; 15] =
        unsafe { core::ptr::addr_of!(raw_inode.i_block).read_unaligned() };
    let extent_root =
        unsafe { core::slice::from_raw_parts(i_block_copy.as_ptr() as *const u8, 60) };
    let bs = fs.block_size;
    let mut done = 0usize;

    while done < buf.len() {
        let file_off = offset + done as u64;
        let logical = (file_off / bs as u64) as u32;
        let block_off = (file_off % bs as u64) as usize;

        let phys = find_extent(fs, extent_root, logical, 0)?;
        if phys == 0 {
            return Err("ext4: write to sparse block NYI");
        }

        let mut blk = fs.read_block(phys).map_err(|_| "ext4: write read-fail")?;
        let to_copy = (bs - block_off).min(buf.len() - done);
        blk[block_off..block_off + to_copy].copy_from_slice(&buf[done..done + to_copy]);
        fs.write_block(phys, &blk).map_err(|_| "ext4: write fail")?;
        done += to_copy;
    }
    Ok(done)
}
