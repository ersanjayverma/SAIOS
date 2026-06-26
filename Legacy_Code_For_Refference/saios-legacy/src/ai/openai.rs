//! OpenAI provider.
//! Requires an API key: `ai key openai sk-...`

use super::ollama::{extract_chat_content, json_escape_pub};
use crate::net::dns;
use crate::net::http::{HttpRequest, send_https};
use alloc::format;
use alloc::string::String;

pub fn complete(api_key: &str, prompt: &str) -> Option<String> {
    let body = format!(
        r#"{{"model":"gpt-4o","messages":[{{"role":"user","content":"{}"}}],"max_tokens":1024}}"#,
        json_escape_pub(prompt)
    );

    let _ip = dns::resolve_blocking("api.openai.com")?;

    let mut req = HttpRequest::post_json("api.openai.com", "/v1/chat/completions", 443, &body);
    req.headers.push(("Authorization", api_key)); // caller passes "Bearer sk-..."

    let resp = send_https(req)?;
    if resp.status != 200 {
        return Some(format!("[OpenAI error {}] {}", resp.status, resp.body));
    }
    extract_chat_content(&resp.body)
}
