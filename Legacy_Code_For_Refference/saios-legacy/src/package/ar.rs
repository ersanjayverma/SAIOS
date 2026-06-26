//! `ar` archive parser — used as the .deb container format.
//!
//! .deb files are `ar` archives containing exactly three members:
//!   1. `debian-binary`   — "2.0\n" (version string)
//!   2. `control.tar.xz`  — package metadata (control file, scripts)
//!   3. `data.tar.xz`     — actual files to install
//!
//! The ar format is simple:
//!   8-byte global header: "!<arch>\n"
//!   Per-file headers (60 bytes each):
//!     name[16]  date[12]  uid[6]  gid[6]  mode[8]  size[10]  end[2]
//!   File data (padded to even byte boundary)

use alloc::string::{String, ToString};
use alloc::vec::Vec;

const AR_MAGIC: &[u8; 8] = b"!<arch>\n";
const AR_END: &[u8; 2] = b"`\n";

/// One member of an ar archive.
pub struct ArEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// Parse an ar archive and return all its members.
pub fn parse(data: &[u8]) -> Result<Vec<ArEntry>, &'static str> {
    if data.len() < 8 {
        return Err("ar: too short");
    }
    if &data[..8] != AR_MAGIC {
        return Err("ar: bad magic");
    }

    let mut entries = Vec::new();
    let mut pos = 8usize;

    while pos + 60 <= data.len() {
        let header = &data[pos..pos + 60];

        // Verify end signature
        if &header[58..60] != AR_END {
            return Err("ar: bad file header terminator");
        }

        // Name: right-padded with spaces, may end with '/'
        let name_raw = core::str::from_utf8(&header[0..16])
            .unwrap_or("")
            .trim_end();
        let name = name_raw.trim_end_matches('/').to_string();

        // Size: decimal ASCII in bytes 48..58
        let size_str = core::str::from_utf8(&header[48..58]).unwrap_or("0").trim();
        let size = size_str.parse::<usize>().unwrap_or(0);

        pos += 60;

        if pos + size > data.len() {
            return Err("ar: member extends past end of archive");
        }

        entries.push(ArEntry {
            name,
            data: data[pos..pos + size].to_vec(),
        });

        // Advance to next header (even boundary)
        pos += size;
        if !pos.is_multiple_of(2) {
            pos += 1;
        }
    }

    Ok(entries)
}

/// Find a named member in an ar archive.
pub fn find<'a>(entries: &'a [ArEntry], name: &str) -> Option<&'a ArEntry> {
    entries
        .iter()
        .find(|e| e.name == name || e.name.starts_with(name))
}
