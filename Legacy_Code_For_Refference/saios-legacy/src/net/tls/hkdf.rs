//! HKDF (HMAC-based Key Derivation Function) — RFC 5869 with SHA-256.
//! Used by TLS 1.3 for deriving all session keys.

use super::sha256::{hash, hmac};
use alloc::vec::Vec;

/// HKDF-Extract: PRK = HMAC-Hash(salt, ikm)
pub fn extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    hmac(salt, ikm)
}

/// HKDF-Expand to `len` bytes.
pub fn expand(prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut t = Vec::new();
    let mut ctr = 1u8;
    while out.len() < len {
        let mut data = t.clone();
        data.extend_from_slice(info);
        data.push(ctr);
        t = hmac(prk, &data).to_vec();
        out.extend_from_slice(&t);
        ctr += 1;
    }
    out.truncate(len);
    out
}

/// TLS 1.3 HKDF-Expand-Label.
///
/// HkdfLabel = length(2) + "tls13 " + label + context_length(1) + context
pub fn expand_label(prk: &[u8], label: &[u8], context: &[u8], len: usize) -> Vec<u8> {
    let mut info = Vec::new();
    info.push((len >> 8) as u8);
    info.push(len as u8);
    info.push((6 + label.len()) as u8);
    info.extend_from_slice(b"tls13 ");
    info.extend_from_slice(label);
    info.push(context.len() as u8);
    info.extend_from_slice(context);
    expand(prk, &info, len)
}

/// Convenience: expand_label returning exactly 16 bytes (AES-128 key).
pub fn expand_label_16(prk: &[u8], label: &[u8], context: &[u8]) -> [u8; 16] {
    let v = expand_label(prk, label, context, 16);
    let mut out = [0u8; 16];
    out.copy_from_slice(&v);
    out
}

/// Convenience: expand_label returning exactly 12 bytes (AES-GCM IV).
pub fn expand_label_12(prk: &[u8], label: &[u8], context: &[u8]) -> [u8; 12] {
    let v = expand_label(prk, label, context, 12);
    let mut out = [0u8; 12];
    out.copy_from_slice(&v);
    out
}
