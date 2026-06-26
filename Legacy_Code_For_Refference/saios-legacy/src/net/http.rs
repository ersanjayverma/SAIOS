//! Minimal HTTP/1.1 client for AI API calls.
//! Uses the TCP layer directly (no TLS yet — use HTTP or a local proxy for now).

use super::dns;
use super::tcp;
use super::tls;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub complete: bool,
    /// Body as lossy UTF-8 — for text responses (HTML, JSON).
    pub body: String,
    /// Raw body bytes — for binary responses (gzip Packages, .deb downloads).
    pub body_bytes: Vec<u8>,
}

pub struct HttpRequest<'a> {
    pub method: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub port: u16,
    pub headers: Vec<(&'a str, &'a str)>,
    pub body: Option<&'a str>,
}

impl<'a> HttpRequest<'a> {
    pub fn post_json(host: &'a str, path: &'a str, port: u16, json: &'a str) -> Self {
        Self {
            method: "POST",
            host,
            path,
            port,
            headers: alloc::vec![
                ("Content-Type", "application/json"),
                ("Accept", "application/json"),
            ],
            body: Some(json),
        }
    }

    pub fn get(host: &'a str, path: &'a str, port: u16) -> Self {
        Self {
            method: "GET",
            host,
            path,
            port,
            headers: alloc::vec![],
            body: None,
        }
    }
}

/// Blocking HTTP request. Returns None on network failure.
pub fn send(req: HttpRequest) -> Option<HttpResponse> {
    let ip = dns::resolve_blocking(req.host)?;
    let src_port = tcp::open(ip, req.port);

    // Build raw HTTP/1.1 request
    let mut raw = String::new();
    raw.push_str(&format!("{} {} HTTP/1.1\r\n", req.method, req.path));
    raw.push_str(&format!("Host: {}\r\n", req.host));
    raw.push_str("Connection: close\r\n");
    // Default User-Agent/Accept unless the caller set them — many servers
    // (CDNs, package mirrors, AI APIs) reject requests that lack a User-Agent.
    if !req
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
    {
        raw.push_str("User-Agent: SAIOS/0.3\r\n");
    }
    if !req
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("accept"))
    {
        raw.push_str("Accept: */*\r\n");
    }
    for (k, v) in &req.headers {
        raw.push_str(&format!("{}: {}\r\n", k, v));
    }
    if let Some(body) = req.body {
        raw.push_str(&format!("Content-Length: {}\r\n", body.len()));
        raw.push_str("\r\n");
        raw.push_str(body);
    } else {
        raw.push_str("\r\n");
    }

    // -- Establish the connection, retransmitting the SYN --------------------
    // The first SYN is usually dropped while ARP resolves the next hop, and we
    // never retransmitted it before — so connections silently never opened.
    let mut established = false;
    let ct0 = crate::shell::commands::boot_ticks();
    let mut last_syn: u64 = 0;
    while crate::shell::commands::boot_ticks().wrapping_sub(ct0) < 800 {
        // ~8 s @100Hz
        super::pump();
        super::tcp::poll();
        if tcp::is_established(src_port, ip, req.port) {
            established = true;
            break;
        }
        let now = crate::shell::commands::boot_ticks().wrapping_sub(ct0);
        if now.wrapping_sub(last_syn) >= 50 {
            // retransmit SYN ~every 0.5 s
            tcp::resend_syn(src_port, ip, req.port);
            last_syn = now.max(1);
        }
        x86_64::instructions::nop();
    }
    if !established {
        crate::serial_println!(
            "[http] connect FAILED {}.{}.{}.{}:{} (unreachable/no SYN-ACK)",
            ip[0],
            ip[1],
            ip[2],
            ip[3],
            req.port
        );
        tcp::close_and_remove(src_port, ip, req.port);
        return None;
    }
    crate::serial_println!(
        "[http] connect ok {}.{}.{}.{}:{}",
        ip[0],
        ip[1],
        ip[2],
        ip[3],
        req.port
    );

    tcp::write(src_port, ip, req.port, raw.as_bytes());
    let mut last_req_send = crate::shell::commands::boot_ticks();
    // Only idempotent requests may be retransmitted: resending a POST/PUT body
    // before the response arrives could duplicate a non-idempotent operation
    // (e.g. an Ollama/OpenAI generation) if the first request actually reached
    // the server and only the reply was slow.  GET/HEAD/OPTIONS are safe to
    // repeat per RFC 7231 §4.2.2.
    let idempotent = matches!(req.method, "GET" | "HEAD" | "OPTIONS");

    // -- Read the FULL response ----------------------------------------------
    // Stop when: (a) we have all headers + Content-Length bytes of body, or
    // (b) the peer closes the connection (we send `Connection: close`, so the
    // server FINs after the body).  The old code stopped at the first \r\n\r\n,
    // i.e. the END OF HEADERS — so the body was always empty/truncated.
    let mut response_raw: Vec<u8> = Vec::new();
    let mut header_end: Option<usize> = None;
    let mut content_len: Option<usize> = None;
    let rt0 = crate::shell::commands::boot_ticks();
    let mut last_data = rt0;
    let mut chunked = false;
    // Throttle the progress bar: rendering to VGA scrolls the screen and is very
    // slow (port I/O), and the receive loop spins thousands of times per second.
    // Rendering every iteration starved the NIC poll so the e1000 ring / slirp
    // NAT overflowed and the peer reset the connection partway through a large
    // download.  Render only when we cross a new 512 KiB boundary.
    let mut last_render: u64 = 0;
    let mut complete = false;
    loop {
        let now = crate::shell::commands::boot_ticks();
        super::pump();
        super::tcp::poll();
        super::pump(); // flush the ACKs tcp::poll just queued, same iteration
        let chunk = tcp::read(src_port, ip, req.port);
        if !chunk.is_empty() {
            last_data = now;
            response_raw.extend_from_slice(&chunk);
            if header_end.is_none()
                && let Some(pos) = find_header_end(&response_raw)
            {
                header_end = Some(pos);
                content_len = parse_content_length(&response_raw[..pos]);
                chunked = header_has(&response_raw[..pos], "transfer-encoding", "chunked");
                crate::serial_println!(
                    "[http] status_hdrs cl={:?} chunked={}",
                    content_len,
                    chunked
                );
                if let Some(cl) = content_len {
                    response_raw.reserve(pos + cl + 4096);
                }
            }
        }
        // Chunked responses (no Content-Length) end with the "0\r\n\r\n" terminator.
        if chunked && let Some(he) = header_end {
            let body = &response_raw[he..];
            if body.len() >= 5 && body.ends_with(b"0\r\n\r\n") {
                crate::serial_println!("[http] done chunked got={}", body.len());
                complete = true;
                break;
            }
        }
        // Live progress bar for the download — throttled to 512 KiB steps so the
        // slow VGA render doesn't starve the receive path (see last_render note).
        if let (Some(he), Some(cl)) = (header_end, content_len) {
            let done = response_raw.len().saturating_sub(he) as u64;
            if done.wrapping_sub(last_render) >= 524_288 || done >= cl as u64 {
                last_render = done;
                crate::shell::progress_set("download", done, cl as u64);
                crate::shell::progress_render();
            }
        }
        // Complete: headers + declared body length received.
        if let (Some(he), Some(cl)) = (header_end, content_len)
            && response_raw.len() >= he + cl
        {
            crate::serial_println!(
                "[http] done complete cl={} got={}",
                cl,
                response_raw.len() - he
            );
            complete = true;
            break;
        }
        // Complete: server closed the connection after the body.
        if header_end.is_some() && tcp::is_closed(src_port, ip, req.port) {
            // Drain any final buffered bytes before giving up.
            let tail = tcp::read(src_port, ip, req.port);
            if !tail.is_empty() {
                response_raw.extend_from_slice(&tail);
            }
            let body_got = header_end
                .map(|he| response_raw.len().saturating_sub(he))
                .unwrap_or(0);
            let elapsed = now.wrapping_sub(rt0).max(1);
            crate::serial_println!(
                "[http] xfer: {} bytes in {} ticks (~{} KB/tick)",
                body_got,
                elapsed,
                (body_got as u64 / 1024) / elapsed
            );
            // Strict: a close-delimited response with NO Content-Length and NO
            // chunked framing has no verified length, so we cannot prove it
            // arrived intact — never mark it complete (a truncated binary
            // download would otherwise be trusted).  `chunked` completion is
            // handled above; here only a satisfied Content-Length counts.
            complete = content_len.map(|cl| body_got >= cl).unwrap_or(false);
            crate::serial_println!(
                "[http] done closed complete={} cl={:?} got={}",
                complete,
                content_len,
                body_got
            );
            break;
        }
        // Retransmit only safe (idempotent) requests whose initial segment was
        // likely lost (no response headers yet).  POST/PUT are never resent.
        if idempotent
            && header_end.is_none()
            && now.wrapping_sub(last_data) > 220
            && now.wrapping_sub(last_req_send) > 110
        {
            tcp::resend_last(src_port, ip, req.port);
            last_req_send = now;
            crate::serial_println!("[http] retransmit request");
        }
        // Stalled — no data for ~30 s — bail with what we have.  A healthy
        // transfer (dup~0) can still pause for several seconds on the public
        // mirror, so give slow/paused servers room before giving up.
        if now.wrapping_sub(last_data) > 3000 {
            // ~30 s @100Hz
            crate::serial_println!(
                "[http] STALL cl={:?} got={}",
                content_len,
                response_raw.len()
            );
            break;
        }
        // Hard cap ~340 s so a slow-but-progressing multi-MB download completes.
        if now.wrapping_sub(rt0) > 34000 {
            // ~340 s @100Hz
            crate::serial_println!(
                "[http] HARDCAP cl={:?} got={}",
                content_len,
                response_raw.len()
            );
            break;
        }
        x86_64::instructions::nop();
    }
    let (io, ooo, dup) = tcp::rx_stats();
    crate::serial_println!(
        "[http] tcp rx: inorder={} ooo={} dup={} tx_acks={}",
        io,
        ooo,
        dup,
        tcp::tx_acks()
    );
    crate::shell::progress_clear();
    if !crate::shell::IN_BG.load(core::sync::atomic::Ordering::Relaxed) {
        crate::println!(); // finish the progress-bar line
    }

    tcp::close_and_remove(src_port, ip, req.port);
    parse_response(&response_raw, complete)
}

/// Like [`send`], but over TLS (HTTPS).  Establishes a TLS 1.3 session to
/// `req.host:req.port`, writes the HTTP/1.1 request, reads the decrypted
/// response and parses it.  Used by the cloud AI providers (Together / OpenAI /
/// Anthropic), whose endpoints are HTTPS-only.
pub fn send_https(req: HttpRequest) -> Option<HttpResponse> {
    let mut stream = tls::TlsStream::connect(req.host, req.port).ok()?;

    let mut raw = String::new();
    raw.push_str(&format!("{} {} HTTP/1.1\r\n", req.method, req.path));
    raw.push_str(&format!("Host: {}\r\n", req.host));
    raw.push_str("Connection: close\r\n");
    // Default User-Agent/Accept unless the caller set them — many servers
    // (CDNs, package mirrors, AI APIs) reject requests that lack a User-Agent.
    if !req
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
    {
        raw.push_str("User-Agent: SAIOS/0.3\r\n");
    }
    if !req
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("accept"))
    {
        raw.push_str("Accept: */*\r\n");
    }
    for (k, v) in &req.headers {
        raw.push_str(&format!("{}: {}\r\n", k, v));
    }
    if let Some(body) = req.body {
        raw.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        raw.push_str(body);
    } else {
        raw.push_str("\r\n");
    }

    stream.write(raw.as_bytes()).ok()?;
    let response = stream.read_all().ok()?;
    let parsed = parse_response(&response, true);
    crate::serial_println!(
        "[https] {} -> {} decrypted body bytes, status={:?}",
        req.host,
        response.len(),
        parsed.as_ref().map(|r| r.status)
    );
    parsed
}

/// Byte offset just past the `\r\n\r\n` header/body separator, if present.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// Parse a `Content-Length:` value from the header bytes (case-insensitive).
fn parse_content_length(headers: &[u8]) -> Option<usize> {
    let text = core::str::from_utf8(headers).ok()?;
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            return rest.trim().parse::<usize>().ok();
        }
    }
    None
}

/// Case-insensitive check that a header `name` contains `value`.
fn header_has(headers: &[u8], name: &str, value: &str) -> bool {
    let text = match core::str::from_utf8(headers) {
        Ok(t) => t,
        Err(_) => return false,
    };
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix(&alloc::format!("{}:", name)) {
            return rest.contains(value);
        }
    }
    false
}

/// Decode an HTTP chunked-transfer body into the raw payload.
fn dechunk(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        // chunk-size line: hex digits, optional ;ext, terminated by CRLF
        let mut j = i;
        while j + 1 < body.len() && !(body[j] == b'\r' && body[j + 1] == b'\n') {
            j += 1;
        }
        let line = core::str::from_utf8(&body[i..j]).unwrap_or("");
        let hex = line.trim().split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(hex, 16).unwrap_or(0);
        i = j + 2; // skip CRLF after the size line
        if size == 0 {
            break;
        } // last chunk
        if i + size > body.len() {
            out.extend_from_slice(&body[i..]);
            break;
        }
        out.extend_from_slice(&body[i..i + size]);
        i += size + 2; // skip data + trailing CRLF
    }
    out
}

fn parse_response(raw: &[u8], complete: bool) -> Option<HttpResponse> {
    // Split headers from body at the BYTE level — the body may be binary
    // (gzip / .deb), so we must not require the whole thing to be UTF-8.
    let sep = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let header_bytes = &raw[..sep];
    let body_raw = &raw[sep + 4..];
    // De-chunk if the response used Transfer-Encoding: chunked (Ollama streams).
    let body_bytes = if header_has(header_bytes, "transfer-encoding", "chunked") {
        dechunk(body_raw)
    } else {
        body_raw.to_vec()
    };

    let header_text = String::from_utf8_lossy(header_bytes);
    let mut header_lines = header_text.lines();
    let status_line = header_lines.next()?;
    let status: u16 = status_line.split(' ').nth(1)?.parse().ok()?;

    let mut headers = Vec::new();
    for line in header_lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.push((k.to_string(), v.to_string()));
        }
    }

    let body = String::from_utf8_lossy(&body_bytes).into_owned();
    Some(HttpResponse {
        status,
        headers,
        complete,
        body,
        body_bytes,
    })
}
