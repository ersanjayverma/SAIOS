use alloc::format;
use alloc::string::String;

use crate::vfs;

pub fn read_rendered(path: &str) -> Result<String, &'static str> {
    let data = vfs::read_path(path)?;
    let text = String::from_utf8_lossy(data.as_slice());
    Ok(sanitize_for_terminal(text.as_ref()))
}

pub fn map_read_error(err: &'static str) -> &'static str {
    match err {
        "path not found" => "cat: path not found",
        "not a file" => "cat: not a file",
        _ => "cat failed",
    }
}

fn sanitize_for_terminal(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '\n' | '\t' => out.push(ch),
            '\r' => out.push('\n'),
            c if c.is_control() => {
                if (c as u32) <= 0xFF {
                    out.push_str(format!("\\x{:02x}", c as u32).as_str());
                } else {
                    out.push_str(format!("\\u{{{:x}}}", c as u32).as_str());
                }
            }
            _ => out.push(ch),
        }
    }
    out
}
