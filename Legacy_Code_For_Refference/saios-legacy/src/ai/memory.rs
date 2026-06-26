//! Persistent memory for ALL AI usage in SAIOS.
//!
//! Every prompt, answer, agent step and tool call is appended to a log file on
//! the root filesystem so context survives across commands (and reboots, on an
//! installed disk).  The agent loads the recent tail as context.

use alloc::format;
use alloc::string::{String, ToString};

const MEM_DIR: &str = "/var/saios";
const MEM_PATH: &str = "/var/saios/ai_memory.log";
/// Keep at most this many bytes (trimmed to a line boundary from the front).
const MAX_BYTES: usize = 64 * 1024;

/// Append one tagged entry, e.g. `log("ask", prompt)`.
pub fn log(kind: &str, text: &str) {
    let one_line = text.replace('\n', " ");
    let mut buf = load();
    buf.push_str(&format!("[{}] {}\n", kind, one_line));
    if buf.len() > MAX_BYTES {
        let cut = buf.len() - MAX_BYTES;
        let start = buf[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
        buf = buf[start..].to_string();
    }
    crate::mkdir_p_pub(MEM_DIR);
    crate::write_file_pub(MEM_PATH, buf.as_bytes());
}

/// Full memory contents (may be empty).
pub fn load() -> String {
    crate::vfs_contract::VfsContract::read_file(MEM_PATH)
        .map(|data| String::from_utf8_lossy(&data).into_owned())
        .unwrap_or_else(|_| String::new())
}

/// The most recent `max` bytes of memory (line-aligned) for prompt context.
pub fn recent(max: usize) -> String {
    let m = load();
    if m.len() <= max {
        return m;
    }
    let cut = m.len() - max;
    let start = m[cut..].find('\n').map(|i| cut + i + 1).unwrap_or(cut);
    m[start..].to_string()
}

/// Erase all stored AI memory.
pub fn clear() {
    crate::mkdir_p_pub(MEM_DIR);
    crate::write_file_pub(MEM_PATH, b"");
}
