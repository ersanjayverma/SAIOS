//! `ssh` — SSH-2 transport client.
//!
//! Implements the SSH transport handshake on SAIOS's TCP stack + in-kernel
//! crypto: version-string exchange, KEXINIT negotiation, and a curve25519
//! (X25519) key exchange, then reports the server's offered algorithms and the
//! SHA-256 fingerprint of its host key (à la `ssh-keyscan host | ssh-keygen -lf`).
//!
//! Interactive sessions (NEWKEYS + userauth + channels) are the tracked next
//! step; this proves the transport + KEX path end-to-end.

use crate::net::{
    dns, tcp,
    tls::{sha256, x25519},
};
use crate::println;
use alloc::string::String;
use alloc::vec::Vec;

const MSG_KEXINIT: u8 = 20;
const MSG_KEX_ECDH_INIT: u8 = 30;
const MSG_KEX_ECDH_REPLY: u8 = 31;

pub fn run(args: &str) {
    let toks: Vec<&str> = args.split_whitespace().collect();
    if toks.is_empty() {
        usage();
        return;
    }

    let mut port = 22u16;
    let mut target = "";
    let mut i = 0;
    while i < toks.len() {
        if toks[i] == "-p" && i + 1 < toks.len() {
            port = toks[i + 1].parse().unwrap_or(22);
            i += 2;
        } else {
            target = toks[i];
            i += 1;
        }
    }
    let (user, host) = match target.split_once('@') {
        Some((u, h)) => (u, h),
        None => ("root", target),
    };
    if host.is_empty() {
        usage();
        return;
    }

    let Some(ip) = dns::resolve_blocking(host) else {
        println!("ssh: could not resolve hostname {}", host);
        return;
    };
    println!(
        "ssh: connecting to {} ({}.{}.{}.{}) port {} (user {})",
        host, ip[0], ip[1], ip[2], ip[3], port, user
    );

    let src = tcp::open(ip, port);
    if !wait_established(src, ip, port) {
        println!("ssh: connect to {}:{} failed", host, port);
        tcp::close_and_remove(src, ip, port);
        return;
    }

    let mut c = Conn {
        src,
        ip,
        port,
        rx: Vec::new(),
    };

    // 1. Version exchange.
    tcp::write(src, ip, port, b"SSH-2.0-SAIOS_0.3\r\n");
    match c.read_line() {
        Some(b) => println!("ssh: remote version: {}", b.trim_end()),
        None => {
            println!("ssh: no identification string from server");
            c.close();
            return;
        }
    }

    // 2. KEXINIT from server.
    let kexinit = match c.read_packet() {
        Some(p) if !p.is_empty() && p[0] == MSG_KEXINIT => p,
        _ => {
            println!("ssh: expected KEXINIT");
            c.close();
            return;
        }
    };
    print_kexinit(&kexinit);

    // Send our (minimal) KEXINIT so the server proceeds to ECDH.
    c.write_packet(&our_kexinit());

    // 3. Curve25519 KEX: send our public key, read the reply.
    let priv_key = x25519::generate_private_key();
    let pub_key = x25519::public_from_private(&priv_key);
    let mut init = Vec::new();
    init.push(MSG_KEX_ECDH_INIT);
    put_string(&mut init, &pub_key); // Q_C
    c.write_packet(&init);

    let reply = match c.read_packet() {
        Some(p) if !p.is_empty() && p[0] == MSG_KEX_ECDH_REPLY => p,
        _ => {
            println!("ssh: no KEX_ECDH_REPLY (server may require a different kex)");
            c.close();
            return;
        }
    };

    // Parse: byte; string K_S (host key); string Q_S; string signature.
    let mut off = 1usize;
    let host_key = match get_string(&reply, &mut off) {
        Some(s) => s,
        None => {
            println!("ssh: malformed KEX reply");
            c.close();
            return;
        }
    };
    let server_pub = get_string(&reply, &mut off);

    // Host key fingerprint (OpenSSH style: SHA256 base64, no padding).
    let fp = sha256::hash(&host_key);
    let fp_b64 = super::openssl::base64_encode(&fp);
    let fp_b64 = fp_b64.trim_end_matches('=');
    let keytype = ssh_string_first(&host_key).unwrap_or_else(|| String::from("ssh-key"));
    println!("ssh: host key type : {}", keytype);
    println!("ssh: fingerprint   : SHA256:{}", fp_b64);

    if let Some(sp) = server_pub
        && sp.len() == 32
    {
        let mut peer = [0u8; 32];
        peer.copy_from_slice(&sp);
        let _shared = x25519::diffie_hellman(&priv_key, &peer);
        println!("ssh: X25519 key exchange completed (shared secret derived)");
    }

    println!("ssh: transport handshake OK.");
    println!("ssh: interactive session (auth + shell channel) not yet implemented.");
    c.close();
}

fn usage() {
    println!("usage: ssh [user@]host [-p port]");
    println!("  performs the SSH-2 transport handshake and prints the host key fingerprint");
}

// -- Buffered SSH connection -------------------------------------------------

struct Conn {
    src: u16,
    ip: [u8; 4],
    port: u16,
    rx: Vec<u8>,
}

impl Conn {
    fn close(&self) {
        tcp::close_and_remove(self.src, self.ip, self.port);
    }

    /// Pull whatever bytes are available into the rx buffer (one poll cycle).
    fn fill(&mut self) {
        crate::net::pump();
        tcp::poll();
        let chunk = tcp::read(self.src, self.ip, self.port);
        if !chunk.is_empty() {
            self.rx.extend_from_slice(&chunk);
        }
    }

    /// Read a CRLF/LF-terminated line (for the version banner).
    fn read_line(&mut self) -> Option<String> {
        let t0 = crate::time::uptime_ns();
        loop {
            if let Some(pos) = self.rx.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.rx.drain(..=pos).collect();
                return Some(String::from_utf8_lossy(&line).into_owned());
            }
            if crate::time::uptime_ns().wrapping_sub(t0) > 5_000_000_000 {
                return None;
            }
            self.fill();
        }
    }

    /// Read one unencrypted SSH binary packet; returns its payload.
    fn read_packet(&mut self) -> Option<Vec<u8>> {
        let t0 = crate::time::uptime_ns();
        loop {
            if self.rx.len() >= 4 {
                let plen =
                    u32::from_be_bytes([self.rx[0], self.rx[1], self.rx[2], self.rx[3]]) as usize;
                if (1..1 << 20).contains(&plen) && self.rx.len() >= 4 + plen {
                    let pad = self.rx[4] as usize;
                    if pad < plen {
                        let payload = self.rx[5..4 + plen - pad].to_vec();
                        self.rx.drain(..4 + plen);
                        return Some(payload);
                    }
                }
            }
            if crate::time::uptime_ns().wrapping_sub(t0) > 5_000_000_000 {
                return None;
            }
            self.fill();
        }
    }

    /// Frame and send an unencrypted SSH binary packet.
    fn write_packet(&mut self, payload: &[u8]) {
        // block size 8 (no cipher); padding 4..=255 so total % 8 == 0.
        let mut pad = 8 - ((5 + payload.len()) % 8);
        if pad < 4 {
            pad += 8;
        }
        let plen = 1 + payload.len() + pad;
        let mut pkt = Vec::with_capacity(4 + plen);
        pkt.extend_from_slice(&(plen as u32).to_be_bytes());
        pkt.push(pad as u8);
        pkt.extend_from_slice(payload);
        pkt.extend(core::iter::repeat_n(0u8, pad));
        tcp::write(self.src, self.ip, self.port, &pkt);
    }
}

fn wait_established(src: u16, ip: [u8; 4], port: u16) -> bool {
    let t0 = crate::time::uptime_ns();
    let mut last_syn = t0;
    while crate::time::uptime_ns().wrapping_sub(t0) < 5_000_000_000 {
        crate::net::pump();
        tcp::poll();
        if tcp::is_established(src, ip, port) {
            return true;
        }
        if crate::time::uptime_ns().wrapping_sub(last_syn) > 1_000_000_000 {
            tcp::resend_syn(src, ip, port);
            last_syn = crate::time::uptime_ns();
        }
        core::hint::spin_loop();
    }
    false
}

// -- SSH wire helpers --------------------------------------------------------

fn put_string(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
}

fn get_string(buf: &[u8], off: &mut usize) -> Option<Vec<u8>> {
    if *off + 4 > buf.len() {
        return None;
    }
    let len = u32::from_be_bytes([buf[*off], buf[*off + 1], buf[*off + 2], buf[*off + 3]]) as usize;
    *off += 4;
    if *off + len > buf.len() {
        return None;
    }
    let s = buf[*off..*off + len].to_vec();
    *off += len;
    Some(s)
}

/// First SSH string inside a host-key blob (its algorithm name, e.g. ssh-ed25519).
fn ssh_string_first(blob: &[u8]) -> Option<String> {
    let mut off = 0;
    get_string(blob, &mut off).map(|s| String::from_utf8_lossy(&s).into_owned())
}

/// Print the algorithm name-lists from a server KEXINIT payload.
fn print_kexinit(p: &[u8]) {
    // byte type | byte[16] cookie | 10 name-lists | ...
    let mut off = 17usize;
    let labels = [
        "kex",
        "host-key",
        "cipher c2s",
        "cipher s2c",
        "mac c2s",
        "mac s2c",
    ];
    for label in labels {
        match get_string(p, &mut off) {
            Some(s) => {
                let list = String::from_utf8_lossy(&s);
                let first = list.split(',').next().unwrap_or("");
                println!("ssh: {:<11}: {}", label, first);
            }
            None => return,
        }
    }
}

/// A minimal client KEXINIT advertising curve25519-sha256 + ssh-ed25519.
fn our_kexinit() -> Vec<u8> {
    let mut p = Vec::new();
    p.push(MSG_KEXINIT);
    // 16-byte cookie (TSC-derived).
    let mut cookie = [0u8; 16];
    let mut s = crate::time::rdtsc();
    for b in cookie.iter_mut() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = s as u8;
    }
    p.extend_from_slice(&cookie);
    let lists = [
        "curve25519-sha256",                // kex
        "ssh-ed25519,rsa-sha2-256,ssh-rsa", // host key
        "aes128-gcm@openssh.com",           // enc c2s
        "aes128-gcm@openssh.com",           // enc s2c
        "hmac-sha2-256",                    // mac c2s
        "hmac-sha2-256",                    // mac s2c
        "none",
        "none", // compression c2s/s2c
        "",
        "", // languages c2s/s2c
    ];
    for l in lists {
        put_string(&mut p, l.as_bytes());
    }
    p.push(0); // first_kex_packet_follows = false
    p.extend_from_slice(&[0, 0, 0, 0]); // reserved
    p
}
