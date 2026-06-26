//! Minimal TLS 1.3 client for SAIOS.
//!
//! Supports the single mandatory TLS 1.3 cipher suite:
//!   TLS_AES_128_GCM_SHA256
//!
//! Key exchange: X25519 (ephemeral ECDH)
//! Authentication: certificate verification is still incomplete, so this module
//! should not be described as providing strong HTTPS trust guarantees yet.
//!
//! Provides `tls_connect(host, port)` which returns a `TlsStream`
//! that wraps the TCP socket and provides read/write.
//!
//! # Usage
//! ```
//! let mut tls = tls_connect("deb.debian.org", 443)?;
//! tls.write(b"GET / HTTP/1.1\r\nHost: deb.debian.org\r\n\r\n")?;
//! let resp = tls.read_all()?;
//! ```

pub mod aes_gcm;
pub mod flight;
pub mod hkdf;
pub mod sha256;
pub mod x25519;
pub mod x509;

use alloc::string::String;
use alloc::vec::Vec;

/// A TLS 1.3 connection over a TCP socket.
pub struct TlsStream {
    /// Source port of the underlying TCP connection.
    tcp_src_port: u16,
    /// Remote IP and port.
    remote_ip: [u8; 4],
    remote_port: u16,
    /// Session traffic keys (derived after handshake).
    client_key: [u8; 16],
    client_iv: [u8; 12],
    server_key: [u8; 16],
    server_iv: [u8; 12],
    /// Record sequence numbers.
    send_seq: u64,
    recv_seq: u64,
    /// Receive buffer for partial records.
    recv_buf: Vec<u8>,
}

impl TlsStream {
    /// Perform a full TLS 1.3 handshake and return a connected stream.
    pub fn connect(host: &str, port: u16) -> Result<Self, &'static str> {
        // Run primitive self-tests once so the serial log says which (if any)
        // crypto primitive is wrong (the GCM AUTH FAIL points at X25519/AES-GCM).
        selftest_once();

        // â”€â”€ DNS / TCP â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let ip = crate::net::dns::resolve_blocking(host).ok_or("tls: DNS resolution failed")?;

        let src_port = crate::net::tcp::open(ip, port);

        // Wait for TCP connection to establish (SYN/ACK)
        for _ in 0..2_000_000u32 {
            crate::net::pump();
            crate::net::tcp::poll();
            x86_64::instructions::nop();
        }

        // â”€â”€ Generate ephemeral X25519 key pair â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let private_key = x25519::generate_private_key();
        let public_key = x25519::public_from_private(&private_key);

        // â”€â”€ ClientHello â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let client_random = generate_random_32();
        let client_hello = build_client_hello(&public_key, &client_random, host);
        send_tls_record(src_port, ip, port, 22, &client_hello); // handshake record

        // â”€â”€ Wait for ServerHello + encrypted extensions + cert + certverify + finished
        let server_data = recv_handshake_messages(src_port, ip, port)?;

        // â”€â”€ Extract server public key from KeyShare â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let server_pub = extract_server_key_share(&server_data)
            .ok_or("tls: no X25519 key share in ServerHello")?;

        // â”€â”€ ECDH shared secret â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let shared_secret = x25519::diffie_hellman(&private_key, &server_pub);

        // -- TLS 1.3 key schedule (RFC 8446 §7.1) ----------------------------
        let server_hello =
            flight::server_hello_message(&server_data).ok_or("tls: no ServerHello in flight")?;

        let empty_hash = sha256::hash(&[]);
        let early = hkdf::extract(&[0u8; 32], &[0u8; 32]);
        let derived = hkdf::expand_label(&early, b"derived", &empty_hash, 32);
        let hs_secret = hkdf::extract(&derived, &shared_secret);

        // Transcript so far = ClientHello || ServerHello (handshake messages).
        let mut transcript: Vec<u8> = Vec::new();
        transcript.extend_from_slice(&client_hello);
        transcript.extend_from_slice(&server_hello);
        let th_sh = sha256::hash(&transcript);

        // Handshake traffic secrets + keys (used to decrypt the server flight and
        // to encrypt our Finished).
        let c_hs = hkdf::expand_label(&hs_secret, b"c hs traffic", &th_sh, 32);
        let s_hs = hkdf::expand_label(&hs_secret, b"s hs traffic", &th_sh, 32);
        let ck_hs = hkdf::expand_label_16(&c_hs, b"key", b"");
        let civ_hs = hkdf::expand_label_12(&c_hs, b"iv", b"");
        let sk_hs = hkdf::expand_label_16(&s_hs, b"key", b"");
        let siv_hs = hkdf::expand_label_12(&s_hs, b"iv", b"");
        // Full dumps for comparison against a known-good trace (Wireshark with
        // the SSLKEYLOGFILE, or `openssl s_client -debug`).  empty_hash must be
        // the SHA-256 of "" (e3b0c44298fc1c14...); th_sh is Hash(CH||SH).
        crate::serial_println!(
            "[tls] ch={}B sh={}B",
            client_hello.len(),
            server_hello.len()
        );
        crate::serial_println!("[tls] empty_hash={:02x?}", &empty_hash);
        crate::serial_println!("[tls] shared(x25519)={:02x?}", &shared_secret);
        crate::serial_println!("[tls] th_sh(CH||SH)={:02x?}", &th_sh);
        crate::serial_println!("[tls] s_hs_secret={:02x?}", &s_hs);
        crate::serial_println!("[tls] sk_hs={:02x?} siv_hs={:02x?}", &sk_hs, &siv_hs);

        // Decrypt the server's encrypted handshake flight (EncryptedExtensions,
        // Certificate, CertificateVerify, Finished) and fold it into the
        // transcript so the master secret and Finished use the full hash.
        let server_hs = decrypt_server_handshakes(&server_data, &sk_hs, &siv_hs);
        crate::serial_println!(
            "[tls] decrypted {} B of server handshake flight",
            server_hs.len()
        );

        // FIX: a flight we could not decrypt means the key schedule didn't match
        // the server - the handshake FAILED.  Abort instead of proceeding to
        // "handshake complete" with bogus application keys.
        if server_hs.is_empty() {
            return Err(
                "tls: handshake failed - could not decrypt server flight (key/crypto mismatch)",
            );
        }

        // -- Authenticate the server: verify its certificate chain -----------
        // Pull the Certificate message out of the decrypted flight and check the
        // chain against the on-disk trust store (/etc/ssl/certs).  Fail closed
        // when roots are installed; if there is no trust store yet, warn and
        // proceed (bootstrap) since verification is impossible either way.
        verify_server_cert(&server_hs, host)?;

        transcript.extend_from_slice(&server_hs);
        let th_sfin = sha256::hash(&transcript);

        // Master secret → application traffic secrets/keys.
        let derived2 = hkdf::expand_label(&hs_secret, b"derived", &empty_hash, 32);
        let master = hkdf::extract(&derived2, &[0u8; 32]);
        let c_ap = hkdf::expand_label(&master, b"c ap traffic", &th_sfin, 32);
        let s_ap = hkdf::expand_label(&master, b"s ap traffic", &th_sfin, 32);
        let ck = hkdf::expand_label_16(&c_ap, b"key", b"");
        let civ = hkdf::expand_label_12(&c_ap, b"iv", b"");
        let sk = hkdf::expand_label_16(&s_ap, b"key", b"");
        let siv = hkdf::expand_label_12(&s_ap, b"iv", b"");

        // -- Client Finished, encrypted with the client HANDSHAKE key ---------
        // verify_data = HMAC(finished_key, transcript-hash-through-server-Finished)
        let finished_key = hkdf::expand_label(&c_hs, b"finished", b"", 32);
        let verify_data = sha256::hmac(&finished_key, &th_sfin);
        let mut fin_msg = alloc::vec![0x14u8, 0x00, 0x00, 0x20]; // Finished, len 32
        fin_msg.extend_from_slice(&verify_data);
        // First record under the client handshake key → record sequence 0.
        send_encrypted_record(src_port, ip, port, &ck_hs, &civ_hs, 0, 0x16, &fin_msg);

        crate::println!("[tls] handshake complete with {}:{}", host, port);

        Ok(TlsStream {
            tcp_src_port: src_port,
            remote_ip: ip,
            remote_port: port,
            client_key: ck,
            client_iv: civ,
            server_key: sk,
            server_iv: siv,
            send_seq: 0,
            recv_seq: 0,
            recv_buf: Vec::new(),
        })
    }

    /// Send application data, AES-128-GCM-encrypted under the client application
    /// traffic key (inner content type 0x17, record header as AAD).
    pub fn write(&mut self, data: &[u8]) -> Result<(), &'static str> {
        send_encrypted_record(
            self.tcp_src_port,
            self.remote_ip,
            self.remote_port,
            &self.client_key,
            &self.client_iv,
            self.send_seq,
            0x17,
            data,
        );
        self.send_seq += 1;
        Ok(())
    }

    /// Read all available application data (blocking until server closes).
    pub fn read_all(&mut self) -> Result<Vec<u8>, &'static str> {
        // Read the whole encrypted response.  We send `Connection: close`, so the
        // peer FINs after the body - that's the completion signal.  Idle is
        // WALL-CLOCK (100 Hz ticks), not a nop counter: the old nop-bounded wait
        // bailed before the response (sent after the handshake flight + session
        // tickets, with real RTT) arrived, so app data decrypted to 0 bytes
        // intermittently.
        let mut out = Vec::new();
        loop {
            crate::net::pump();
            crate::net::tcp::poll();
            let chunk = crate::net::tcp::read(self.tcp_src_port, self.remote_ip, self.remote_port);
            if !chunk.is_empty() {
                self.recv_buf.extend_from_slice(&chunk);
            }
            // Peer closed (FIN after body - we sent Connection: close) - done.
            if crate::net::tcp::is_closed(self.tcp_src_port, self.remote_ip, self.remote_port) {
                let tail =
                    crate::net::tcp::read(self.tcp_src_port, self.remote_ip, self.remote_port);
                if !tail.is_empty() {
                    self.recv_buf.extend_from_slice(&tail);
                }
                break;
            }
            // No timeout: wait indefinitely, animating + honouring Ctrl+C.
            if crate::net::wait_spin() {
                return Err("tls: cancelled");
            }
        }
        self.decrypt_records(&mut out)?;
        Ok(out)
    }

    fn decrypt_records(&mut self, out: &mut Vec<u8>) -> Result<(), &'static str> {
        let mut pos = 0;
        while pos + 5 <= self.recv_buf.len() {
            let content_type = self.recv_buf[pos];
            let len = u16::from_be_bytes([self.recv_buf[pos + 3], self.recv_buf[pos + 4]]) as usize;
            if pos + 5 + len > self.recv_buf.len() {
                break;
            }
            let frag = &self.recv_buf[pos + 5..pos + 5 + len];
            if content_type == 0x17 {
                // application_data - decrypt under the server app key.  Each
                // decryptable record consumes a sequence number (incl. post-
                // handshake NewSessionTicket records, which we then ignore).
                let nonce = record_nonce(&self.server_iv, self.recv_seq);
                let aad = record_aad(len);
                if let Ok(pt) = aes_gcm::decrypt(&self.server_key, &nonce, frag, &aad) {
                    self.recv_seq += 1;
                    if let Some((inner_type, inner)) = flight::strip_inner_type(&pt)
                        && inner_type == 0x17
                    {
                        out.extend_from_slice(inner);
                    } // app data
                    // inner_type 0x16 (handshake, e.g. NewSessionTicket): ignore
                }
            }
            pos += 5 + len;
        }
        self.recv_buf.drain(..pos);
        Ok(())
    }
}

// â”€â”€ TLS message builders (simplified) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn build_client_hello(pub_key: &[u8; 32], random: &[u8; 32], sni: &str) -> Vec<u8> {
    let mut hs = Vec::new();

    // TLS version: 0x0303 (TLS 1.2 compat)
    hs.extend_from_slice(&[0x03, 0x03]);
    hs.extend_from_slice(random);

    // Session ID (empty)
    hs.push(0);

    // Cipher suites: TLS_AES_128_GCM_SHA256 (0x1301) + TLS_EMPTY_RENEGOTIATION_INFO_SCSV
    hs.extend_from_slice(&[0x00, 0x04, 0x13, 0x01, 0x00, 0xFF]);

    // Compression: null only
    hs.extend_from_slice(&[0x01, 0x00]);

    // Extensions
    let mut exts = Vec::new();

    // SNI extension (0x0000)
    let sni_bytes = sni.as_bytes();
    let sni_list_len = sni_bytes.len() + 3;
    exts.extend_from_slice(&[0x00, 0x00]); // ext type
    let ext_data_len = sni_list_len + 2;
    exts.push((ext_data_len >> 8) as u8);
    exts.push(ext_data_len as u8);
    exts.push((sni_list_len >> 8) as u8);
    exts.push(sni_list_len as u8);
    exts.push(0x00); // host_name type
    exts.push((sni_bytes.len() >> 8) as u8);
    exts.push(sni_bytes.len() as u8);
    exts.extend_from_slice(sni_bytes);

    // Supported versions: TLS 1.3 (0x0304)
    exts.extend_from_slice(&[0x00, 0x2B, 0x00, 0x03, 0x02, 0x03, 0x04]);

    // Supported groups: x25519 (0x001D)
    exts.extend_from_slice(&[0x00, 0x0A, 0x00, 0x04, 0x00, 0x02, 0x00, 0x1D]);

    // Key share: X25519
    exts.extend_from_slice(&[0x00, 0x33]); // key_share ext type
    let ks_len = 38u16; // 2 (group) + 2 (key len) + 32 (key) + 2 (outer len)
    exts.push((ks_len >> 8) as u8);
    exts.push(ks_len as u8);
    let inner = 36u16;
    exts.push((inner >> 8) as u8);
    exts.push(inner as u8);
    exts.extend_from_slice(&[0x00, 0x1D]); // x25519
    exts.extend_from_slice(&[0x00, 0x20]); // 32 bytes
    exts.extend_from_slice(pub_key);

    // Signature algorithms
    exts.extend_from_slice(&[0x00, 0x0D, 0x00, 0x04, 0x00, 0x02, 0x04, 0x03]);

    // Append extension list length to hs
    hs.push((exts.len() >> 8) as u8);
    hs.push(exts.len() as u8);
    hs.extend_from_slice(&exts);

    // Wrap in Handshake header (type=1 = ClientHello)
    let mut wrapped = alloc::vec![0x01u8];
    wrapped.push(0);
    wrapped.push((hs.len() >> 8) as u8);
    wrapped.push(hs.len() as u8);
    wrapped.extend_from_slice(&hs);
    wrapped
}

fn recv_handshake_messages(src_port: u16, ip: [u8; 4], port: u16) -> Result<Vec<u8>, &'static str> {
    // Read the server's whole first flight.  Idle is measured in WALL-CLOCK
    // ticks (100 Hz), not a nop counter - the old nop-count bailed within
    // microseconds of the ServerHello, before the encrypted records (cert chain,
    // several KB) arrived over the real network, so we decrypted 0 bytes.
    let mut buf = Vec::new();
    let mut last_data = crate::shell::commands::boot_ticks();
    loop {
        let now = crate::shell::commands::boot_ticks();
        crate::net::pump();
        crate::net::tcp::poll();
        let chunk = crate::net::tcp::read(src_port, ip, port);
        if !chunk.is_empty() {
            buf.extend_from_slice(&chunk);
            last_data = now;
        }
        // Flight complete: have data and the link has been quiet ~400 ms.
        if !buf.is_empty() && now.wrapping_sub(last_data) > 40 {
            break;
        }
        // No timeout: wait indefinitely, animating + honouring Ctrl+C.
        if crate::net::wait_spin() {
            return Err("tls: cancelled");
        }
    }
    // Frame the records we received and log their types/lengths.
    let (recs, _) = flight::split_records(&buf);
    crate::serial_println!(
        "[tls] recv flight: {} bytes, {} record(s)",
        buf.len(),
        recs.len()
    );
    for r in &recs {
        crate::serial_println!(
            "[tls]   record type={} len={}",
            r.content_type,
            r.fragment.len()
        );
    }
    Ok(buf)
}

fn extract_server_key_share(data: &[u8]) -> Option<[u8; 32]> {
    // Frame the record layer, find the (plaintext) ServerHello handshake message
    // and parse its key_share extension properly - far more robust than scanning
    // the raw bytes for the 0x001D group id (which can match payload data).
    let (records, _) = flight::split_records(data);
    for r in records {
        if r.content_type != flight::CT_HANDSHAKE {
            continue;
        }
        for hs in flight::parse_handshakes(r.fragment) {
            if hs.msg_type == flight::HS_SERVER_HELLO
                && let Some(k) = flight::server_hello_key_share(hs.body)
            {
                return Some(k);
            }
        }
    }
    None
}

/// Decrypt the server's encrypted handshake flight and return ONLY the inner
/// handshake messages (EncryptedExtensions / Certificate / CertificateVerify /
/// Finished), concatenated - for folding into the transcript hash.  The
/// plaintext ServerHello is excluded (the caller already has it).  Each
/// application_data record is AES-GCM-decrypted with the server handshake key,
/// per-record nonce (iv XOR seq) and the record header as AAD.
fn decrypt_server_handshakes(server_data: &[u8], key: &[u8; 16], iv: &[u8; 12]) -> Vec<u8> {
    let (records, _) = flight::split_records(server_data);
    let mut out = Vec::new();
    let mut seq = 0u64;
    for r in records {
        if r.content_type != flight::CT_APPLICATION_DATA {
            continue;
        }
        let nonce = record_nonce(iv, seq);
        let aad = record_aad(r.fragment.len());
        match aes_gcm::decrypt(key, &nonce, r.fragment, &aad) {
            Ok(pt) => {
                let inner = flight::strip_inner_type(&pt);
                crate::serial_println!(
                    "[tls] decrypt rec seq={} ct={} -> {} B pt, inner_type={:?}",
                    seq,
                    r.fragment.len(),
                    pt.len(),
                    inner.as_ref().map(|(t, _)| *t)
                );
                seq += 1;
                if let Some((flight::CT_HANDSHAKE, inner)) = inner {
                    out.extend_from_slice(inner);
                }
            }
            Err(_) => {
                crate::serial_println!(
                    "[tls] decrypt rec seq={} ct={} -> GCM AUTH FAIL (wrong key/nonce/aad)",
                    seq,
                    r.fragment.len()
                );
                seq += 1;
            }
        }
    }
    out
}

/// Extract the server Certificate from the decrypted handshake flight and
/// verify the chain against the on-disk trust store for `host`.
/// - roots present + chain valid  → Ok
/// - roots present + invalid/missing chain → Err (handshake aborts, fail-closed)
/// - no trust store installed → warn and Ok (cannot verify; bootstrap)
fn verify_server_cert(server_hs: &[u8], host: &str) -> Result<(), &'static str> {
    let mut chain: Vec<Vec<u8>> = Vec::new();
    for hs in flight::parse_handshakes(server_hs) {
        if hs.msg_type == flight::HS_CERTIFICATE {
            chain = flight::parse_certificate_chain(hs.body);
            break;
        }
    }

    if !x509::have_roots() {
        crate::serial_println!(
            "[tls] no CA trust store at /etc/ssl/certs - skipping cert verification (INSECURE)"
        );
        return Ok(());
    }
    if chain.is_empty() {
        crate::serial_println!("[tls] server sent no certificate - aborting");
        return Err("tls: server sent no certificate");
    }

    let now = crate::time::realtime().0;
    let report = x509::verify_chain(&chain, host, now);
    if report.ok {
        crate::serial_println!("[tls] certificate verified: CN={}", report.leaf_cn);
        Ok(())
    } else {
        crate::serial_println!("[tls] certificate verification FAILED: {}", report.reason);
        Err("tls: certificate verification failed")
    }
}

/// Parse a 64-hex-char string into 32 bytes (for test vectors).
fn hex32(s: &str) -> [u8; 32] {
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let nib = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
    for i in 0..32 {
        out[i] = (nib(b[i * 2]) << 4) | nib(b[i * 2 + 1]);
    }
    out
}

use core::sync::atomic::{AtomicBool, Ordering as AOrd};
static SELFTEST_DONE: AtomicBool = AtomicBool::new(false);

/// Run X25519 + AES-128-GCM known-answer tests once, logging pass/fail.  This
/// pinpoints which primitive is wrong when the handshake hits a GCM AUTH FAIL.
fn selftest_once() {
    if SELFTEST_DONE.swap(true, AOrd::Relaxed) {
        return;
    }

    // AES-128-GCM KAT (NIST): K=0^128, IV=0^96, P="", AAD="" → tag
    // 58e2fccefa7e3061367f1d57a4e7455a (encrypt of empty plaintext = the tag).
    let tag = aes_gcm::encrypt(&[0u8; 16], &[0u8; 12], &[], &[]);
    let want_tag = hex16("58e2fccefa7e3061367f1d57a4e7455a");
    crate::serial_println!(
        "[tls] KAT AES-GCM: got={:02x?} ok={}",
        &tag,
        tag == want_tag
    );

    // X25519 KAT (RFC 7748 §5.2, iteration 1).
    let scalar = hex32("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
    let upoint = hex32("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
    let got = x25519::diffie_hellman(&scalar, &upoint);
    let want = hex32("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552");
    crate::serial_println!(
        "[tls] KAT X25519: ok={} got={:02x?}",
        got == want,
        &got[..8]
    );
}

fn hex16(s: &str) -> [u8; 16] {
    let b = s.as_bytes();
    let mut out = [0u8; 16];
    let nib = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
    for i in 0..16 {
        out[i] = (nib(b[i * 2]) << 4) | nib(b[i * 2 + 1]);
    }
    out
}

/// TLS 1.3 per-record nonce: static IV XOR the 64-bit record sequence number,
/// right-aligned into the low 8 bytes (RFC 8446 §5.3).
fn record_nonce(iv: &[u8; 12], seq: u64) -> [u8; 12] {
    let mut n = *iv;
    let sb = seq.to_be_bytes();
    for i in 0..8 {
        n[4 + i] ^= sb[i];
    }
    n
}

/// AES-GCM additional data for a TLS 1.3 record = its 5-byte header:
/// opaque_type(0x17) || legacy_version(0x0303) || ciphertext length (incl. tag).
fn record_aad(ct_len: usize) -> [u8; 5] {
    [0x17, 0x03, 0x03, (ct_len >> 8) as u8, ct_len as u8]
}

/// Encrypt `plaintext` (with TLS 1.3 inner content type `inner_type`) and send
/// it as one application_data record under `key`/`iv` at record `seq`.
#[allow(clippy::too_many_arguments)]
fn send_encrypted_record(
    src_port: u16,
    ip: [u8; 4],
    port: u16,
    key: &[u8; 16],
    iv: &[u8; 12],
    seq: u64,
    inner_type: u8,
    plaintext: &[u8],
) {
    let mut inner = plaintext.to_vec();
    inner.push(inner_type); // TLS 1.3 inner content type
    let ct_len = inner.len() + 16; // ciphertext = inner + 16-byte GCM tag
    let nonce = record_nonce(iv, seq);
    let aad = record_aad(ct_len);
    let ct = aes_gcm::encrypt(key, &nonce, &inner, &aad);
    let mut record = alloc::vec![0x17u8, 0x03, 0x03, (ct.len() >> 8) as u8, ct.len() as u8];
    record.extend_from_slice(&ct);
    crate::net::tcp::write(src_port, ip, port, &record);
    crate::net::virtio::flush_tx();
}

fn send_tls_record(src_port: u16, ip: [u8; 4], port: u16, content_type: u8, data: &[u8]) {
    // TLS record header: type(1) + legacy_version(2) + length(2)
    let mut record = alloc::vec![
        content_type,
        0x03,
        0x01,
        (data.len() >> 8) as u8,
        data.len() as u8,
    ];
    record.extend_from_slice(data);
    crate::net::tcp::write(src_port, ip, port, &record);
    crate::net::virtio::flush_tx();
}

fn generate_random_32() -> [u8; 32] {
    let mut r = [0u8; 32];
    let mut s = crate::shell::commands::boot_ticks() ^ 0xDEAD_BEEF_1337;
    for b in &mut r {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = s as u8;
    }
    r
}
