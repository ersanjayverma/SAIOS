//! X25519 Diffie-Hellman (RFC 7748).
//! Used as the key exchange mechanism in TLS 1.3.
//! Implements Curve25519 scalar multiplication.

/// Generate a private key from random bytes.
pub fn generate_private_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    let ticks = crate::shell::commands::boot_ticks();
    let mut s = ticks ^ 0xCAFE_BABE_DEAD_BEEF;
    for b in &mut k {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        *b = s as u8;
    }
    // Clamp as per RFC 7748
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    k
}

/// Compute the public key from a private key (scalar * base point).
pub fn public_from_private(private: &[u8; 32]) -> [u8; 32] {
    // Base point u = 9
    let mut base = [0u8; 32];
    base[0] = 9;
    scalar_mult(private, &base)
}

/// Compute the shared secret: scalar * peer_public.
pub fn diffie_hellman(private: &[u8; 32], peer_public: &[u8; 32]) -> [u8; 32] {
    scalar_mult(private, peer_public)
}

// â”€â”€ Curve25519 scalar multiplication (RFC 7748 Â§5) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// p = 2^255 - 19
const P: U256 = U256([
    0xFFFF_FFFF_FFFF_FFED,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_FFFF_FFFF,
    0x7FFF_FFFF_FFFF_FFFF,
]);

const A24: u64 = 121665; // (A-2)/4 = (486662-2)/4

fn scalar_mult(scalar: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    // Clamp the scalar (RFC 7748 §5) - required for EVERY scalar mult, not just
    // freshly generated keys, or the result is wrong.
    let mut e = *scalar;
    e[0] &= 248;
    e[31] &= 127;
    e[31] |= 64;

    // Decode u coordinate
    let u_field = decode_u255(u);

    // Montgomery ladder
    let x1 = u_field;
    let mut x2 = U256::one();
    let mut z2 = U256::zero();
    let mut x3 = u_field;
    let mut z3 = U256::one();
    let mut swap = 0u8;

    for i in (0..255).rev() {
        let k_bit = (e[i >> 3] >> (i & 7)) & 1;
        swap ^= k_bit;
        cswap(swap, &mut x2, &mut x3);
        cswap(swap, &mut z2, &mut z3);
        swap = k_bit;

        let a = add_mod(x2, z2);
        let aa = mul_mod(a, a);
        let b = sub_mod(x2, z2);
        let bb = mul_mod(b, b);
        let e = sub_mod(aa, bb);
        let c = add_mod(x3, z3);
        let d = sub_mod(x3, z3);
        let da = mul_mod(d, a);
        let cb = mul_mod(c, b);
        x3 = add_mod(da, cb);
        x3 = mul_mod(x3, x3);
        z3 = sub_mod(da, cb);
        z3 = mul_mod(z3, z3);
        z3 = mul_mod(z3, x1);
        x2 = mul_mod(aa, bb);
        z2 = mul_mod(U256::from_u64(A24), e);
        z2 = add_mod(z2, aa);
        z2 = mul_mod(e, z2);
    }
    cswap(swap, &mut x2, &mut x3);
    cswap(swap, &mut z2, &mut z3);

    // Result = x2 * z2^(p-2) mod p  (modular inverse via Fermat's little theorem)
    let inv = pow_mod(z2, exp_p_minus_2());
    let result = mul_mod(x2, inv);
    encode_u255(result)
}

// â”€â”€ Big-integer arithmetic mod 2^255-19 â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Clone, Copy)]
struct U256([u64; 4]);

impl U256 {
    fn zero() -> Self {
        U256([0; 4])
    }
    fn one() -> Self {
        U256([1, 0, 0, 0])
    }
    fn from_u64(v: u64) -> Self {
        U256([v, 0, 0, 0])
    }
}

fn decode_u255(b: &[u8; 32]) -> U256 {
    let mut words = [0u64; 4];
    for i in 0..4 {
        for j in 0..8 {
            words[i] |= (b[i * 8 + j] as u64) << (j * 8);
        }
    }
    words[3] &= 0x7FFF_FFFF_FFFF_FFFF;
    U256(words)
}

fn encode_u255(u: U256) -> [u8; 32] {
    let mut b = [0u8; 32];
    for i in 0..4 {
        for j in 0..8 {
            b[i * 8 + j] = (u.0[i] >> (j * 8)) as u8;
        }
    }
    b
}

fn add_mod(a: U256, b: U256) -> U256 {
    let mut carry = 0u128;
    let mut r = [0u64; 4];
    for (i, slot) in r.iter_mut().enumerate() {
        let s = a.0[i] as u128 + b.0[i] as u128 + carry;
        *slot = s as u64;
        carry = s >> 64;
    }
    // Reduce mod p if needed
    let mut result = U256(r);
    if carry > 0 || cmp_ge(result, P) {
        result = sub_raw(result, P);
    }
    result
}

fn sub_mod(a: U256, b: U256) -> U256 {
    if cmp_ge(a, b) {
        sub_raw(a, b)
    } else {
        add_mod(a, sub_raw(P, b))
    }
}

fn sub_raw(a: U256, b: U256) -> U256 {
    let mut borrow = 0i128;
    let mut r = [0u64; 4];
    for (i, slot) in r.iter_mut().enumerate() {
        let d = a.0[i] as i128 - b.0[i] as i128 - borrow;
        *slot = d as u64;
        borrow = if d < 0 { 1 } else { 0 };
    }
    U256(r)
}

fn mul_mod(a: U256, b: U256) -> U256 {
    // Schoolbook 4x4 → 8-limb (512-bit) product with carry propagation.  Each
    // step a[i]*b[j] + t[i+j] + carry is at most (2^64-1)^2 + 2*(2^64-1) =
    // 2^128 - 1, so it fits EXACTLY in a u128 (the previous `prod[i+j] += ...`
    // summed up to four ~2^128 terms and overflowed u128 - corrupting every
    // multiply).
    let mut t = [0u64; 8];
    for i in 0..4 {
        let mut carry: u64 = 0;
        for j in 0..4 {
            let cur = a.0[i] as u128 * b.0[j] as u128 + t[i + j] as u128 + carry as u128;
            t[i + j] = cur as u64;
            carry = (cur >> 64) as u64;
        }
        t[i + 4] = carry; // fresh high limb for this row
    }
    reduce_512(t)
}

/// Reduce a 512-bit value (8 limbs, little-endian) mod p = 2^255 - 19.
/// Uses 2^256 ≡ 38 (mod p): fold the high half into the low half (×38) until the
/// high limbs vanish, then a final conditional subtraction of p.
fn reduce_512(mut t: [u64; 8]) -> U256 {
    loop {
        if t[4] == 0 && t[5] == 0 && t[6] == 0 && t[7] == 0 {
            break;
        }
        let mut acc = [0u128; 8];
        for i in 0..4 {
            acc[i] = t[i] as u128;
        }
        for i in 0..4 {
            acc[i] += 38u128 * t[i + 4] as u128;
        }
        let mut carry = 0u128;
        for i in 0..8 {
            let v = acc[i] + carry;
            t[i] = v as u64;
            carry = v >> 64;
        }
        // any final carry folds back via 2^512 ≡ 38^2, but t[4..8] now hold the
        // spill and the loop repeats until they are zero (converges fast).
    }
    let mut r = U256([t[0], t[1], t[2], t[3]]);
    // r < 2^256 ≈ 2p, so at most two subtractions bring it into [0, p).
    while cmp_ge(r, P) {
        r = sub_raw(r, P);
    }
    r
}

fn pow_mod(base: U256, exp: [u64; 4]) -> U256 {
    let mut result = U256::one();
    let mut b = base;
    for word in &exp {
        for bit in 0..64 {
            if word & (1 << bit) != 0 {
                result = mul_mod(result, b);
            }
            b = mul_mod(b, b);
        }
    }
    result
}

fn exp_p_minus_2() -> [u64; 4] {
    // p - 2 = 2^255 - 21
    [
        0xFFFF_FFFF_FFFF_FFEB,
        0xFFFF_FFFF_FFFF_FFFF,
        0xFFFF_FFFF_FFFF_FFFF,
        0x7FFF_FFFF_FFFF_FFFF,
    ]
}

fn cmp_ge(a: U256, b: U256) -> bool {
    for i in (0..4).rev() {
        if a.0[i] > b.0[i] {
            return true;
        }
        if a.0[i] < b.0[i] {
            return false;
        }
    }
    true
}

fn cswap(do_swap: u8, a: &mut U256, b: &mut U256) {
    if do_swap != 0 {
        core::mem::swap(a, b);
    }
}
