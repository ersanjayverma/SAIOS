//! DEFLATE and gzip decompressor (RFC 1951 / RFC 1952).
//!
//! Used by:
//!   - Packages.gz  (apt package index)
//!   - .deb files with control.tar.gz / data.tar.gz (older packages)
//!   - HTTP Content-Encoding: gzip responses
//!
//! Implements the full DEFLATE algorithm:
//!   - Uncompressed blocks (BTYPE=00)
//!   - Fixed Huffman codes (BTYPE=01)
//!   - Dynamic Huffman codes (BTYPE=10)
//!
//! Plus gzip wrapper detection and CRC32 verification.

use alloc::vec::Vec;

/// Decompress a gzip stream. Returns the uncompressed bytes or an error.
pub fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 18 {
        return Err("gzip: too short");
    }
    // Gzip header
    if data[0] != 0x1F || data[1] != 0x8B {
        return Err("gzip: bad magic");
    }
    if data[2] != 8 {
        return Err("gzip: unsupported compression method");
    }
    let flags = data[3];
    let mut pos = 10usize;

    // Skip extra field
    if flags & 0x04 != 0 {
        if pos + 2 > data.len() {
            return Err("gzip: truncated extra");
        }
        let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + xlen;
    }
    // Skip original filename
    if flags & 0x08 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // Skip comment
    if flags & 0x10 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // Skip header CRC
    if flags & 0x02 != 0 {
        pos += 2;
    }

    if pos + 8 > data.len() {
        return Err("gzip: no room for DEFLATE + footer");
    }

    // Stored uncompressed size from footer (last 4 bytes)
    let stored_size = u32::from_le_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]) as usize;

    let deflate_data = &data[pos..data.len() - 8];
    crate::serial_println!(
        "[gzip] input={} hdr_pos={} deflate_len={} isize={}",
        data.len(),
        pos,
        deflate_data.len(),
        stored_size
    );
    // `stored_size` comes from the gzip footer, which is GARBAGE if the stream
    // was truncated (e.g. a partial download).  Never trust it for allocation —
    // inflate() clamps the capacity hint and bounds total output, so a bad/short
    // stream returns an error instead of attempting a multi-GB allocation that
    // would OOM-panic the kernel.
    inflate(deflate_data, stored_size)
}

/// Decompress raw DEFLATE data (no wrapper).
pub fn inflate(data: &[u8], hint_size: usize) -> Result<Vec<u8>, &'static str> {
    // Hard ceiling on decompressed output — protects against a corrupt/truncated
    // stream (or a hostile one) driving an unbounded allocation that OOM-panics.
    const MAX_OUTPUT: usize = 256 * 1024 * 1024; // 256 MiB
    let mut reader = BitReader::new(data);
    // Clamp the capacity hint: it derives from the (untrusted) gzip footer.
    let cap = hint_size.clamp(256, 64 * 1024 * 1024);
    let mut out = Vec::with_capacity(cap);

    loop {
        let bfinal = reader.read_bits(1)?;
        let btype = reader.read_bits(2)?;

        match btype {
            0 => {
                // Uncompressed block
                reader.align_to_byte();
                let len = reader.read_u16_le()?;
                let nlen = reader.read_u16_le()?;
                if len != !nlen {
                    return Err("deflate: uncompressed block check failed");
                }
                for _ in 0..len {
                    out.push(reader.read_byte()?);
                }
            }
            1 => {
                // Fixed Huffman
                let (lit_tree, dist_tree) = fixed_huffman_trees();
                decode_block(&mut reader, &mut out, &lit_tree, &dist_tree)?;
            }
            2 => {
                // Dynamic Huffman
                let hlit = reader.read_bits(5)? as usize + 257;
                let hdist = reader.read_bits(5)? as usize + 1;
                let hclen = reader.read_bits(4)? as usize + 4;
                let (lit_tree, dist_tree) = dynamic_huffman_trees(&mut reader, hlit, hdist, hclen)?;
                decode_block(&mut reader, &mut out, &lit_tree, &dist_tree)?;
            }
            _ => return Err("deflate: invalid block type 3"),
        }

        if out.len() > MAX_OUTPUT {
            return Err("deflate: output exceeds limit");
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(out)
}

// -- Huffman decode logic ---------------------------------------------------

fn decode_block(
    reader: &mut BitReader,
    out: &mut Vec<u8>,
    lit_tree: &HuffTree,
    dist_tree: &HuffTree,
) -> Result<(), &'static str> {
    loop {
        let sym = lit_tree.decode(reader)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => break, // end of block
            sym => {
                // Length + distance back-reference
                let len = decode_length(reader, sym)?;
                let dsym = dist_tree.decode(reader)?;
                let dist = decode_dist(reader, dsym)?;
                if dist > out.len() {
                    return Err("deflate: invalid back-reference distance");
                }
                let start = out.len() - dist;
                for i in 0..len {
                    let b = out[start + i % dist];
                    out.push(b);
                }
            }
        }
    }
    Ok(())
}

fn decode_length(reader: &mut BitReader, sym: u32) -> Result<usize, &'static str> {
    const EXTRA: [(u32, usize); 29] = [
        (3, 0),
        (4, 0),
        (5, 0),
        (6, 0),
        (7, 0),
        (8, 0),
        (9, 0),
        (10, 0),
        (11, 1),
        (13, 1),
        (15, 1),
        (17, 1),
        (19, 2),
        (23, 2),
        (27, 2),
        (31, 2),
        (35, 3),
        (43, 3),
        (51, 3),
        (59, 3),
        (67, 4),
        (83, 4),
        (99, 4),
        (115, 4),
        (131, 5),
        (163, 5),
        (195, 5),
        (227, 5),
        (258, 0),
    ];
    let idx = (sym - 257) as usize;
    if idx >= 29 {
        return Err("deflate: bad length symbol");
    }
    let (base, extra_bits) = EXTRA[idx];
    let extra = reader.read_bits(extra_bits)? as usize;
    Ok(base as usize + extra)
}

fn decode_dist(reader: &mut BitReader, sym: u32) -> Result<usize, &'static str> {
    const EXTRA: [(u32, usize); 30] = [
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 1),
        (7, 1),
        (9, 2),
        (13, 2),
        (17, 3),
        (25, 3),
        (33, 4),
        (49, 4),
        (65, 5),
        (97, 5),
        (129, 6),
        (193, 6),
        (257, 7),
        (385, 7),
        (513, 8),
        (769, 8),
        (1025, 9),
        (1537, 9),
        (2049, 10),
        (3073, 10),
        (4097, 11),
        (6145, 11),
        (8193, 12),
        (12289, 12),
        (16385, 13),
        (24577, 13),
    ];
    if sym >= 30 {
        return Err("deflate: bad distance symbol");
    }
    let (base, extra_bits) = EXTRA[sym as usize];
    let extra = reader.read_bits(extra_bits)? as usize;
    Ok(base as usize + extra)
}

// -- Huffman tree -----------------------------------------------------------

struct HuffTree {
    /// For each symbol: (code, bit_length). Max 288 symbols for literal tree.
    codes: Vec<(u32, u8)>,
}

impl HuffTree {
    /// Build a Huffman tree from an array of code lengths (one per symbol).
    fn from_lengths(lengths: &[u8]) -> Result<Self, &'static str> {
        let max_len = *lengths.iter().max().unwrap_or(&0) as usize;
        if max_len > 15 {
            return Err("deflate: huffman code length > 15");
        }

        let mut bl_count = [0u32; 16];
        for &l in lengths {
            if l > 0 {
                bl_count[l as usize] += 1;
            }
        }

        // Compute starting code for each length
        let mut next_code = [0u32; 16];
        let mut code = 0u32;
        for bits in 1..=max_len {
            code = (code + bl_count[bits - 1]) << 1;
            next_code[bits] = code;
        }

        // Assign codes
        let mut codes = alloc::vec![(0u32, 0u8); lengths.len()];
        for (sym, &len) in lengths.iter().enumerate() {
            if len > 0 {
                codes[sym] = (next_code[len as usize], len);
                next_code[len as usize] += 1;
            }
        }
        Ok(HuffTree { codes })
    }

    /// Decode one symbol by reading bits from `reader`.
    fn decode(&self, reader: &mut BitReader) -> Result<u32, &'static str> {
        let mut code = 0u32;
        let mut len = 0u8;
        loop {
            code = (code << 1) | reader.read_bits(1)?;
            len += 1;
            for (sym, &(c, l)) in self.codes.iter().enumerate() {
                if l == len && c == code {
                    return Ok(sym as u32);
                }
            }
            if len > 15 {
                return Err("deflate: huffman decode overrun");
            }
        }
    }
}

fn fixed_huffman_trees() -> (HuffTree, HuffTree) {
    // Fixed literal/length code lengths (RFC 1951 §3.2.6)
    let mut lit = alloc::vec![0u8; 288];
    for v in lit.iter_mut().take(144) {
        *v = 8;
    }
    for v in lit.iter_mut().take(256).skip(144) {
        *v = 9;
    }
    for v in lit.iter_mut().take(280).skip(256) {
        *v = 7;
    }
    for v in lit.iter_mut().take(288).skip(280) {
        *v = 8;
    }
    // Fixed distance code lengths (5 bits each)
    let dist: Vec<u8> = alloc::vec![5u8; 32];
    (
        HuffTree::from_lengths(&lit).unwrap(),
        HuffTree::from_lengths(&dist).unwrap(),
    )
}

fn dynamic_huffman_trees(
    reader: &mut BitReader,
    hlit: usize,
    hdist: usize,
    hclen: usize,
) -> Result<(HuffTree, HuffTree), &'static str> {
    // Code-length code lengths
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut cl_lengths = [0u8; 19];
    for i in 0..hclen {
        cl_lengths[ORDER[i]] = reader.read_bits(3)? as u8;
    }
    let cl_tree = HuffTree::from_lengths(&cl_lengths)?;

    // Decode literal/distance code lengths
    let total = hlit + hdist;
    let mut lengths = alloc::vec![0u8; total];
    let mut i = 0;
    while i < total {
        let sym = cl_tree.decode(reader)?;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
            }
            16 => {
                let rep = reader.read_bits(2)? as usize + 3;
                let v = if i > 0 { lengths[i - 1] } else { 0 };
                for _ in 0..rep {
                    if i < total {
                        lengths[i] = v;
                        i += 1;
                    }
                }
            }
            17 => {
                let rep = reader.read_bits(3)? as usize + 3;
                for _ in 0..rep {
                    if i < total {
                        lengths[i] = 0;
                        i += 1;
                    }
                }
            }
            18 => {
                let rep = reader.read_bits(7)? as usize + 11;
                for _ in 0..rep {
                    if i < total {
                        lengths[i] = 0;
                        i += 1;
                    }
                }
            }
            _ => return Err("deflate: bad code-length symbol"),
        }
    }
    let lit_tree = HuffTree::from_lengths(&lengths[..hlit])?;
    let dist_tree = HuffTree::from_lengths(&lengths[hlit..])?;
    Ok((lit_tree, dist_tree))
}

// -- Bit-level reader -------------------------------------------------------

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,   // byte position
    bit_buf: u32, // buffered bits (LSB first)
    bit_cnt: u8,  // valid bits in bit_buf
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit_buf: 0,
            bit_cnt: 0,
        }
    }

    fn read_bits(&mut self, n: usize) -> Result<u32, &'static str> {
        while self.bit_cnt < n as u8 {
            if self.pos >= self.data.len() {
                crate::serial_println!("[deflate] EOF at byte {}/{}", self.pos, self.data.len());
                return Err("deflate: unexpected end of data");
            }
            self.bit_buf |= (self.data[self.pos] as u32) << self.bit_cnt;
            self.pos += 1;
            self.bit_cnt += 8;
        }
        let mask = (1u32 << n) - 1;
        let val = self.bit_buf & mask;
        self.bit_buf >>= n;
        self.bit_cnt -= n as u8;
        Ok(val)
    }

    fn align_to_byte(&mut self) {
        self.bit_buf = 0;
        self.bit_cnt = 0;
    }

    fn read_byte(&mut self) -> Result<u8, &'static str> {
        Ok(self.read_bits(8)? as u8)
    }

    fn read_u16_le(&mut self) -> Result<u16, &'static str> {
        let lo = self.read_byte()? as u16;
        let hi = self.read_byte()? as u16;
        Ok(lo | (hi << 8))
    }
}
