//! TLS 1.3 record-layer + handshake "flight" decoding.
//!
//! A server's TLS 1.3 first flight on the wire looks like:
//!   record(type=22 handshake)        → ServerHello              (plaintext)
//!   record(type=20 change_cipher_spec)                          (ignored in 1.3)
//!   record(type=23 application_data) → {EncryptedExtensions,
//!                                        Certificate,
//!                                        CertificateVerify,
//!                                        Finished}               (encrypted)
//!
//! This module turns that byte soup into structured records and handshake
//! messages: [`split_records`] frames the record layer, [`parse_handshakes`]
//! walks the concatenated handshake messages, [`strip_inner_type`] peels the
//! TLS 1.3 inner content-type, and [`decode_server_flight`] ties them together
//! (decrypting the type-23 records via a caller-supplied closure) to recover the
//! full handshake transcript.

use alloc::vec::Vec;

// Record content types.
pub const CT_CHANGE_CIPHER_SPEC: u8 = 20;
pub const CT_ALERT: u8 = 21;
pub const CT_HANDSHAKE: u8 = 22;
pub const CT_APPLICATION_DATA: u8 = 23;

// Handshake message types.
pub const HS_CLIENT_HELLO: u8 = 1;
pub const HS_SERVER_HELLO: u8 = 2;
pub const HS_NEW_SESSION_TICKET: u8 = 4;
pub const HS_ENCRYPTED_EXTENSIONS: u8 = 8;
pub const HS_CERTIFICATE: u8 = 11;
pub const HS_CERTIFICATE_VERIFY: u8 = 15;
pub const HS_FINISHED: u8 = 20;

/// One TLS record (TLSPlaintext / TLSCiphertext) framed off the wire.
pub struct Record<'a> {
    pub content_type: u8,
    pub version: u16,
    pub fragment: &'a [u8],
}

/// Split `buf` into complete TLS records.  Returns the records plus the number
/// of bytes consumed; a trailing partial record (header or body not yet fully
/// received) is left unconsumed for the next read.
pub fn split_records(buf: &[u8]) -> (Vec<Record<'_>>, usize) {
    let mut recs = Vec::new();
    let mut pos = 0usize;
    while pos + 5 <= buf.len() {
        let ct = buf[pos];
        let ver = u16::from_be_bytes([buf[pos + 1], buf[pos + 2]]);
        let len = u16::from_be_bytes([buf[pos + 3], buf[pos + 4]]) as usize;
        if len > (1 << 14) + 256 {
            break;
        } // RFC 8446 max record — bail on garbage
        if pos + 5 + len > buf.len() {
            break;
        } // incomplete trailing record
        recs.push(Record {
            content_type: ct,
            version: ver,
            fragment: &buf[pos + 5..pos + 5 + len],
        });
        pos += 5 + len;
    }
    (recs, pos)
}

/// One handshake message (after the 4-byte msg_type + 24-bit length header).
pub struct Handshake<'a> {
    pub msg_type: u8,
    pub body: &'a [u8],
}

/// Walk concatenated handshake messages out of a plaintext handshake byte stream
/// (the framing is the same whether the bytes came from a plaintext record or a
/// decrypted one).
pub fn parse_handshakes(buf: &[u8]) -> Vec<Handshake<'_>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= buf.len() {
        let mt = buf[pos];
        let len = ((buf[pos + 1] as usize) << 16)
            | ((buf[pos + 2] as usize) << 8)
            | (buf[pos + 3] as usize);
        if pos + 4 + len > buf.len() {
            break;
        }
        out.push(Handshake {
            msg_type: mt,
            body: &buf[pos + 4..pos + 4 + len],
        });
        pos += 4 + len;
    }
    out
}

/// A TLS 1.3 encrypted record's plaintext is `inner_content || inner_type ||
/// zero_padding`.  Strip trailing zero padding and the inner content-type byte,
/// returning `(inner_type, inner_content)`.
pub fn strip_inner_type(plaintext: &[u8]) -> Option<(u8, &[u8])> {
    let mut end = plaintext.len();
    while end > 0 && plaintext[end - 1] == 0 {
        end -= 1;
    }
    if end == 0 {
        return None;
    }
    Some((plaintext[end - 1], &plaintext[..end - 1]))
}

/// Extract the server's X25519 key share from a ServerHello handshake body
/// (the bytes after the 4-byte handshake header).  Parses the message fields
/// and extension list properly rather than byte-scanning for the group id.
pub fn server_hello_key_share(sh_body: &[u8]) -> Option<[u8; 32]> {
    // legacy_version(2) random(32) session_id(1+len) cipher_suite(2)
    // legacy_compression(1) extensions_len(2) extensions...
    let mut p = 2 + 32;
    if p >= sh_body.len() {
        return None;
    }
    let sid_len = sh_body[p] as usize;
    p += 1 + sid_len;
    p += 2 + 1; // cipher suite + compression method
    if p + 2 > sh_body.len() {
        return None;
    }
    let ext_len = u16::from_be_bytes([sh_body[p], sh_body[p + 1]]) as usize;
    p += 2;
    let ext_end = (p + ext_len).min(sh_body.len());
    while p + 4 <= ext_end {
        let et = u16::from_be_bytes([sh_body[p], sh_body[p + 1]]);
        let el = u16::from_be_bytes([sh_body[p + 2], sh_body[p + 3]]) as usize;
        p += 4;
        if p + el > ext_end {
            break;
        }
        if et == 0x0033 && el >= 4 {
            // key_share
            let group = u16::from_be_bytes([sh_body[p], sh_body[p + 1]]);
            let klen = u16::from_be_bytes([sh_body[p + 2], sh_body[p + 3]]) as usize;
            if group == 0x001D && klen == 32 && p + 4 + 32 <= sh_body.len() {
                let mut k = [0u8; 32];
                k.copy_from_slice(&sh_body[p + 4..p + 4 + 32]);
                return Some(k);
            }
        }
        p += el;
    }
    None
}

/// Return the full ServerHello handshake message (4-byte header + body) from a
/// raw server flight, for inclusion in the transcript hash.
pub fn server_hello_message(buf: &[u8]) -> Option<Vec<u8>> {
    let (records, _) = split_records(buf);
    for r in records {
        if r.content_type != CT_HANDSHAKE {
            continue;
        }
        let mut pos = 0usize;
        while pos + 4 <= r.fragment.len() {
            let mt = r.fragment[pos];
            let len = ((r.fragment[pos + 1] as usize) << 16)
                | ((r.fragment[pos + 2] as usize) << 8)
                | (r.fragment[pos + 3] as usize);
            if pos + 4 + len > r.fragment.len() {
                break;
            }
            if mt == HS_SERVER_HELLO {
                return Some(r.fragment[pos..pos + 4 + len].to_vec());
            }
            pos += 4 + len;
        }
    }
    None
}

/// Decode a server flight into the flat handshake transcript bytes.
///
/// Plaintext handshake records (ServerHello) pass through; ChangeCipherSpec is
/// dropped; each application-data record is decrypted via `decrypt(fragment,
/// seq)` (the caller owns the key/IV and the record sequence number we pass it),
/// and its inner handshake content is appended.  Alerts stop the flight.
/// The returned bytes can be fed straight to [`parse_handshakes`].
pub fn decode_server_flight<F>(buf: &[u8], mut decrypt: F) -> Vec<u8>
where
    F: FnMut(&[u8], u64) -> Option<Vec<u8>>,
{
    let (records, _consumed) = split_records(buf);
    let mut transcript = Vec::new();
    let mut seq = 0u64;
    for r in records {
        match r.content_type {
            CT_HANDSHAKE => transcript.extend_from_slice(r.fragment), // plaintext ServerHello
            CT_CHANGE_CIPHER_SPEC => {}                               // ignored in TLS 1.3
            CT_APPLICATION_DATA => {
                if let Some(pt) = decrypt(r.fragment, seq) {
                    seq += 1;
                    if let Some((CT_HANDSHAKE, inner)) = strip_inner_type(&pt) {
                        transcript.extend_from_slice(inner);
                    }
                }
            }
            CT_ALERT => break,
            _ => {}
        }
    }
    transcript
}

/// Parse a TLS 1.3 Certificate handshake body (RFC 8446 §4.4.2) into the DER
/// certificate chain, leaf first.  Layout:
///   certificate_request_context: 1-byte length + bytes (empty for a server)
///   certificate_list: 3-byte length, then entries of
///       cert_data:  3-byte length + DER certificate
///       extensions: 2-byte length + bytes
pub fn parse_certificate_chain(body: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if body.is_empty() {
        return out;
    }
    let ctx_len = body[0] as usize;
    let mut p = 1 + ctx_len;
    if p + 3 > body.len() {
        return out;
    }
    let list_len =
        ((body[p] as usize) << 16) | ((body[p + 1] as usize) << 8) | (body[p + 2] as usize);
    p += 3;
    let end = (p + list_len).min(body.len());
    while p + 3 <= end {
        let cl =
            ((body[p] as usize) << 16) | ((body[p + 1] as usize) << 8) | (body[p + 2] as usize);
        p += 3;
        if cl == 0 || p + cl > end {
            break;
        }
        out.push(body[p..p + cl].to_vec());
        p += cl;
        if p + 2 > end {
            break;
        }
        let ext_len = ((body[p] as usize) << 8) | (body[p + 1] as usize);
        p += 2 + ext_len;
    }
    out
}

/// Human-readable name for a handshake message type (for logging the flight).
pub fn hs_name(t: u8) -> &'static str {
    match t {
        HS_CLIENT_HELLO => "ClientHello",
        HS_SERVER_HELLO => "ServerHello",
        HS_NEW_SESSION_TICKET => "NewSessionTicket",
        HS_ENCRYPTED_EXTENSIONS => "EncryptedExtensions",
        HS_CERTIFICATE => "Certificate",
        HS_CERTIFICATE_VERIFY => "CertificateVerify",
        HS_FINISHED => "Finished",
        _ => "Unknown",
    }
}
