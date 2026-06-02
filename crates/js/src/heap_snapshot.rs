//! Deflate compression + 5 MB cap for QuickJS heap snapshots (ADR-008 §10C.3).
//!
//! When a tab transitions to T3 (Hibernated) its JS runtime is suspended into a
//! [`SuspendedHeap`]. The raw heap payload (produced by the engine serializer —
//! task 10C.2) is deflate-compressed here and capped at
//! [`MAX_HEAP_SNAPSHOT_BYTES`] so a hibernated tab's on-disk JS state stays
//! small. Heap payloads are string/bytecode-heavy and shrink well under deflate,
//! directly serving the ADR-008 RAM/disk budget (50 tabs ~400 MB vs Chrome
//! 6-10 GB).
//!
//! **Why deflate, not zstd:** reuses the already-vendored `flate2` (PNG iCCP
//! path, also used by `lumen-storage` for DOM-blob compression in §10J.1) — no
//! new external dependency. The [`SuspendedHeap::compressed`] field name and the
//! trait docs say "zstd" aspirationally; the on-disk format is opaque to the
//! `lumen-core` type and the 4-byte [`HEAP_MAGIC`] prefix lets the format evolve
//! (e.g. switch to zstd) without breaking older snapshots.

use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use lumen_core::SuspendedHeap;

/// Per-tab cap on the compressed heap snapshot (ADR-008 §10C.3: "cap 5 MB/tab
/// disk"). [`compress_heap`] refuses to produce a snapshot larger than this so a
/// single runaway tab cannot blow the hibernation disk budget; the caller then
/// falls back to script re-execution on restore.
pub const MAX_HEAP_SNAPSHOT_BYTES: usize = 5 * 1024 * 1024;

/// Magic prefix tagging a deflate-compressed heap snapshot.
///
/// [`compress_heap`] prepends these 4 bytes before the zlib stream so
/// [`decompress_heap`] can tell a compressed snapshot from a raw/legacy one and
/// pick the right path. "LJH1" = **L**umen **J**s **H**eap, format version **1**.
const HEAP_MAGIC: [u8; 4] = *b"LJH1";

/// Error from the heap-snapshot compression layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapSnapshotError {
    /// The compressed snapshot exceeds [`MAX_HEAP_SNAPSHOT_BYTES`]; the heap is
    /// not persisted and the tab must re-run scripts on restore. Carries the
    /// would-be compressed size and the cap for diagnostics.
    TooLarge {
        /// Size in bytes the compressed snapshot would have occupied.
        compressed: usize,
        /// The enforced cap ([`MAX_HEAP_SNAPSHOT_BYTES`]).
        cap: usize,
    },
    /// Decompression failed — the stream is corrupt or truncated.
    Decode(String),
}

impl std::fmt::Display for HeapSnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { compressed, cap } => {
                write!(f, "heap snapshot too large: {compressed} B > {cap} B cap")
            }
            Self::Decode(msg) => write!(f, "heap snapshot decode: {msg}"),
        }
    }
}

impl std::error::Error for HeapSnapshotError {}

/// Compress a raw heap payload into a [`SuspendedHeap`].
///
/// Produces [`HEAP_MAGIC`] followed by a zlib (deflate) stream of `raw`. Returns
/// [`HeapSnapshotError::TooLarge`] when the compressed result would exceed
/// [`MAX_HEAP_SNAPSHOT_BYTES`] — the caller then skips heap persistence so a
/// runaway tab cannot exhaust the disk budget. An empty `raw` is valid and
/// round-trips to an empty `Vec` via [`decompress_heap`].
pub fn compress_heap(raw: &[u8]) -> Result<SuspendedHeap, HeapSnapshotError> {
    let mut out = Vec::with_capacity(raw.len() / 3 + HEAP_MAGIC.len());
    out.extend_from_slice(&HEAP_MAGIC);
    let mut encoder = ZlibEncoder::new(out, Compression::default());
    encoder
        .write_all(raw)
        .map_err(|e| HeapSnapshotError::Decode(e.to_string()))?;
    let buf = encoder
        .finish()
        .map_err(|e| HeapSnapshotError::Decode(e.to_string()))?;
    if buf.len() > MAX_HEAP_SNAPSHOT_BYTES {
        return Err(HeapSnapshotError::TooLarge {
            compressed: buf.len(),
            cap: MAX_HEAP_SNAPSHOT_BYTES,
        });
    }
    Ok(SuspendedHeap::new(buf))
}

/// Inverse of [`compress_heap`]: strip the [`HEAP_MAGIC`] prefix and inflate.
///
/// A snapshot whose bytes do not begin with [`HEAP_MAGIC`] is returned verbatim
/// (a raw/legacy payload). An empty snapshot decodes to an empty `Vec`.
pub fn decompress_heap(heap: &SuspendedHeap) -> Result<Vec<u8>, HeapSnapshotError> {
    let bytes = &heap.compressed;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.len() < HEAP_MAGIC.len() || bytes[..HEAP_MAGIC.len()] != HEAP_MAGIC {
        // Raw / legacy payload — no compression applied.
        return Ok(bytes.clone());
    }
    let mut decoder = ZlibDecoder::new(&bytes[HEAP_MAGIC.len()..]);
    let mut raw = Vec::new();
    decoder
        .read_to_end(&mut raw)
        .map_err(|e| HeapSnapshotError::Decode(e.to_string()))?;
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_simple() {
        let raw = b"function f(){ return 42; } var state = { count: 7 };";
        let heap = compress_heap(raw).unwrap();
        assert_eq!(decompress_heap(&heap).unwrap(), raw);
    }

    #[test]
    fn roundtrip_empty() {
        let heap = compress_heap(b"").unwrap();
        assert!(decompress_heap(&heap).unwrap().is_empty());
    }

    #[test]
    fn compressed_has_magic_prefix() {
        let heap = compress_heap(b"hello heap").unwrap();
        assert_eq!(&heap.compressed[..4], &HEAP_MAGIC);
    }

    #[test]
    fn highly_repetitive_payload_shrinks() {
        // Heap snapshots repeat tag/identifier strings — deflate should shrink them.
        let raw = "globalThis.__state__=".repeat(4096).into_bytes();
        let heap = compress_heap(&raw).unwrap();
        assert!(
            heap.len() < raw.len() / 4,
            "expected >4x shrink, got {} -> {}",
            raw.len(),
            heap.len()
        );
        assert_eq!(decompress_heap(&heap).unwrap(), raw);
    }

    #[test]
    fn binary_payload_roundtrips() {
        let raw: Vec<u8> = (0u32..2048).map(|i| (i % 256) as u8).collect();
        let heap = compress_heap(&raw).unwrap();
        assert_eq!(decompress_heap(&heap).unwrap(), raw);
    }

    #[test]
    fn cap_rejects_incompressible_oversized_payload() {
        // Pseudo-random bytes barely compress, so >5 MB raw stays >5 MB compressed.
        let mut raw = vec![0u8; MAX_HEAP_SNAPSHOT_BYTES + 1024];
        let mut x: u32 = 0x1234_5678;
        for b in &mut raw {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = (x & 0xff) as u8;
        }
        match compress_heap(&raw) {
            Err(HeapSnapshotError::TooLarge { compressed, cap }) => {
                assert_eq!(cap, MAX_HEAP_SNAPSHOT_BYTES);
                assert!(compressed > cap);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[test]
    fn large_but_compressible_payload_fits_under_cap() {
        // 20 MB of repeats compresses far below the 5 MB cap.
        let raw = vec![0xABu8; 20 * 1024 * 1024];
        let heap = compress_heap(&raw).unwrap();
        assert!(heap.len() <= MAX_HEAP_SNAPSHOT_BYTES);
        assert_eq!(decompress_heap(&heap).unwrap(), raw);
    }

    #[test]
    fn legacy_payload_without_magic_passthrough() {
        // A snapshot written before 10C.3 (raw bytes, no magic) reads verbatim.
        let raw = vec![0x00, 0x01, 0x02, 0x03];
        let legacy = SuspendedHeap::new(raw.clone());
        assert_eq!(decompress_heap(&legacy).unwrap(), raw);
    }

    #[test]
    fn corrupt_stream_errors() {
        let mut bytes = HEAP_MAGIC.to_vec();
        bytes.extend_from_slice(b"not a valid zlib stream");
        let heap = SuspendedHeap::new(bytes);
        assert!(matches!(
            decompress_heap(&heap),
            Err(HeapSnapshotError::Decode(_))
        ));
    }

    #[test]
    fn error_display_messages() {
        let too_large = HeapSnapshotError::TooLarge { compressed: 10, cap: 5 };
        assert!(too_large.to_string().contains("too large"));
        let decode = HeapSnapshotError::Decode("boom".into());
        assert!(decode.to_string().contains("boom"));
    }
}
