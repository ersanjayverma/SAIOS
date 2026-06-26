//! POSIX tar (ustar) archive parser.
//!
//! Used to extract files from `data.tar.xz` and `control.tar.xz`
//! inside Debian packages.
//!
//! Each tar entry consists of:
//!   - 512-byte header block (POSIX ustar format)
//!   - Zero or more 512-byte data blocks
//!
//! Header format (ustar):
//!   Offset  Size  Field
//!   0       100   File name
//!   100     8     Mode (octal ASCII)
//!   108     8     UID  (octal ASCII)
//!   116     8     GID  (octal ASCII)
//!   124     12    File size (octal ASCII)
//!   136     12    Modification time (octal ASCII)
//!   148     8     Header checksum
//!   156     1     Type flag
//!   157     100   Link name
//!   257     6     "ustar" magic
//!   265     2     Version ("00")
//!   265     32    User name
//!   297     32    Group name
//!   329     8     Device major
//!   337     8     Device minor
//!   345     155   Filename prefix

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// One file extracted from a tar archive.
#[derive(Debug, Clone)]
pub struct TarEntry {
    /// Full path of the file (prefix + name).
    pub path: String,
    /// File type: b'0'=file, b'2'=symlink, b'5'=dir, b'L'=long-name (GNU)
    pub ftype: u8,
    /// File permissions (e.g. 0o644).
    pub mode: u32,
    /// File contents (empty for directories and symlinks).
    pub data: Vec<u8>,
    /// Symlink target (populated for ftype=b'2').
    pub link: String,
}

/// Parse a tar archive (plain, not compressed). Returns all entries.
pub fn parse(data: &[u8]) -> Result<Vec<TarEntry>, &'static str> {
    let mut entries = Vec::new();
    let mut pos = 0usize;
    let mut gnu_long_name: Option<String> = None;

    while pos + 512 <= data.len() {
        let header = &data[pos..pos + 512];
        pos += 512;

        // End of archive: two consecutive zero blocks
        if header.iter().all(|&b| b == 0) {
            if pos + 512 <= data.len() && data[pos..pos + 512].iter().all(|&b| b == 0) {
                break;
            }
            continue;
        }

        // Validate checksum (optional — we skip invalid entries)
        if !valid_checksum(header) {
            continue;
        }

        let name_raw = parse_str(&header[0..100]);
        let mode = parse_octal(&header[100..108]) as u32;
        let size = parse_octal(&header[124..136]) as usize;
        let ftype = header[156];
        let link_raw = parse_str(&header[157..257]);
        let prefix_raw = parse_str(&header[345..500]);

        let strip_dot_slash = |s: &str| -> String {
            s.trim_start_matches('.')
                .trim_start_matches('/')
                .to_string()
        };
        let name = if !prefix_raw.is_empty() {
            let pre = prefix_raw.trim_end_matches('/');
            let base = strip_dot_slash(&name_raw);
            format!("{}/{}", pre, base)
        } else {
            strip_dot_slash(&name_raw)
        };

        // Round up data size to 512-byte boundary
        let data_blocks = size.div_ceil(512);

        if ftype == b'L' {
            // GNU long filename extension
            let long_name = if pos + size <= data.len() {
                parse_str(&data[pos..pos + size])
            } else {
                name.clone()
            };
            gnu_long_name = Some(long_name);
            pos += data_blocks * 512;
            continue;
        }

        let real_name = gnu_long_name.take().unwrap_or(name);

        let entry_data = if ftype == b'0' || ftype == b'\0' {
            // Regular file — read data
            if pos + size > data.len() {
                return Err("tar: file data extends past end of archive");
            }
            let d = data[pos..pos + size].to_vec();
            pos += data_blocks * 512;
            d
        } else {
            pos += data_blocks * 512;
            Vec::new()
        };

        entries.push(TarEntry {
            path: real_name,
            ftype,
            mode,
            data: entry_data,
            link: link_raw,
        });
    }

    Ok(entries)
}

/// Extract all files from a (possibly compressed) tar archive into the VFS.
///
/// `prefix` is stripped from each path (e.g. `"."` or `"./usr"`).
/// `dest`   is the VFS destination root (e.g. `"/"`).
pub fn extract_to_vfs(tar_data: &[u8], dest: &str) -> Result<Vec<String>, &'static str> {
    let entries = parse(tar_data)?;
    let mut installed = Vec::new();

    for entry in &entries {
        let path = entry.path.trim_start_matches("./");
        if path.is_empty() || path == "." {
            continue;
        }

        let full_path = if dest.trim_end_matches('/').is_empty() || dest == "/" {
            alloc::format!("/{}", path)
        } else {
            alloc::format!("{}/{}", dest.trim_end_matches('/'), path)
        };

        match entry.ftype {
            b'5' => {
                // Directory — create it
                crate::mkdir_p_pub(&full_path);
            }
            b'2' => {
                // Symlink — create in VFS
                let _ = crate::vfs_contract::VfsContract::symlink(&full_path, &entry.link);
                installed.push(alloc::format!("l {}", full_path));
            }
            b'0' | b'\0' => {
                // Regular file
                if let Some(last_slash) = full_path.rfind('/') {
                    let dir = &full_path[..last_slash.max(1)];
                    crate::mkdir_p_pub(dir);
                }
                if crate::vfs_contract::VfsContract::write_file(&full_path, &entry.data, entry.mode)
                    .is_ok()
                {
                    let _ = crate::vfs_contract::VfsContract::chmod(&full_path, entry.mode);
                }
                installed.push(full_path.clone());
            }
            _ => {} // ignore other types (block/char devices etc.)
        }
    }

    Ok(installed)
}

// -- Helpers ----------------------------------------------------------------

fn parse_str(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim().to_string()
}

fn parse_octal(raw: &[u8]) -> u64 {
    let s = parse_str(raw);
    u64::from_str_radix(s.trim(), 8).unwrap_or(0)
}

fn valid_checksum(header: &[u8]) -> bool {
    // Sum all bytes treating the checksum field (148..156) as spaces
    let stored = parse_octal(&header[148..156]);
    let mut sum = 0u32;
    for (i, &b) in header.iter().enumerate() {
        sum += if (148..156).contains(&i) {
            32
        } else {
            b as u32
        };
    }
    sum as u64 == stored || (sum as i64 - 256) as u64 == stored
}
