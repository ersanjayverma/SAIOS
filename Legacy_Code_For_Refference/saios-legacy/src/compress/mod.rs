//! Compression/decompression library for SAIOS.
//!
//! Supports the formats needed by the package manager:
//!   gzip  (.gz)  — apt package index (Packages.gz), older .deb
//!   xz    (.xz)  — modern Debian packages (data.tar.xz, control.tar.xz)
//!   zstd  (.zst) — future package payload support gated by CompatibilityContract

pub mod deflate;
pub mod lzma;

use alloc::vec::Vec;

/// Auto-detect and decompress based on magic bytes.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 6 {
        return Err("compress: too short to detect format");
    }

    if data[0] == 0x1F && data[1] == 0x8B {
        // gzip magic
        return deflate::gzip_decompress(data);
    }

    if &data[0..6] == b"\xfd7zXZ\0" {
        // xz magic
        return lzma::xz_decompress(data);
    }

    if data[0] == 0x28 && data[1] == 0xB5 && data[2] == 0x2F && data[3] == 0xFD {
        let _ = crate::compatibility_contract::CompatibilityContract::require_placeholder_available(
            "compress.zstd",
        );
        return Err("compress: zstd placeholder gated by compatibility roadmap");
    }

    // Not compressed — return as-is
    Ok(data.to_vec())
}

/// Detect whether data is compressed (by magic bytes).
pub fn is_compressed(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    (data[0] == 0x1F && data[1] == 0x8B) ||          // gzip
    (data.len() >= 6 && &data[0..6] == b"\xfd7zXZ\0") || // xz
    (data[0] == 0x28 && data[1] == 0xB5) // zstd
}
