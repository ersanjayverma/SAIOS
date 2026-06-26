//! ext4 directory entry parsing (linear and htree).

use crate::vfs::{DirEntry, FileType, VfsError, VfsResult};
use alloc::string::String;
use alloc::vec::Vec;

// dir_entry_2 file types
fn ftype_from_ext4(t: u8) -> FileType {
    match t {
        1 => FileType::RegularFile,
        2 => FileType::Directory,
        3 => FileType::CharDevice,
        4 => FileType::BlockDevice,
        5 => FileType::Pipe,
        6 => FileType::Socket,
        7 => FileType::SymLink,
        _ => FileType::RegularFile,
    }
}

/// 4-byte-aligned minimal record length needed to hold a name.
fn dirent_size(name_len: usize) -> usize {
    (8 + name_len).div_ceil(4) * 4
}

/// Write a directory entry at `pos`; returns the position after it.
pub fn write_dirent(buf: &mut [u8], pos: usize, ino: u32, ftype: u8, name: &[u8]) -> usize {
    let rec = dirent_size(name.len());
    if pos + rec > buf.len() {
        return pos;
    }
    buf[pos..pos + 4].copy_from_slice(&ino.to_le_bytes());
    buf[pos + 4] = (rec & 0xFF) as u8;
    buf[pos + 5] = (rec >> 8) as u8;
    buf[pos + 6] = name.len() as u8;
    buf[pos + 7] = ftype;
    buf[pos + 8..pos + 8 + name.len()].copy_from_slice(name);
    pos + rec
}

/// Write the final directory entry, extending its rec_len to span to the end of
/// the block (the standard ext4 convention for the last entry in a block).
pub fn write_dirent_span(
    buf: &mut [u8],
    pos: usize,
    ino: u32,
    ftype: u8,
    name: &[u8],
    block_size: usize,
) {
    if pos + 8 > buf.len() || block_size <= pos {
        return;
    }
    let rec = block_size - pos;
    if rec < dirent_size(name.len()) {
        return;
    }
    buf[pos..pos + 4].copy_from_slice(&ino.to_le_bytes());
    buf[pos + 4] = (rec & 0xFF) as u8;
    buf[pos + 5] = (rec >> 8) as u8;
    buf[pos + 6] = name.len() as u8;
    buf[pos + 7] = ftype;
    buf[pos + 8..pos + 8 + name.len()].copy_from_slice(name);
}

/// Does this directory block already contain `name`?
pub fn block_has_name(buf: &[u8], name: &[u8]) -> bool {
    let mut pos = 0usize;
    while pos + 8 <= buf.len() {
        let e_ino = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        let rec_len = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]) as usize;
        let name_len = buf[pos + 6] as usize;
        if rec_len < 8 {
            break;
        }
        if e_ino != 0
            && name_len == name.len()
            && pos + 8 + name_len <= buf.len()
            && &buf[pos + 8..pos + 8 + name_len] == name
        {
            return true;
        }
        pos += rec_len;
    }
    false
}

/// Try to insert a new entry into a single directory block by splitting the
/// slack of an existing entry.  Returns true if inserted (buf mutated).
pub fn insert_into_block(buf: &mut [u8], ino: u32, ftype: u8, name: &[u8]) -> bool {
    let need = dirent_size(name.len());
    let mut pos = 0usize;
    while pos + 8 <= buf.len() {
        let e_ino = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        let rec_len = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]) as usize;
        let name_len = buf[pos + 6] as usize;
        if rec_len < 8 || pos + rec_len > buf.len() {
            break;
        }
        let used = if e_ino == 0 { 0 } else { dirent_size(name_len) };
        let slack = rec_len.saturating_sub(used);
        if slack >= need {
            let new_pos = pos + used;
            let new_rec = rec_len - used;
            if e_ino != 0 {
                buf[pos + 4] = (used & 0xFF) as u8;
                buf[pos + 5] = (used >> 8) as u8;
            }
            buf[new_pos..new_pos + 4].copy_from_slice(&ino.to_le_bytes());
            buf[new_pos + 4] = (new_rec & 0xFF) as u8;
            buf[new_pos + 5] = (new_rec >> 8) as u8;
            buf[new_pos + 6] = name.len() as u8;
            buf[new_pos + 7] = ftype;
            buf[new_pos + 8..new_pos + 8 + name.len()].copy_from_slice(name);
            return true;
        }
        pos += rec_len;
    }
    false
}

/// Find the inode number of `name` in a directory block (0 if absent).
pub fn find_ino(buf: &[u8], name: &[u8]) -> u32 {
    let mut pos = 0usize;
    while pos + 8 <= buf.len() {
        let ino = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        let rec = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]) as usize;
        let nlen = buf[pos + 6] as usize;
        if rec < 8 || pos + rec > buf.len() {
            break;
        }
        if ino != 0
            && nlen == name.len()
            && pos + 8 + nlen <= buf.len()
            && &buf[pos + 8..pos + 8 + nlen] == name
        {
            return ino;
        }
        pos += rec;
    }
    0
}

/// Remove `name` from a directory block by merging its slot into the previous
/// entry (or zeroing the inode if it is the first).  Returns true if removed.
pub fn remove_dirent(buf: &mut [u8], name: &[u8]) -> bool {
    let mut pos = 0usize;
    let mut prev = usize::MAX;
    while pos + 8 <= buf.len() {
        let ino = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]);
        let rec = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]) as usize;
        let nlen = buf[pos + 6] as usize;
        if rec < 8 || pos + rec > buf.len() {
            break;
        }
        if ino != 0
            && nlen == name.len()
            && pos + 8 + nlen <= buf.len()
            && &buf[pos + 8..pos + 8 + nlen] == name
        {
            if prev != usize::MAX {
                let prev_rec = u16::from_le_bytes([buf[prev + 4], buf[prev + 5]]) as usize;
                let merged = prev_rec + rec;
                buf[prev + 4] = (merged & 0xFF) as u8;
                buf[prev + 5] = (merged >> 8) as u8;
            } else {
                buf[pos] = 0;
                buf[pos + 1] = 0;
                buf[pos + 2] = 0;
                buf[pos + 3] = 0;
            }
            return true;
        }
        prev = pos;
        pos += rec;
    }
    false
}

/// Parse a raw directory block into DirEntry list, starting at `offset` bytes.
pub fn parse_dirents(data: &[u8], offset: u64) -> VfsResult<Vec<DirEntry>> {
    let mut entries = Vec::new();
    let mut pos = offset as usize;

    while pos + 8 <= data.len() {
        let inode = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let rec_len = u16::from_le_bytes([data[pos + 4], data[pos + 5]]) as usize;
        let name_len = data[pos + 6] as usize;
        let ftype = data[pos + 7];

        if rec_len < 8 || pos + rec_len > data.len() {
            break;
        }
        if inode != 0 && name_len > 0 {
            let name_end = (pos + 8 + name_len).min(data.len());
            let name_bytes = &data[pos + 8..name_end];
            if let Ok(name) = core::str::from_utf8(name_bytes) {
                entries.push(DirEntry {
                    name: String::from(name),
                    inode: inode as u64,
                    ftype: ftype_from_ext4(ftype),
                });
            }
        }
        pos += rec_len;
        if rec_len == 0 {
            break;
        } // safety: avoid infinite loop
    }
    Ok(entries)
}
