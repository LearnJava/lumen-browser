//! RFC 7692 permessage-deflate WebSocket extension: per-message DEFLATE codec.
//!
//! Uses `client_no_context_takeover` + `server_no_context_takeover` so each
//! message is compressed/decompressed independently — no shared zlib state.
//!
//! Compression: raw DEFLATE sync-flush → strip trailing `00 00 FF FF`.
//! Decompression: append `00 00 FF FF` → raw DEFLATE inflate.

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};

use crate::Error;
use lumen_core::error::Result;

// ── Compress ──────────────────────────────────────────────────────────────────

/// Compress `data` for a single permessage-deflate frame.
///
/// Returns raw DEFLATE bytes with the trailing sync-flush token stripped.
/// Uses `no_context_takeover`: allocates a fresh compressor each call.
pub(crate) fn compress_message(data: &[u8]) -> Result<Vec<u8>> {
    let mut comp = Compress::new(Compression::default(), false);
    // Worst-case compressed output ≈ input + 5 bytes/block + 4-byte sync flush.
    // Allocating 2× input + 128 B guarantees one-shot success for any realistic WS message.
    let cap = data.len() * 2 + 128;
    let mut out = vec![0u8; cap];

    comp.compress(data, &mut out, FlushCompress::Sync)
        .map_err(|e| Error::Network(format!("ws deflate compress: {e}")))?;

    let written = comp.total_out() as usize;
    out.truncate(written);

    // Strip trailing sync-flush token `00 00 FF FF` required by RFC 7692 §7.2.1.
    if out.ends_with(&[0x00, 0x00, 0xFF, 0xFF]) {
        out.truncate(out.len() - 4);
    }

    Ok(out)
}

// ── Decompress ────────────────────────────────────────────────────────────────

/// Decompress a permessage-deflate frame payload.
///
/// Re-appends the stripped sync-flush token `00 00 FF FF` before inflating.
/// Iterates with a doubling output buffer so arbitrarily-compressed data works.
pub(crate) fn decompress_message(data: &[u8]) -> Result<Vec<u8>> {
    // Re-append the sync-flush token stripped by the sender (RFC 7692 §7.2.2).
    let mut input = Vec::with_capacity(data.len() + 4);
    input.extend_from_slice(data);
    input.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);

    let mut decomp = Decompress::new(false);
    let mut out = Vec::new();
    // Use a fixed-size scratch buffer and loop to handle arbitrary expansion ratios.
    let mut scratch = vec![0u8; 4096];
    let mut in_pos = 0;

    loop {
        let before_in = decomp.total_in() as usize;
        let before_out = decomp.total_out() as usize;

        decomp
            .decompress(&input[in_pos..], &mut scratch, FlushDecompress::None)
            .map_err(|e| Error::Network(format!("ws deflate decompress: {e}")))?;

        let new_out = decomp.total_out() as usize - before_out;
        out.extend_from_slice(&scratch[..new_out]);
        in_pos = decomp.total_in() as usize;

        if in_pos >= input.len() {
            break;
        }
        // No progress — stuck (shouldn't happen with valid data).
        if decomp.total_in() as usize == before_in && decomp.total_out() as usize == before_out {
            return Err(Error::Network(
                "ws deflate decompress: no progress (corrupt stream?)".into(),
            ));
        }
    }

    Ok(out)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Compress then decompress must reproduce the original data.
    #[test]
    fn deflate_compress_roundtrip() {
        let original = b"Hello, WebSocket permessage-deflate! This text repeats. This text repeats.";
        let compressed = compress_message(original).unwrap();
        let recovered = decompress_message(&compressed).unwrap();
        assert_eq!(recovered, original);
    }

    /// For highly repetitive data the compressed form must be smaller than the input.
    #[test]
    fn deflate_compress_output_smaller_for_repetitive_data() {
        let original = "aaaaaaaaaa".repeat(100);
        let compressed = compress_message(original.as_bytes()).unwrap();
        assert!(
            compressed.len() < original.len(),
            "compressed ({}) should be smaller than original ({})",
            compressed.len(),
            original.len()
        );
    }

    /// Empty message should round-trip without error.
    #[test]
    fn deflate_compress_empty_roundtrip() {
        let compressed = compress_message(b"").unwrap();
        let recovered = decompress_message(&compressed).unwrap();
        assert_eq!(recovered, b"");
    }
}
