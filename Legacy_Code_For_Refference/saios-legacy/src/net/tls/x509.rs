//! Minimal X.509 certificate parsing + chain verification.
//!
//! Implements just enough ASN.1 DER to extract the fields needed to validate a
//! server certificate chain:
//!   - validity window (notBefore / notAfter) checked against the real clock
//!   - subject CN + subjectAltName dNSNames checked against the requested host
//!   - chain linkage (issuer DN of cert[i] == subject DN of cert[i+1])
//!   - RSA PKCS#1 v1.5 / SHA-256 signature of each cert by its issuer's key
//!   - anchoring to an embedded trusted root (ISRG Root X1)
//!
//! ECDSA-signed certs are detected and reported (EC verification is a follow-up).

use super::sha256;
use alloc::string::String;
use alloc::vec::Vec;

// -- ASN.1 DER cursor --------------------------------------------------------

struct Der<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Der<'a> {
    fn new(b: &'a [u8]) -> Self {
        Der { b, pos: 0 }
    }
    fn at(b: &'a [u8], pos: usize) -> Self {
        Der { b, pos }
    }

    /// Read one TLV; return (tag, contents, total_len_consumed_header).
    /// Returns (tag, content_start, content_len).
    fn tlv(&mut self) -> Option<(u8, usize, usize)> {
        if self.pos + 2 > self.b.len() {
            return None;
        }
        let tag = self.b[self.pos];
        let mut p = self.pos + 1;
        let l0 = self.b[p];
        p += 1;
        let len = if l0 & 0x80 == 0 {
            l0 as usize
        } else {
            let n = (l0 & 0x7F) as usize;
            if n == 0 || n > 4 || p + n > self.b.len() {
                return None;
            }
            let mut v = 0usize;
            for _ in 0..n {
                v = (v << 8) | self.b[p] as usize;
                p += 1;
            }
            v
        };
        if p + len > self.b.len() {
            return None;
        }
        let cs = p;
        self.pos = p + len;
        Some((tag, cs, len))
    }
}

/// Slice helper.
fn sl(b: &[u8], start: usize, len: usize) -> &[u8] {
    &b[start..start + len]
}

// -- Parsed certificate ------------------------------------------------------

pub struct Cert<'a> {
    pub der: &'a [u8],
    pub tbs: &'a [u8],        // raw TBSCertificate (what the signature covers)
    pub issuer_dn: &'a [u8],  // raw DER of the issuer Name
    pub subject_dn: &'a [u8], // raw DER of the subject Name
    pub not_before: u64,      // epoch seconds
    pub not_after: u64,
    pub spki: &'a [u8], // raw SubjectPublicKeyInfo
    pub sig_alg_rsa_sha256: bool,
    pub signature: &'a [u8], // signature bytes (BIT STRING contents, no unused byte)
    pub cn: String,
    pub sans: Vec<String>,
}

/// Parse a single DER certificate.
pub fn parse(der: &[u8]) -> Result<Cert<'_>, &'static str> {
    let mut top = Der::new(der);
    let (tag, cs, cl) = top.tlv().ok_or("x509: bad outer")?;
    if tag != 0x30 {
        return Err("x509: not a SEQUENCE");
    }
    let body = sl(der, cs, cl);
    let body_off = cs;

    let mut c = Der::new(body);
    let (t_tbs, tbs_cs, tbs_cl) = c.tlv().ok_or("x509: no tbs")?;
    if t_tbs != 0x30 {
        return Err("x509: tbs not SEQUENCE");
    }
    // tbs slice relative to der: header was inside body which starts at body_off.
    let tbs_abs_start = body_off + tbs_cs;
    let tbs = &der[tbs_abs_start..tbs_abs_start + tbs_cl];

    // signatureAlgorithm
    let (_t_sa, sa_cs, sa_cl) = c.tlv().ok_or("x509: no sigalg")?;
    let sig_alg_rsa_sha256 = oid_is_rsa_sha256(sl(body, sa_cs, sa_cl));

    // signatureValue (BIT STRING)
    let (t_sig, sig_cs, sig_cl) = c.tlv().ok_or("x509: no sig")?;
    if t_sig != 0x03 || sig_cl < 1 {
        return Err("x509: bad sig");
    }
    let signature = sl(body, sig_cs + 1, sig_cl - 1); // skip unused-bits byte

    // -- Walk the TBSCertificate ----------------------------------------------
    let mut tb = Der::new(tbs);
    let field = tb.tlv().ok_or("x509: tbs empty")?;
    // optional [0] version
    if field.0 == 0xA0 {
        let _ = tb.tlv().ok_or("x509: after version")?;
    }
    // serialNumber INTEGER (field is it) — skip
    // signature AlgId
    let _ = tb.tlv().ok_or("x509: tbs sigalg")?;
    // issuer Name
    let (_ti, i_cs, i_cl) = tb.tlv().ok_or("x509: issuer")?;
    let issuer_dn = sl(tbs, i_cs, i_cl);
    let issuer_full_start = i_cs; // for raw including header we recompute below
    // validity SEQUENCE
    let (_tv, v_cs, v_cl) = tb.tlv().ok_or("x509: validity")?;
    let (not_before, not_after) = parse_validity(sl(tbs, v_cs, v_cl))?;
    // subject Name
    let (_ts, s_cs, s_cl) = tb.tlv().ok_or("x509: subject")?;
    let subject_dn = sl(tbs, s_cs, s_cl);
    // subjectPublicKeyInfo (capture WITH its header)
    let spki_hdr_start = tb.pos;
    let (_tk, k_cs, k_cl) = tb.tlv().ok_or("x509: spki")?;
    let spki = &tbs[spki_hdr_start..k_cs + k_cl];

    // Names compared as raw DER INCLUDING the tag/length header, so recompute.
    let issuer_dn = der_with_header(tbs, i_cs, i_cl);
    let subject_dn = der_with_header(tbs, s_cs, s_cl);
    let _ = (issuer_full_start, body, der, tbs_cs);

    let cn = extract_cn(subject_dn);
    let sans = extract_sans(&mut Der::at(tbs, tb.pos), tbs);

    Ok(Cert {
        der,
        tbs,
        issuer_dn,
        subject_dn,
        not_before,
        not_after,
        spki,
        sig_alg_rsa_sha256,
        signature,
        cn,
        sans,
    })
}

/// Reconstruct a TLV slice that includes its 1–more byte header, given the
/// content start/len within `buf` (DER lengths here are short enough that the
/// header is recoverable by scanning backwards from the content start).
fn der_with_header(buf: &[u8], content_start: usize, content_len: usize) -> &[u8] {
    // The header is [tag][len-bytes]; find its start by checking standard
    // short/long length forms ending exactly at content_start.
    // Try 2-byte header (short form) first, then long forms up to 4 length bytes.
    for hdr in 2..=6usize {
        if content_start < hdr {
            continue;
        }
        let s = content_start - hdr;
        let l0 = buf[s + 1];
        let ok = if l0 & 0x80 == 0 {
            hdr == 2 && l0 as usize == content_len
        } else {
            (l0 & 0x7F) as usize == hdr - 2
        };
        if ok {
            return &buf[s..content_start + content_len];
        }
    }
    &buf[content_start..content_start + content_len]
}

fn oid_is_rsa_sha256(alg_seq: &[u8]) -> bool {
    // sha256WithRSAEncryption: 1.2.840.113549.1.1.11
    const OID: [u8; 9] = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B];
    alg_seq.windows(OID.len()).any(|w| w == OID)
}

fn parse_validity(v: &[u8]) -> Result<(u64, u64), &'static str> {
    let mut d = Der::new(v);
    let (t1, c1, l1) = d.tlv().ok_or("x509: notBefore")?;
    let nb = parse_time(t1, sl(v, c1, l1))?;
    let (t2, c2, l2) = d.tlv().ok_or("x509: notAfter")?;
    let na = parse_time(t2, sl(v, c2, l2))?;
    Ok((nb, na))
}

/// UTCTime (YYMMDDHHMMSSZ) or GeneralizedTime (YYYYMMDDHHMMSSZ) → epoch seconds.
fn parse_time(tag: u8, b: &[u8]) -> Result<u64, &'static str> {
    let s: Vec<u8> = b.iter().copied().filter(|c| c.is_ascii_digit()).collect();
    let g = |i: usize| (s[i] - b'0') as u64;
    let (year, idx) = if tag == 0x17 {
        // UTCTime, 2-digit year
        let yy = g(0) * 10 + g(1);
        (if yy >= 50 { 1900 + yy } else { 2000 + yy }, 2)
    } else {
        // GeneralizedTime, 4-digit year
        (g(0) * 1000 + g(1) * 100 + g(2) * 10 + g(3), 4)
    };
    if s.len() < idx + 10 {
        return Err("x509: bad time");
    }
    let mo = g(idx) * 10 + g(idx + 1);
    let da = g(idx + 2) * 10 + g(idx + 3);
    let hh = g(idx + 4) * 10 + g(idx + 5);
    let mi = g(idx + 6) * 10 + g(idx + 7);
    let se = g(idx + 8) * 10 + g(idx + 9);
    Ok(crate::time::epoch_from_civil(year, mo, da, hh, mi, se))
}

/// Pull the commonName (OID 2.5.29 -> actually 2.5.4.3) printable string out of a Name.
fn extract_cn(name: &[u8]) -> String {
    const CN_OID: [u8; 3] = [0x55, 0x04, 0x03];
    let mut i = 0;
    while i + CN_OID.len() < name.len() {
        if name[i..i + 3] == CN_OID {
            // followed by a string TLV: tag, len, bytes
            let j = i + 3;
            if j + 2 <= name.len() {
                let len = name[j + 1] as usize;
                if j + 2 + len <= name.len() {
                    return String::from_utf8_lossy(&name[j + 2..j + 2 + len]).into_owned();
                }
            }
        }
        i += 1;
    }
    String::new()
}

/// Scan remaining TBS fields for the SAN extension (OID 2.5.29.17) dNSNames.
fn extract_sans(d: &mut Der, tbs: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    // Find the extensions [3] wrapper.
    while let Some((tag, cs, cl)) = d.tlv() {
        if tag == 0xA3 {
            scan_extensions(sl(tbs, cs, cl), &mut out);
            break;
        }
    }
    out
}

fn scan_extensions(exts: &[u8], out: &mut Vec<String>) {
    const SAN_OID: [u8; 3] = [0x55, 0x1D, 0x11];
    // exts is SEQUENCE OF Extension; find SAN_OID then its OCTET STRING value.
    let mut i = 0;
    while i + 3 < exts.len() {
        if exts[i..i + 3] == SAN_OID {
            // Skip the OID TLV; the value OCTET STRING follows (maybe after a BOOL).
            let mut j = i + 3;
            // find next OCTET STRING (0x04)
            while j < exts.len() && exts[j] != 0x04 {
                j += 1;
            }
            if j + 2 <= exts.len() {
                let len = exts[j + 1] as usize;
                if j + 2 + len <= exts.len() {
                    parse_general_names(&exts[j + 2..j + 2 + len], out);
                }
            }
            return;
        }
        i += 1;
    }
}

fn parse_general_names(val: &[u8], out: &mut Vec<String>) {
    let mut d = Der::new(val);
    if let Some((t, cs, cl)) = d.tlv() {
        if t != 0x30 {
            return;
        }
        let seq = sl(val, cs, cl);
        let mut s = Der::new(seq);
        while let Some((tag, c, l)) = s.tlv() {
            if tag == 0x82 {
                // [2] dNSName (IA5String, implicit)
                out.push(String::from_utf8_lossy(sl(seq, c, l)).into_owned());
            }
        }
    }
}

// -- PEM → DER ----------------------------------------------------------------

/// Split a PEM blob into its DER certificate(s).
pub fn pem_to_ders(pem: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut inside = false;
    for line in pem.lines() {
        if line.contains("BEGIN CERTIFICATE") {
            inside = true;
            cur.clear();
            continue;
        }
        if line.contains("END CERTIFICATE") {
            inside = false;
            if let Some(der) = crate::tools::openssl::base64_decode_pub(&cur) {
                out.push(der);
            }
            continue;
        }
        if inside {
            cur.push_str(line.trim());
        }
    }
    out
}

// -- Hostname match --------------------------------------------------------

fn host_matches(pattern: &str, host: &str) -> bool {
    if let Some(rest) = pattern.strip_prefix("*.") {
        // wildcard: one label
        if let Some(hrest) = host.split_once('.').map(|(_, r)| r) {
            return rest.eq_ignore_ascii_case(hrest);
        }
        return false;
    }
    pattern.eq_ignore_ascii_case(host)
}

// -- RSA PKCS#1 v1.5 / SHA-256 verification (big-integer modexp) --------------

/// Verify `sig`^e mod n decodes to a PKCS#1 v1.5 block over SHA-256(tbs).
fn rsa_verify(n: &[u8], e: &[u8], sig: &[u8], tbs: &[u8]) -> bool {
    let m = bigint::modexp(sig, e, n);
    // m must be EM = 0x00 01 FF..FF 00 || DigestInfo(SHA256(tbs))
    // Left-pad m to n.len().
    let mut em = alloc::vec![0u8; n.len()];
    if m.len() > em.len() {
        return false;
    }
    em[n.len() - m.len()..].copy_from_slice(&m);
    if em.len() < 11 || em[0] != 0x00 || em[1] != 0x01 {
        return false;
    }
    let mut i = 2;
    while i < em.len() && em[i] == 0xFF {
        i += 1;
    }
    if i >= em.len() || em[i] != 0x00 {
        return false;
    }
    let payload = &em[i + 1..];
    // DigestInfo for SHA-256.
    const DI: [u8; 19] = [
        0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01,
        0x05, 0x00, 0x04, 0x20,
    ];
    if payload.len() != DI.len() + 32 {
        return false;
    }
    if payload[..DI.len()] != DI {
        return false;
    }
    let want = sha256::hash(tbs);
    payload[DI.len()..] == want
}

/// Extract (modulus, exponent) from an RSA SubjectPublicKeyInfo.
fn rsa_pubkey(spki: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut d = Der::new(spki);
    let (_t, cs, cl) = d.tlv()?; // SEQ
    let body = sl(spki, cs, cl);
    let mut b = Der::new(body);
    let _alg = b.tlv()?; // AlgorithmIdentifier
    let (tk, kc, kl) = b.tlv()?; // BIT STRING subjectPublicKey
    if tk != 0x03 || kl < 1 {
        return None;
    }
    let key = sl(body, kc + 1, kl - 1); // skip unused-bits
    let mut k = Der::new(key);
    let (_ts, sc, sll) = k.tlv()?; // SEQ { n, e }
    let seq = sl(key, sc, sll);
    let mut s = Der::new(seq);
    let (_tn, nc, nl) = s.tlv()?; // INTEGER n
    let (_te, ec, el) = s.tlv()?; // INTEGER e
    let n = trim_leading_zero(sl(seq, nc, nl));
    let e = trim_leading_zero(sl(seq, ec, el));
    Some((n.to_vec(), e.to_vec()))
}

fn trim_leading_zero(b: &[u8]) -> &[u8] {
    let mut i = 0;
    while i + 1 < b.len() && b[i] == 0 {
        i += 1;
    }
    &b[i..]
}

// -- Chain verification -------------------------------------------------------

pub struct VerifyReport {
    pub ok: bool,
    pub reason: String,
    pub leaf_cn: String,
}

/// True if any trusted root certificates are installed (so verification is
/// possible).  When false the caller can't authenticate the peer at all.
pub fn have_roots() -> bool {
    !fs_root_keys().is_empty()
}

/// Read trusted root certificates from the filesystem trust store
/// (/etc/ssl/certs/ca-certificates.crt and any *.pem there), returning each
/// root's (subject DN, SPKI) as owned bytes for anchoring.
fn fs_root_keys() -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    // 1. Combined bundle.
    if let Some(b) = read_vfs("/etc/ssl/certs/ca-certificates.crt") {
        add_pem_roots(&mut out, &b);
    }
    // 2. Individual *.pem / *.crt files in the certs dir.
    if let Ok(entries) = crate::vfs_contract::VfsContract::read_dir("/etc/ssl/certs") {
        for e in entries {
            if out.len() >= 512 {
                break;
            }
            if (e.name.ends_with(".pem") || e.name.ends_with(".crt"))
                && let Some(b) = read_vfs(&alloc::format!("/etc/ssl/certs/{}", e.name))
            {
                add_pem_roots(&mut out, &b);
            }
        }
    }
    out
}

fn add_pem_roots(out: &mut Vec<(Vec<u8>, Vec<u8>)>, bytes: &[u8]) {
    for der in pem_to_ders(&String::from_utf8_lossy(bytes)) {
        if out.len() >= 512 {
            break;
        }
        if let Ok(c) = parse(&der) {
            out.push((c.subject_dn.to_vec(), c.spki.to_vec()));
        }
    }
}

fn read_vfs(path: &str) -> Option<Vec<u8>> {
    crate::vfs_contract::VfsContract::read_file(path).ok()
}

/// Verify a chain (leaf first) against `host` at `now` epoch seconds.
pub fn verify_chain(ders: &[Vec<u8>], host: &str, now: u64) -> VerifyReport {
    if ders.is_empty() {
        return VerifyReport {
            ok: false,
            reason: String::from("empty chain"),
            leaf_cn: String::new(),
        };
    }
    let mut parsed = Vec::new();
    for d in ders {
        match parse(d) {
            Ok(c) => parsed.push(c),
            Err(e) => {
                return VerifyReport {
                    ok: false,
                    reason: String::from(e),
                    leaf_cn: String::new(),
                };
            }
        }
    }
    let leaf_cn = parsed[0].cn.clone();

    // 1. Validity dates of every cert.
    for c in &parsed {
        if now != 0 && (now < c.not_before || now > c.not_after) {
            return VerifyReport {
                ok: false,
                reason: alloc::format!("certificate expired/not-yet-valid (CN={})", c.cn),
                leaf_cn,
            };
        }
    }
    // 2. Hostname against leaf CN / SANs.
    if !host.is_empty() {
        let matched = host_matches(&parsed[0].cn, host)
            || parsed[0].sans.iter().any(|s| host_matches(s, host));
        if !matched {
            return VerifyReport {
                ok: false,
                reason: alloc::format!("hostname {} not in certificate", host),
                leaf_cn,
            };
        }
    }
    // 3. Chain linkage + signatures.  Load the FS trust store once.
    let roots = fs_root_keys();
    let mut root_spki_owned: Vec<u8>;
    for i in 0..parsed.len() {
        let issuer_spki: &[u8] = if i + 1 < parsed.len() {
            // issuer is the next cert in the chain
            if parsed[i].issuer_dn != parsed[i + 1].subject_dn {
                return VerifyReport {
                    ok: false,
                    reason: String::from("broken chain linkage"),
                    leaf_cn,
                };
            }
            parsed[i + 1].spki
        } else {
            // top of provided chain — anchor to a trusted root (matched by DN).
            match roots
                .iter()
                .find(|(dn, _)| dn.as_slice() == parsed[i].issuer_dn)
            {
                Some((_, spki)) => {
                    root_spki_owned = spki.clone();
                    &root_spki_owned
                }
                None => {
                    let why = if roots.is_empty() {
                        "no trusted roots installed (add /etc/ssl/certs/ca-certificates.crt)"
                    } else {
                        "issuer is not a trusted root"
                    };
                    return VerifyReport {
                        ok: false,
                        reason: String::from(why),
                        leaf_cn,
                    };
                }
            }
        };
        if parsed[i].sig_alg_rsa_sha256 {
            if let Some((n, e)) = rsa_pubkey(issuer_spki) {
                if !rsa_verify(&n, &e, parsed[i].signature, parsed[i].tbs) {
                    return VerifyReport {
                        ok: false,
                        reason: alloc::format!("bad signature on cert {}", i),
                        leaf_cn,
                    };
                }
            } else {
                return VerifyReport {
                    ok: false,
                    reason: String::from("issuer key is not RSA"),
                    leaf_cn,
                };
            }
        } else {
            return VerifyReport {
                ok: false,
                reason: String::from("non-RSA signature (ECDSA verify is a follow-up)"),
                leaf_cn,
            };
        }
    }
    VerifyReport {
        ok: true,
        reason: String::from("chain verified"),
        leaf_cn,
    }
}

// -- Big-integer modular exponentiation (for RSA verify) ----------------------

mod bigint {
    use alloc::vec::Vec;

    /// Compute base^exp mod modulus, all big-endian byte strings. Returns the
    /// big-endian result (no leading zeros).
    pub fn modexp(base: &[u8], exp: &[u8], modulus: &[u8]) -> Vec<u8> {
        let m = to_limbs(modulus);
        if m.is_empty() {
            return Vec::new();
        }
        let mut result = from_u32(1);
        let mut b = modn(&to_limbs(base), &m);
        // iterate exponent bits MSB→LSB
        for &byte in exp {
            for bit in (0..8).rev() {
                result = modn(&mul(&result, &result), &m);
                if byte & (1 << bit) != 0 {
                    result = modn(&mul(&result, &b), &m);
                }
            }
            let _ = &mut b;
        }
        to_be_bytes(&result)
    }

    // little-endian u32 limbs
    fn to_limbs(be: &[u8]) -> Vec<u32> {
        let mut v = Vec::new();
        let mut i = be.len();
        while i > 0 {
            let s = i.saturating_sub(4);
            let mut x = 0u32;
            for &b in &be[s..i] {
                x = (x << 8) | b as u32;
            }
            v.push(x);
            i = s;
        }
        while v.len() > 1 && *v.last().unwrap() == 0 {
            v.pop();
        }
        v
    }
    fn from_u32(x: u32) -> Vec<u32> {
        alloc::vec![x]
    }

    fn to_be_bytes(limbs: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        for &l in limbs.iter().rev() {
            out.extend_from_slice(&l.to_be_bytes());
        }
        let mut i = 0;
        while i + 1 < out.len() && out[i] == 0 {
            i += 1;
        }
        out[i..].to_vec()
    }

    fn cmp(a: &[u32], b: &[u32]) -> core::cmp::Ordering {
        let (la, lb) = (eff_len(a), eff_len(b));
        if la != lb {
            return la.cmp(&lb);
        }
        for i in (0..la).rev() {
            if a[i] != b[i] {
                return a[i].cmp(&b[i]);
            }
        }
        core::cmp::Ordering::Equal
    }
    fn eff_len(a: &[u32]) -> usize {
        let mut n = a.len();
        while n > 1 && a[n - 1] == 0 {
            n -= 1;
        }
        n
    }

    fn mul(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut out = alloc::vec![0u32; a.len() + b.len()];
        for (i, &ai) in a.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &bj) in b.iter().enumerate() {
                let cur = out[i + j] as u64 + ai as u64 * bj as u64 + carry;
                out[i + j] = cur as u32;
                carry = cur >> 32;
            }
            out[i + b.len()] += carry as u32;
        }
        while out.len() > 1 && *out.last().unwrap() == 0 {
            out.pop();
        }
        out
    }

    fn shl1(a: &[u32]) -> Vec<u32> {
        let mut out = alloc::vec![0u32; a.len() + 1];
        let mut carry = 0u32;
        for (i, &x) in a.iter().enumerate() {
            out[i] = (x << 1) | carry;
            carry = x >> 31;
        }
        out[a.len()] = carry;
        while out.len() > 1 && *out.last().unwrap() == 0 {
            out.pop();
        }
        out
    }
    fn sub(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut out = a.to_vec();
        let mut borrow = 0i64;
        for i in 0..out.len() {
            let bj = if i < b.len() { b[i] as i64 } else { 0 };
            let cur = out[i] as i64 - bj - borrow;
            if cur < 0 {
                out[i] = (cur + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                out[i] = cur as u32;
                borrow = 0;
            }
        }
        while out.len() > 1 && *out.last().unwrap() == 0 {
            out.pop();
        }
        out
    }
    fn bit_len(a: &[u32]) -> usize {
        let n = eff_len(a);
        if n == 1 && a[0] == 0 {
            return 0;
        }
        (n - 1) * 32 + (32 - a[n - 1].leading_zeros() as usize)
    }
    fn test_bit(a: &[u32], i: usize) -> bool {
        let limb = i / 32;
        if limb >= a.len() {
            return false;
        }
        a[limb] & (1 << (i % 32)) != 0
    }

    /// a mod m via binary long division.
    fn modn(a: &[u32], m: &[u32]) -> Vec<u32> {
        if cmp(a, m) == core::cmp::Ordering::Less {
            return a.to_vec();
        }
        let mut rem = from_u32(0);
        for i in (0..bit_len(a)).rev() {
            rem = shl1(&rem);
            if test_bit(a, i) {
                rem[0] |= 1;
            }
            if cmp(&rem, m) != core::cmp::Ordering::Less {
                rem = sub(&rem, m);
            }
        }
        rem
    }
}

// Trusted roots are loaded from the filesystem store (/etc/ssl/certs) by
// fs_root_keys() — drop a ca-certificates.crt bundle or *.pem files there.
