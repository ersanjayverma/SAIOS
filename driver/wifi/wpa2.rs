//! WPA2-PSK / WPA3-SAE authentication.
//!
//! Implements the EAPOL 4-way handshake needed to join a WPA2 network:
//!   1. AP  → STA : EAPOL-Key (ANonce)
//!   2. STA → AP  : EAPOL-Key (SNonce, MIC, RSN IE)
//!   3. AP  → STA : EAPOL-Key (GTK, MIC)
//!   4. STA → AP  : EAPOL-Key (ACK)
//!
//! Key derivation: PBKDF2-SHA1(password, SSID, 4096, 32) → PMK → PTK via PRF-512.

use alloc::string::String;
use alloc::vec::Vec;

// -- EAPOL frame constants -------------------------------------------------

const EAPOL_VERSION: u8 = 2;
const EAPOL_KEY: u8 = 3;

// Key Info flags
const KEY_TYPE_PAIRWISE: u16 = 1 << 3;
const KEY_MIC: u16 = 1 << 8;
const KEY_SECURE: u16 = 1 << 9;
const KEY_ACK: u16 = 1 << 7;
const KEY_INSTALL: u16 = 1 << 6;
const KEY_ENC: u16 = 1 << 10;

#[derive(Debug, Clone)]
pub struct EapolKey {
    pub key_info: u16,
    pub key_len: u16,
    pub replay_ctr: u64,
    pub nonce: [u8; 32],
    pub iv: [u8; 16],
    pub rsc: [u8; 8],
    pub mic: [u8; 16],
    pub key_data_len: u16,
    pub key_data: Vec<u8>,
}

impl EapolKey {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.len() < 99 {
            return None;
        }
        // Skip EAPOL header (4 bytes): version, type, length
        let body = &raw[4..];
        if body[0] != 2 {
            return None;
        } // descriptor type RSN
        let key_info = u16::from_be_bytes([body[1], body[2]]);
        let key_len = u16::from_be_bytes([body[3], body[4]]);
        let replay_ctr = u64::from_be_bytes(body[5..13].try_into().ok()?);
        let nonce: [u8; 32] = body[13..45].try_into().ok()?;
        let iv: [u8; 16] = body[45..61].try_into().ok()?;
        let rsc: [u8; 8] = body[61..69].try_into().ok()?;
        let _id: [u8; 8] = body[69..77].try_into().ok()?;
        let mic: [u8; 16] = body[77..93].try_into().ok()?;
        let kdl = u16::from_be_bytes([body[93], body[94]]) as usize;
        let key_data = body.get(95..95 + kdl).unwrap_or(&[]).to_vec();
        Some(Self {
            key_info,
            key_len,
            replay_ctr,
            nonce,
            iv,
            rsc,
            mic,
            key_data_len: kdl as u16,
            key_data,
        })
    }

    pub fn is_msg1(&self) -> bool {
        self.key_info & KEY_ACK != 0 && self.key_info & KEY_MIC == 0
    }
    pub fn is_msg3(&self) -> bool {
        self.key_info & KEY_ACK != 0
            && self.key_info & KEY_MIC != 0
            && self.key_info & KEY_INSTALL != 0
    }
}

// -- Key derivation --------------------------------------------------------

/// Derive the PMK from a WPA2-PSK passphrase and SSID.
/// Uses PBKDF2-HMAC-SHA1 with 4096 iterations.
pub fn derive_pmk(passphrase: &[u8], ssid: &[u8]) -> [u8; 32] {
    pbkdf2_sha1(passphrase, ssid, 4096, 32)
        .try_into()
        .unwrap_or([0u8; 32])
}

/// Derive PTK from PMK + MAC addresses + nonces using PRF-512.
pub fn derive_ptk(
    pmk: &[u8; 32],
    aa: &[u8; 6],  // AP  MAC
    spa: &[u8; 6], // STA MAC
    anonce: &[u8; 32],
    snonce: &[u8; 32],
) -> [u8; 64] {
    // PRF-512("Pairwise key expansion", min(AA,SPA) || max(AA,SPA) || min(ANonce,SNonce) || max)
    let mut data = Vec::with_capacity(76);
    if aa < spa {
        data.extend_from_slice(aa);
        data.extend_from_slice(spa);
    } else {
        data.extend_from_slice(spa);
        data.extend_from_slice(aa);
    }
    if anonce < snonce {
        data.extend_from_slice(anonce);
        data.extend_from_slice(snonce);
    } else {
        data.extend_from_slice(snonce);
        data.extend_from_slice(anonce);
    }

    prf_512(pmk, b"Pairwise key expansion", &data)
}

/// Build EAPOL message 2 of the 4-way handshake.
pub fn build_msg2(
    replay_ctr: u64,
    snonce: &[u8; 32],
    mic_key: &[u8; 16], // KCK = first 16 bytes of PTK
    rsn_ie: &[u8],
) -> Vec<u8> {
    let key_info: u16 = KEY_TYPE_PAIRWISE | KEY_MIC;
    let mut body = Vec::with_capacity(95 + rsn_ie.len());

    // EAPOL header: version=2, type=KEY, length
    let total_len = 91 + rsn_ie.len() as u16;
    body.push(EAPOL_VERSION);
    body.push(EAPOL_KEY);
    body.extend_from_slice(&total_len.to_be_bytes());

    // Key descriptor type = 2 (RSN)
    body.push(2);
    body.extend_from_slice(&key_info.to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes()); // key length
    body.extend_from_slice(&replay_ctr.to_be_bytes());
    body.extend_from_slice(snonce);
    body.extend_from_slice(&[0u8; 16]); // IV
    body.extend_from_slice(&[0u8; 8]); // RSC
    body.extend_from_slice(&[0u8; 8]); // ID
    // MIC placeholder (16 zero bytes — we'll fill it in)
    let mic_off = body.len();
    body.extend_from_slice(&[0u8; 16]);
    body.extend_from_slice(&(rsn_ie.len() as u16).to_be_bytes());
    body.extend_from_slice(rsn_ie);

    // Compute MIC over the whole EAPOL frame (HMAC-SHA1 first 16 bytes)
    let mic = hmac_sha1_16(mic_key, &body);
    body[mic_off..mic_off + 16].copy_from_slice(&mic);
    body
}

/// Build EAPOL message 4 (final ACK).
pub fn build_msg4(replay_ctr: u64, mic_key: &[u8; 16]) -> Vec<u8> {
    let key_info: u16 = KEY_TYPE_PAIRWISE | KEY_MIC | KEY_SECURE;
    let mut body = Vec::with_capacity(99);
    body.push(EAPOL_VERSION);
    body.push(EAPOL_KEY);
    body.extend_from_slice(&91u16.to_be_bytes());
    body.push(2);
    body.extend_from_slice(&key_info.to_be_bytes());
    body.extend_from_slice(&[0u8; 2]);
    body.extend_from_slice(&replay_ctr.to_be_bytes());
    body.extend_from_slice(&[0u8; 32]); // nonce = 0
    body.extend_from_slice(&[0u8; 16]); // IV
    body.extend_from_slice(&[0u8; 8]); // RSC
    body.extend_from_slice(&[0u8; 8]); // ID
    let mic_off = body.len();
    body.extend_from_slice(&[0u8; 16]);
    body.extend_from_slice(&0u16.to_be_bytes());
    let mic = hmac_sha1_16(mic_key, &body);
    body[mic_off..mic_off + 16].copy_from_slice(&mic);
    body
}

// -- Crypto primitives -----------------------------------------------------

/// PBKDF2-HMAC-SHA1 (WPA2 PMK derivation).
fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32, dklen: usize) -> Vec<u8> {
    let mut dk = Vec::with_capacity(dklen);
    let mut block_num = 1u32;
    while dk.len() < dklen {
        let mut u = {
            let mut s = salt.to_vec();
            s.extend_from_slice(&block_num.to_be_bytes());
            hmac_sha1(password, &s)
        };
        let mut xor = u;
        for _ in 1..iterations {
            u = hmac_sha1(password, &u);
            for (a, b) in xor.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        dk.extend_from_slice(&xor);
        block_num += 1;
    }
    dk.truncate(dklen);
    dk
}

/// PRF-512 (IEEE 802.11 pseudo-random function).
fn prf_512(key: &[u8], label: &[u8], data: &[u8]) -> [u8; 64] {
    let mut out = [0u8; 64];
    let mut pos = 0;
    for i in 0u8..4 {
        let mut input = label.to_vec();
        input.push(0x00);
        input.extend_from_slice(data);
        input.push(i);
        let h = hmac_sha1(key, &input);
        let copy = h.len().min(64 - pos);
        out[pos..pos + copy].copy_from_slice(&h[..copy]);
        pos += copy;
        if pos >= 64 {
            break;
        }
    }
    out
}

/// HMAC-SHA1.
fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let h = sha1(key);
        k[..20].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0u8; 64];
    let mut opad = [0u8; 64];
    for i in 0..64 {
        ipad[i] = k[i] ^ 0x36;
        opad[i] = k[i] ^ 0x5C;
    }
    let mut inner = ipad.to_vec();
    inner.extend_from_slice(data);
    let inner_hash = sha1(&inner);
    let mut outer = opad.to_vec();
    outer.extend_from_slice(&inner_hash);
    sha1(&outer)
}

/// Return first 16 bytes of HMAC-SHA1 (for MIC computation).
fn hmac_sha1_16(key: &[u8], data: &[u8]) -> [u8; 16] {
    let h = hmac_sha1(key, data);
    let mut out = [0u8; 16];
    out.copy_from_slice(&h[..16]);
    out
}

/// SHA-1 hash.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for i in 0..5 {
        out[i * 4..(i + 1) * 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}
