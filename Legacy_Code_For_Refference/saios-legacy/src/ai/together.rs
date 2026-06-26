//! Together AI provider — OpenAI-compatible chat-completions API.
//!
//!   POST https://api.together.xyz/v1/chat/completions
//!   Authorization: Bearer <key>
//!   {"model":"<model>","messages":[{"role":"user","content":"<prompt>"}]}
//!
//! Default model is `openai/gpt-oss-120b` (change with `ai model <name>` while
//! Together is the active provider).  Set the key with `ai key together <key>`.
//!
//! NOTE: the endpoint is HTTPS only.  Like the OpenAI/Anthropic providers, this
//! issues the request on port 80 for now; it will work once the TLS client is
//! wired end-to-end (the transcript-hash handshake is in place; record-layer
//! app-data exchange is the remaining piece).

use super::ollama::{extract_chat_content, json_escape_pub};
use crate::net::dns;
use crate::net::http::{HttpRequest, send_https};
use alloc::format;
use alloc::string::String;

/// Together model catalog: friendly alias → real Together API model id.
/// The first four come from the reference LangGraph service (authoritative);
/// the rest are well-established Together serverless models.  This is only a
/// convenience table — `resolve_model` passes any unknown name through, so a
/// full API id (or a model not listed here) always works directly.  Verify the
/// live set against the authenticated `GET https://api.together.xyz/v1/models`
/// endpoint, which needs the API key.
pub const MODELS: &[(&str, &str)] = &[
    ("gpt-oss-120b", "openai/gpt-oss-120b"),
    (
        "together-llama-3.1-405b",
        "meta-llama/Meta-Llama-3.1-405B-Instruct-Turbo",
    ),
    (
        "together-mixtral-8x7b",
        "mistralai/Mixtral-8x7B-Instruct-v0.1",
    ),
    ("together-qwen", "Qwen/Qwen3-Coder-480B-A35B-Instruct-FP8"),
    ("gpt-oss-20b", "openai/gpt-oss-20b"),
    ("llama-3.3-70b", "meta-llama/Llama-3.3-70B-Instruct-Turbo"),
    ("qwen3-235b", "Qwen/Qwen3-235B-A22B-Instruct-2507-tput"),
];

/// Resolve a friendly alias to its Together API id.  Unknown names pass through
/// unchanged, so a full API id (e.g. "openai/gpt-oss-120b") also works directly.
pub fn resolve_model(name: &str) -> &str {
    for (alias, api) in MODELS {
        if *alias == name {
            return api;
        }
    }
    name
}

pub fn complete(api_key: &str, model: &str, prompt: &str) -> Option<String> {
    let api_model = resolve_model(model);
    let body = format!(
        r#"{{"model":"{}","messages":[{{"role":"user","content":"{}"}}]}}"#,
        api_model,
        json_escape_pub(prompt)
    );

    let _ip = dns::resolve_blocking("api.together.xyz")?;

    // Build the Bearer header; `auth` outlives the request borrow below.
    let auth = format!("Bearer {}", api_key);
    let mut req = HttpRequest::post_json("api.together.xyz", "/v1/chat/completions", 443, &body);
    req.headers.push(("Authorization", auth.as_str()));

    let resp = send_https(req)?;
    if resp.status != 200 {
        return Some(format!("[Together error {}] {}", resp.status, resp.body));
    }
    // OpenAI-style response: choices[0].message.content.
    extract_chat_content(&resp.body)
}
