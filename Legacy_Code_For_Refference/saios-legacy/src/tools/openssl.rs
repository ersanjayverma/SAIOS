//! `openssl` — a small OpenSSL-style crypto command built on SAIOS's in-kernel
//! crypto (SHA-256/HMAC, AES-128-GCM, x25519) and TLS stack.
//!
//! Subcommands:
//!   openssl version
//!   openssl rand [-hex|-base64] <n>
//!   openssl sha256 <text...>           (alias: dgst -sha256 <text...>)
//!   openssl base64 [-d] <text...>
//!   openssl s_client -connect <host:port>

use crate::println;
use alloc::string::String;
use alloc::vec::Vec;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn run(args: &str) {
    let mut it = args.split_whitespace();
    match it.next() {
        Some("version") | None => version(),
        Some("rand") => cmd_rand(args),
        Some("sha256") => cmd_sha256(rest_after(args, "sha256")),
        Some("dgst") => cmd_dgst(args),
        Some("base64") => cmd_base64(args),
        Some("s_client") => cmd_s_client(args),
        Some("x509") => cmd_x509(args),
        Some(other) => {
            println!("openssl: unknown command '{}'", other);
            println!("usage: openssl version|rand|sha256|dgst|base64|s_client");
        }
    }
}

fn version() {
    println!("SAIOS-OpenSSL 0.3 (in-kernel crypto)");
    println!("  digests : SHA-256, HMAC-SHA256");
    println!("  ciphers : AES-128-GCM");
    println!("  kex     : X25519");
    println!("  tls     : TLS 1.2/1.3 client (s_client)");
}

// -- rand --------------------------------------------------------------------

/// TSC-seeded xorshift RNG (not a CSPRNG, but non-deterministic per boot).
fn rng_fill(out: &mut [u8]) {
    let mut s = crate::time::rdtsc() ^ 0x9E37_79B9_7F4A_7C15;
    for b in out.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = (s >> 24) as u8;
    }
}

fn cmd_rand(args: &str) {
    let toks: Vec<&str> = args.split_whitespace().collect();
    let hex = toks.contains(&"-hex");
    let b64 = toks.contains(&"-base64");
    let n: usize = toks.iter().rev().find_map(|t| t.parse().ok()).unwrap_or(16);
    let n = n.min(4096);
    let mut buf = alloc::vec![0u8; n];
    rng_fill(&mut buf);
    if hex {
        let mut s = String::new();
        for b in &buf {
            s.push_str(&hex_byte(*b));
        }
        println!("{}", s);
    } else if b64 {
        println!("{}", base64_encode(&buf));
    } else {
        // Raw bytes aren't useful on a text console — default to hex.
        let mut s = String::new();
        for b in &buf {
            s.push_str(&hex_byte(*b));
        }
        println!("{}", s);
    }
}

// -- digests ---------------------------------------------------------------

fn cmd_sha256(text: &str) {
    let digest = crate::net::tls::sha256::hash(text.as_bytes());
    println!("SHA256({}) = {}", text, hex_str(&digest));
}

fn cmd_dgst(args: &str) {
    // openssl dgst -sha256 <text...>
    if args.contains("-sha256") {
        cmd_sha256(rest_after(args, "-sha256"));
        return;
    }
    println!("openssl dgst: only -sha256 is supported");
}

// -- base64 ------------------------------------------------------------------

fn cmd_base64(args: &str) {
    let decode = args.split_whitespace().any(|t| t == "-d");
    if decode {
        let data = rest_after(args, "-d");
        match base64_decode(data.trim()) {
            Some(bytes) => println!("{}", String::from_utf8_lossy(&bytes)),
            None => println!("openssl base64: invalid input"),
        }
    } else {
        let data = rest_after(args, "base64");
        println!("{}", base64_encode(data.as_bytes()));
    }
}

// -- s_client -----------------------------------------------------------------

fn cmd_s_client(args: &str) {
    // openssl s_client -connect host:port
    let target = rest_after(args, "-connect");
    let target = target.trim();
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(443)),
        None => (target, 443u16),
    };
    if host.is_empty() {
        println!("usage: openssl s_client -connect <host:port>");
        return;
    }
    println!("CONNECTED({}:{})", host, port);
    match crate::net::tls::TlsStream::connect(host, port) {
        Ok(mut s) => {
            println!("TLS handshake OK — secure channel established");
            // Probe with a minimal HTTP HEAD to prove the channel works.
            let req = alloc::format!(
                "HEAD / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                host
            );
            if s.write(req.as_bytes()).is_ok()
                && let Ok(resp) = s.read_all()
            {
                let text = String::from_utf8_lossy(&resp);
                for line in text.lines().take(3) {
                    println!("    {}", line);
                }
            }
        }
        Err(e) => println!("TLS handshake failed: {}", e),
    }
}

// -- x509 (parse + verify a certificate / chain) -----------------------------

fn cmd_x509(args: &str) {
    // openssl x509 -in <file> [-verify <host>]
    let file = {
        let a = rest_after(args, "-in");
        a.split_whitespace().next().unwrap_or("")
    };
    if file.is_empty() {
        println!("usage: openssl x509 -in <file.pem|der> [-verify <host>]");
        return;
    }
    let data = match read_file(file) {
        Some(d) => d,
        None => {
            println!("openssl x509: cannot read {}", file);
            return;
        }
    };

    // Accept PEM (one or more certs) or a single DER.
    let ders: Vec<Vec<u8>> = if data.starts_with(b"-----") || data.windows(5).any(|w| w == b"-----")
    {
        crate::net::tls::x509::pem_to_ders(&String::from_utf8_lossy(&data))
    } else {
        alloc::vec![data.clone()]
    };
    if ders.is_empty() {
        println!("openssl x509: no certificate found");
        return;
    }

    // Print the leaf's fields.
    match crate::net::tls::x509::parse(&ders[0]) {
        Ok(c) => {
            println!("Certificate:");
            println!("  Subject CN : {}", c.cn);
            if !c.sans.is_empty() {
                println!("  SAN        : {}", c.sans.join(", "));
            }
            let (y1, mo1, d1, h1, mi1, s1) = crate::time::civil_from_epoch(c.not_before);
            let (y2, mo2, d2, h2, mi2, s2) = crate::time::civil_from_epoch(c.not_after);
            println!(
                "  Not Before : {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                y1, mo1, d1, h1, mi1, s1
            );
            println!(
                "  Not After  : {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                y2, mo2, d2, h2, mi2, s2
            );
            println!(
                "  Sig alg    : {}",
                if c.sig_alg_rsa_sha256 {
                    "RSA-SHA256"
                } else {
                    "other (ECDSA?)"
                }
            );
            let fp = crate::net::tls::sha256::hash(&ders[0]);
            println!("  SHA256 fp  : {}", hex_str(&fp));
        }
        Err(e) => {
            println!("openssl x509: parse error: {}", e);
            return;
        }
    }

    // Optional chain verification.
    let host = {
        let a = rest_after(args, "-verify");
        a.split_whitespace().next().unwrap_or("")
    };
    if args.contains("-verify") {
        let (now, _) = crate::time::realtime();
        let r = crate::net::tls::x509::verify_chain(&ders, host, now);
        println!(
            "Verify ({}): {}",
            if r.ok { "OK" } else { "FAILED" },
            r.reason
        );
    }
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    let resolved = super::resolve_path(path);
    crate::vfs_contract::VfsContract::read_file(&resolved).ok()
}

// -- helpers ---------------------------------------------------------------

/// Everything in `args` after the first occurrence of `tok` (trimmed).
fn rest_after<'a>(args: &'a str, tok: &str) -> &'a str {
    match args.find(tok) {
        Some(i) => args[i + tok.len()..].trim_start(),
        None => "",
    }
}

fn hex_byte(b: u8) -> String {
    let h = b"0123456789abcdef";
    let mut s = String::new();
    s.push(h[(b >> 4) as usize] as char);
    s.push(h[(b & 0xF) as usize] as char);
    s
}
fn hex_str(bytes: &[u8]) -> String {
    let mut s = String::new();
    for b in bytes {
        s.push_str(&hex_byte(*b));
    }
    s
}

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(B64[(b[0] >> 2) as usize] as char);
        out.push(B64[(((b[0] & 0x3) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[(((b[1] & 0xF) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(b[2] & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Public wrapper for the x509 PEM decoder.
pub fn base64_decode_pub(s: &str) -> Option<Vec<u8>> {
    base64_decode(s)
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|&c| c != b'=' && !c.is_ascii_whitespace())
        .collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        let mut bits = 0;
        for &c in chunk {
            acc = (acc << 6) | val(c)? as u32;
            bits += 6;
        }
        let mut shift = bits - 8;
        while shift >= 0 {
            out.push((acc >> shift) as u8);
            shift -= 8;
        }
    }
    Some(out)
}
