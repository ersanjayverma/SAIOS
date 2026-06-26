//! Ollama provider — free, runs locally/on LAN, open-source.
//! API: POST http://<host>:<port>/api/generate  (no auth required)
//! Install on host: https://ollama.com  — then `ollama pull llama3`

use crate::net::http::{HttpRequest, send};
use alloc::format;
use alloc::string::String;

pub fn complete(host: [u8; 4], port: u16, model: &str, prompt: &str) -> Option<String> {
    let host_str = format!("{}.{}.{}.{}", host[0], host[1], host[2], host[3]);

    // Escape the prompt for JSON (basic escaping)
    let escaped = json_escape(prompt);
    let body = format!(
        r#"{{"model":"{}","prompt":"{}","stream":false}}"#,
        model, escaped
    );

    let req = HttpRequest::post_json(&host_str, "/api/generate", port, &body);
    let resp = send(req)?;

    if resp.status != 200 {
        return Some(format!("[Ollama error {}] {}", resp.status, resp.body));
    }

    // Parse "response" field from JSON (no serde — manual extraction)
    extract_json_string(&resp.body, "response")
}

pub fn extract_json_string_pub(json: &str, key: &str) -> Option<String> {
    extract_json_string(json, key)
}

/// Extract the assistant reply from an OpenAI/Together chat-completions response
/// — `choices[0].message.content` — by anchoring on `"message"` then reading its
/// `"content"` string (so we don't accidentally match some other `content`
/// field).  Falls back to the first `"content"` in the document.
pub fn extract_chat_content(json: &str) -> Option<String> {
    if let Some(mi) = json.find("\"message\"")
        && let Some(s) = extract_json_string(&json[mi..], "content")
    {
        return Some(s);
    }
    extract_json_string(json, "content")
}

pub fn json_escape_pub(s: &str) -> String {
    json_escape(s)
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = alloc::format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let inner = &rest[1..];
    let end = find_unescaped_quote(inner)?;
    Some(unescape_json(&inner[..end]))
}

fn find_unescaped_quote(s: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            return Some(i);
        }
    }
    None
}

fn unescape_json(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{08}'),
            Some('f') => out.push('\u{0C}'),
            Some('"') => out.push('"'),
            Some('/') => out.push('/'),
            Some('\\') => out.push('\\'),
            // \uXXXX — decode the 4 hex digits to a code point (BMP).
            Some('u') => {
                let mut code = 0u32;
                let mut n = 0;
                while n < 4 {
                    match chars.peek().and_then(|c| c.to_digit(16)) {
                        Some(h) => {
                            code = code * 16 + h;
                            chars.next();
                            n += 1;
                        }
                        None => break,
                    }
                }
                if let Some(ch) = char::from_u32(code) {
                    out.push(ch);
                }
            }
            Some(other) => out.push(other), // unknown escape: drop the backslash
            None => {}
        }
    }
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Any other control char must be \u-escaped or the JSON is invalid
            // (servers reject it — a cause of empty/failed responses).
            c if (c as u32) < 0x20 => out.push_str(&alloc::format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
