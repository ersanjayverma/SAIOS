//! AES-128-GCM authenticated encryption (NIST SP 800-38D).
//! Used as the TLS 1.3 record protection cipher.

use alloc::vec::Vec;

// -- AES-128 ----------------------------------------------------------------

const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

fn aes128_expand_key(key: &[u8; 16]) -> [[u8; 16]; 11] {
    let mut w = [[0u8; 4]; 44];
    for i in 0..4 {
        w[i].copy_from_slice(&key[i * 4..(i + 1) * 4]);
    }
    for i in 4..44 {
        let mut temp = w[i - 1];
        if i % 4 == 0 {
            temp.rotate_left(1);
            for b in &mut temp {
                *b = SBOX[*b as usize];
            }
            temp[0] ^= RCON[i / 4 - 1];
        }
        for j in 0..4 {
            w[i][j] = w[i - 4][j] ^ temp[j];
        }
    }
    let mut rk = [[0u8; 16]; 11];
    for i in 0..11 {
        for j in 0..4 {
            rk[i][j * 4..(j + 1) * 4].copy_from_slice(&w[i * 4 + j]);
        }
    }
    rk
}

fn xtime(b: u8) -> u8 {
    if b & 0x80 != 0 {
        (b << 1) ^ 0x1B
    } else {
        b << 1
    }
}
fn mul(a: u8, b: u8) -> u8 {
    let mut r = 0u8;
    let mut a = a;
    let mut b = b;
    for _ in 0..8 {
        if b & 1 != 0 {
            r ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    r
}

fn aes128_encrypt_block(block: &[u8; 16], rk: &[[u8; 16]; 11]) -> [u8; 16] {
    let mut s = *block;
    // Add initial round key
    for i in 0..16 {
        s[i] ^= rk[0][i];
    }
    for (round, round_key) in rk.iter().enumerate().take(10 + 1).skip(1) {
        // SubBytes
        for b in &mut s {
            *b = SBOX[*b as usize];
        }
        // ShiftRows
        let tmp = s[1];
        s[1] = s[5];
        s[5] = s[9];
        s[9] = s[13];
        s[13] = tmp;
        let (tmp0, tmp1) = (s[2], s[6]);
        s[2] = s[10];
        s[6] = s[14];
        s[10] = tmp0;
        s[14] = tmp1;
        let tmp = s[15];
        s[15] = s[11];
        s[11] = s[7];
        s[7] = s[3];
        s[3] = tmp;
        // MixColumns (skip in final round).  The state is column-major (matching
        // ShiftRows above), so column c is the four CONTIGUOUS bytes s[4c..4c+4]
        // — not s[c], s[c+4], ... (which is a row, and was the bug).
        if round < 10 {
            for c in 0..4 {
                let o = 4 * c;
                let (a, b, cc, d) = (s[o], s[o + 1], s[o + 2], s[o + 3]);
                s[o] = mul(2, a) ^ mul(3, b) ^ cc ^ d;
                s[o + 1] = a ^ mul(2, b) ^ mul(3, cc) ^ d;
                s[o + 2] = a ^ b ^ mul(2, cc) ^ mul(3, d);
                s[o + 3] = mul(3, a) ^ b ^ cc ^ mul(2, d);
            }
        }
        // AddRoundKey
        for i in 0..16 {
            s[i] ^= round_key[i];
        }
    }
    s
}

// -- GCM -------------------------------------------------------------------

fn gcm_mult(x: &[u8; 16], h: &[u8; 16]) -> [u8; 16] {
    let mut z = [0u8; 16];
    let mut v = *h;
    for byte in x.iter().take(16) {
        for bit in (0..8).rev() {
            if (byte >> bit) & 1 != 0 {
                for j in 0..16 {
                    z[j] ^= v[j];
                }
            }
            let lsb = v[15] & 1;
            // Right shift v by 1
            for j in (1..16).rev() {
                v[j] = (v[j] >> 1) | ((v[j - 1] & 1) << 7);
            }
            v[0] >>= 1;
            if lsb != 0 {
                v[0] ^= 0xE1;
            }
        }
    }
    z
}

fn ghash(h: &[u8; 16], data: &[u8]) -> [u8; 16] {
    let mut y = [0u8; 16];
    for chunk in data.chunks(16) {
        let mut block = [0u8; 16];
        block[..chunk.len()].copy_from_slice(chunk);
        for i in 0..16 {
            y[i] ^= block[i];
        }
        y = gcm_mult(&y, h);
    }
    y
}

fn gcm_counter(iv: &[u8; 12], ctr: u32) -> [u8; 16] {
    let mut block = [0u8; 16];
    block[..12].copy_from_slice(iv);
    block[12..].copy_from_slice(&ctr.to_be_bytes());
    block
}

/// AES-128-GCM encrypt. Returns ciphertext || tag (16 bytes).
pub fn encrypt(key: &[u8; 16], nonce: &[u8; 12], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let rk = aes128_expand_key(key);
    let h_block = aes128_encrypt_block(&[0u8; 16], &rk);
    let h = &h_block;

    // Encrypt
    let mut ct = Vec::with_capacity(plaintext.len());
    for (ctr, chunk) in (2u32..).zip(plaintext.chunks(16)) {
        let keystream = aes128_encrypt_block(&gcm_counter(nonce, ctr), &rk);
        for (i, &b) in chunk.iter().enumerate() {
            ct.push(b ^ keystream[i]);
        }
    }

    // GHASH(H, AAD || CT || lengths)
    let mut ghash_input = Vec::new();
    ghash_input.extend_from_slice(aad);
    // Pad AAD to 16 bytes
    while ghash_input.len() % 16 != 0 {
        ghash_input.push(0);
    }
    ghash_input.extend_from_slice(&ct);
    while ghash_input.len() % 16 != 0 {
        ghash_input.push(0);
    }
    let aad_bits = (aad.len() as u64 * 8).to_be_bytes();
    let ct_bits = (ct.len() as u64 * 8).to_be_bytes();
    ghash_input.extend_from_slice(&aad_bits);
    ghash_input.extend_from_slice(&ct_bits);

    let s = ghash(h, &ghash_input);
    let j0_enc = aes128_encrypt_block(&gcm_counter(nonce, 1), &rk);
    let mut tag = [0u8; 16];
    for i in 0..16 {
        tag[i] = s[i] ^ j0_enc[i];
    }

    ct.extend_from_slice(&tag);
    ct
}

/// AES-128-GCM decrypt. Returns plaintext or error if tag doesn't match.
pub fn decrypt(
    key: &[u8; 16],
    nonce: &[u8; 12],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<alloc::vec::Vec<u8>, &'static str> {
    if ciphertext.len() < 16 {
        return Err("aes-gcm: ciphertext too short");
    }
    let (ct, tag) = ciphertext.split_at(ciphertext.len() - 16);
    let rk = aes128_expand_key(key);
    let h_block = aes128_encrypt_block(&[0u8; 16], &rk);

    // Verify tag
    let mut ghash_input = Vec::new();
    ghash_input.extend_from_slice(aad);
    while ghash_input.len() % 16 != 0 {
        ghash_input.push(0);
    }
    ghash_input.extend_from_slice(ct);
    while ghash_input.len() % 16 != 0 {
        ghash_input.push(0);
    }
    ghash_input.extend_from_slice(&(aad.len() as u64 * 8).to_be_bytes());
    ghash_input.extend_from_slice(&(ct.len() as u64 * 8).to_be_bytes());

    let s = ghash(&h_block, &ghash_input);
    let j0_enc = aes128_encrypt_block(&gcm_counter(nonce, 1), &rk);
    let mut expected_tag = [0u8; 16];
    for i in 0..16 {
        expected_tag[i] = s[i] ^ j0_enc[i];
    }

    // Constant-time comparison
    let mut diff = 0u8;
    for i in 0..16 {
        diff |= expected_tag[i] ^ tag[i];
    }
    if diff != 0 {
        return Err("aes-gcm: authentication tag mismatch");
    }

    // Decrypt
    let mut pt = Vec::with_capacity(ct.len());
    for (ctr, chunk) in (2u32..).zip(ct.chunks(16)) {
        let keystream = aes128_encrypt_block(&gcm_counter(nonce, ctr), &rk);
        for (i, &b) in chunk.iter().enumerate() {
            pt.push(b ^ keystream[i]);
        }
    }
    Ok(pt)
}
