//! Anthropic Claude provider.
//! Requires an API key: `ai key anthropic sk-ant-...`

use crate::net::dns;
use crate::net::http::{HttpRequest, send_https};
use alloc::format;
use alloc::string::String;

pub fn complete(api_key: &str, prompt: &str) -> Option<String> {
    let body = format!(
        r#"{{"model":"claude-sonnet-4-6","max_tokens":1024,"messages":[{{"role":"user","content":"{}"}}]}}"#,
        crate::ai::ollama::json_escape_pub(prompt)
    );

    // Resolve api.anthropic.com
    let _ip = dns::resolve_blocking("api.anthropic.com")?;

    let mut req = HttpRequest::post_json("api.anthropic.com", "/v1/messages", 443, &body);
    req.headers.push(("x-api-key", api_key));
    req.headers.push(("anthropic-version", "2023-06-01"));

    let resp = send_https(req)?;
    if resp.status != 200 {
        return Some(format!("[Anthropic error {}] {}", resp.status, resp.body));
    }
    super::ollama::extract_json_string_pub(&resp.body, "text")
}
