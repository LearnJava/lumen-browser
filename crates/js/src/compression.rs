//! Streaming codecs behind `CompressionStream` / `DecompressionStream`
//! (<https://compression.spec.whatwg.org/>).
//!
//! # Why a registry and not a one-shot call
//!
//! The spec's transform algorithm (§4) compresses/decompresses **each chunk**
//! and enqueues whatever the codec produced for it; only the flush algorithm
//! finishes the stream.  The one-shot `bytes -> bytes` natives this module
//! replaced (`_lumen_compress_bytes` / `_lumen_decompress_bytes`, now deleted
//! along with their last caller) could not express that: they had nowhere to
//! keep the codec's half-finished state between chunks, so the shim buffered every
//! chunk and decode at `flush` — i.e. a read before `writer.close()` never
//! resolved (BUG-846).  So the codec itself lives here, keyed by an opaque
//! `u32` handle, exactly like the `CryptoKey` registry in
//! [`crate::subtle_crypto`]: a V8 isolate is single-threaded and each Web
//! Worker runs on its own thread, so `thread_local` gives per-runtime
//! isolation for free.
//!
//! # Why decompression uses two different flate2 APIs
//!
//! A decoder must report three outcomes the JS side has to tell apart: bytes
//! produced, "the input was corrupt or ended early", and "the stream ended and
//! there is junk behind it" (which per WPT's `decompression-extra-input` must
//! still deliver the bytes decoded so far, and only then error the stream).
//!
//! * `deflate` / `deflate-raw` use the low-level [`flate2::Decompress`], which
//!   reports [`flate2::Status::StreamEnd`] explicitly.  The high-level
//!   `write::ZlibDecoder` wrapper cannot: its `finish()` does not distinguish
//!   "the adler32 trailer is missing" from "the stream ended cleanly", so a
//!   truncated input decoded as a success.
//! * `gzip` uses `flate2::write::GzDecoder`, whose `finish()` *does* verify the
//!   trailer (CRC32 + ISIZE, and that all 8 trailer bytes arrived) and whose
//!   `write()` documents `Ok(0)` for input past the end of the member.
//!   Re-deriving the gzip header parse (FEXTRA/FNAME/FCOMMENT/FHCRC) on top of
//!   the low-level API would only risk getting those cases wrong.
//!
//! Compression needs none of this — an encoder cannot fail on its input — so
//! all three formats use the `write::*Encoder` wrappers.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write as _;

/// Status byte prefixed to every byte array this module hands back to JS.
pub(crate) mod status {
    /// The call succeeded; the rest of the array is the produced output.
    pub(crate) const OK: u8 = 1;
    /// The input was corrupt, truncated, or the handle was unknown.
    pub(crate) const ERROR: u8 = 0;
    /// The stream ended and the input carried junk past its end.  The rest of
    /// the array is still valid output and must be delivered *before* the
    /// stream is errored (`decompression-extra-input`).
    pub(crate) const TRAILING_JUNK: u8 = 2;
}

/// How much spare capacity to hand the low-level decoder per pass.
const INFLATE_CHUNK: usize = 16 * 1024;

/// One live codec: an encoder, or a decoder for one of the three formats.
enum Codec {
    /// Raw DEFLATE encoder (RFC 1951).
    EncodeDeflateRaw(flate2::write::DeflateEncoder<Vec<u8>>),
    /// zlib encoder (RFC 1950).
    EncodeDeflate(flate2::write::ZlibEncoder<Vec<u8>>),
    /// gzip encoder (RFC 1952).
    EncodeGzip(flate2::write::GzEncoder<Vec<u8>>),
    /// Raw DEFLATE / zlib decoder — see the module docs for why this one is
    /// the low-level API.
    Inflate(Inflater),
    /// gzip decoder (RFC 1952).
    DecodeGzip(flate2::write::GzDecoder<Vec<u8>>),
}

/// Low-level DEFLATE/zlib decoder plus the bit of state the high-level wrapper
/// hides: whether the compressed stream has already ended.
struct Inflater {
    /// The codec proper.
    inner: flate2::Decompress,
    /// Bytes decoded but not yet handed to JS.
    out: Vec<u8>,
    /// Set once the codec reported [`flate2::Status::StreamEnd`].  Anything
    /// pushed afterwards is junk behind the end of the stream.
    stream_end: bool,
}

thread_local! {
    /// Per-thread registry of live codecs.
    static CODECS: RefCell<HashMap<u32, Codec>> = RefCell::new(HashMap::new());
    /// Monotonic handle allocator; 0 is reserved for "no codec".
    static NEXT_HANDLE: Cell<u32> = const { Cell::new(1) };
}

/// What went wrong while feeding a decoder.
enum FeedError {
    /// The input is not a well-formed stream of this format.
    Corrupt,
    /// The stream ended and the input carried bytes past its end.
    TrailingJunk,
}

/// Creates a codec for `format` and returns its handle, or 0 if the format is
/// not one of `deflate-raw` / `deflate` / `gzip`.
///
/// `decompress` picks the direction: `false` builds an encoder
/// (`CompressionStream`), `true` a decoder (`DecompressionStream`).
pub(crate) fn cs_new(format: &str, decompress: bool) -> u32 {
    let level = flate2::Compression::default();
    let codec = match (format, decompress) {
        ("deflate-raw", false) => {
            Codec::EncodeDeflateRaw(flate2::write::DeflateEncoder::new(Vec::new(), level))
        }
        ("deflate", false) => {
            Codec::EncodeDeflate(flate2::write::ZlibEncoder::new(Vec::new(), level))
        }
        ("gzip", false) => Codec::EncodeGzip(flate2::write::GzEncoder::new(Vec::new(), level)),
        ("deflate-raw", true) => Codec::Inflate(Inflater {
            inner: flate2::Decompress::new(false),
            out: Vec::new(),
            stream_end: false,
        }),
        ("deflate", true) => Codec::Inflate(Inflater {
            inner: flate2::Decompress::new(true),
            out: Vec::new(),
            stream_end: false,
        }),
        ("gzip", true) => Codec::DecodeGzip(flate2::write::GzDecoder::new(Vec::new())),
        _ => return 0,
    };
    let handle = NEXT_HANDLE.with(|c| {
        let v = c.get();
        c.set(v.wrapping_add(1).max(1));
        v
    });
    CODECS.with(|m| m.borrow_mut().insert(handle, codec));
    handle
}

/// Feeds `data` to the codec behind `handle` and returns whatever it produced,
/// prefixed with a [`status`] byte.
///
/// An unknown handle answers [`status::ERROR`] rather than panicking: the shim
/// frees a codec as soon as its stream errors, so a late chunk on an already
/// dead stream is an ordinary outcome, not a bug.
pub(crate) fn cs_push(handle: u32, data: &[u8]) -> Vec<u8> {
    CODECS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(codec) = map.get_mut(&handle) else {
            return vec![status::ERROR];
        };
        let outcome = feed(codec, data);
        let produced = drain(codec);
        match outcome {
            Ok(()) => prefixed(status::OK, produced),
            Err(FeedError::TrailingJunk) => prefixed(status::TRAILING_JUNK, produced),
            Err(FeedError::Corrupt) => {
                map.remove(&handle);
                vec![status::ERROR]
            }
        }
    })
}

/// Finishes the codec behind `handle`, frees it, and returns its final output
/// prefixed with a [`status`] byte.
///
/// [`status::ERROR`] here means the stream ended before the format said it
/// could — a truncated body, or a trailer that does not match the data.
pub(crate) fn cs_finish(handle: u32) -> Vec<u8> {
    let Some(codec) = CODECS.with(|m| m.borrow_mut().remove(&handle)) else {
        return vec![status::ERROR];
    };
    match finish(codec) {
        Ok(bytes) => prefixed(status::OK, bytes),
        Err(()) => vec![status::ERROR],
    }
}

/// Drops the codec behind `handle` without finishing it.  Called by the shim
/// when the stream is errored or terminated, so an abandoned codec does not
/// sit in the registry for the lifetime of the runtime.
pub(crate) fn cs_free(handle: u32) {
    CODECS.with(|m| {
        m.borrow_mut().remove(&handle);
    });
}

/// Prepends `st` to `bytes` without a second allocation.
fn prefixed(st: u8, mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.insert(0, st);
    bytes
}

/// Pushes `data` through `codec`.
fn feed(codec: &mut Codec, data: &[u8]) -> Result<(), FeedError> {
    match codec {
        Codec::EncodeDeflateRaw(e) => e.write_all(data).map_err(|_| FeedError::Corrupt),
        Codec::EncodeDeflate(e) => e.write_all(data).map_err(|_| FeedError::Corrupt),
        Codec::EncodeGzip(e) => e.write_all(data).map_err(|_| FeedError::Corrupt),
        Codec::DecodeGzip(d) => write_all_detecting_end(d, data),
        Codec::Inflate(inf) => inflate(inf, data),
    }
}

/// `write_all`, but reading flate2's documented `Ok(0)` as "the stream ended
/// and this is junk behind it" instead of the generic `WriteZero` error
/// `std::io::Write::write_all` would produce.
fn write_all_detecting_end(w: &mut impl std::io::Write, mut data: &[u8]) -> Result<(), FeedError> {
    while !data.is_empty() {
        match w.write(data) {
            Ok(0) => return Err(FeedError::TrailingJunk),
            Ok(n) => data = &data[n..],
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(FeedError::Corrupt),
        }
    }
    Ok(())
}

/// Runs the low-level DEFLATE/zlib decoder over `data`.
fn inflate(inf: &mut Inflater, data: &[u8]) -> Result<(), FeedError> {
    let mut rest = data;
    loop {
        if inf.stream_end {
            return if rest.is_empty() {
                Ok(())
            } else {
                Err(FeedError::TrailingJunk)
            };
        }
        // `decompress_vec` only ever writes into the vector's spare capacity —
        // it never grows it — so the reserve is what makes progress possible,
        // not an optimisation.
        inf.out.reserve(INFLATE_CHUNK);
        let before_in = inf.inner.total_in();
        let before_out = inf.inner.total_out();
        let status = inf
            .inner
            .decompress_vec(rest, &mut inf.out, flate2::FlushDecompress::None)
            .map_err(|_| FeedError::Corrupt)?;
        let consumed = (inf.inner.total_in() - before_in) as usize;
        let produced = inf.inner.total_out() - before_out;
        rest = &rest[consumed.min(rest.len())..];
        match status {
            flate2::Status::StreamEnd => inf.stream_end = true,
            flate2::Status::Ok | flate2::Status::BufError => {
                // Nothing consumed and nothing produced means the codec wants
                // more input than this chunk carries.
                if rest.is_empty() && produced == 0 {
                    return Ok(());
                }
            }
        }
    }
}

/// Moves flate2's internal staging buffer into the sink.
///
/// `zio::Writer` (which backs every `write::*` wrapper) dumps that buffer at
/// the *start* of the next call, not at the end of the current one, so the
/// bytes a `write` produced are not in the sink `Vec` yet when it returns — an
/// empty write performs the dump and nothing else.  Without this the whole
/// point of BUG-846 is lost for gzip: a full stream pushed in one chunk
/// answered with zero bytes and only surrendered them at `finish`.
fn pump(w: &mut impl std::io::Write) {
    let _ = w.write(&[]);
}

/// Takes whatever the codec has already written to its output buffer.
fn drain(codec: &mut Codec) -> Vec<u8> {
    match codec {
        Codec::EncodeDeflateRaw(e) => {
            pump(e);
            std::mem::take(e.get_mut())
        }
        Codec::EncodeDeflate(e) => {
            pump(e);
            std::mem::take(e.get_mut())
        }
        Codec::EncodeGzip(e) => {
            pump(e);
            std::mem::take(e.get_mut())
        }
        Codec::DecodeGzip(d) => {
            pump(d);
            std::mem::take(d.get_mut())
        }
        // The low-level decoder writes straight into `out`; nothing is staged.
        Codec::Inflate(inf) => std::mem::take(&mut inf.out),
    }
}

/// Ends the stream, returning the trailing output or `Err` if the format says
/// the input was incomplete.
fn finish(codec: Codec) -> Result<Vec<u8>, ()> {
    match codec {
        Codec::EncodeDeflateRaw(e) => e.finish().map_err(|_| ()),
        Codec::EncodeDeflate(e) => e.finish().map_err(|_| ()),
        Codec::EncodeGzip(e) => e.finish().map_err(|_| ()),
        Codec::DecodeGzip(d) => d.finish().map_err(|_| ()),
        // A decoder that never saw the end of its stream was handed a
        // truncated body; the spec errors the stream for that
        // (`decompression-corrupt-input`, "truncating the input should give an
        // error").
        Codec::Inflate(inf) => {
            if inf.stream_end {
                Ok(inf.out)
            } else {
                Err(())
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test module: assertions and fixtures may panic"
)]
mod tests {
    use super::*;

    /// `'expected output'` as zlib (RFC 1950) — WPT's own
    /// `compression/resources/decompression-input.js` vector.
    const DEFLATE: &[u8] = &[
        120, 156, 75, 173, 40, 72, 77, 46, 73, 77, 81, 200, 47, 45, 41, 40, 45, 1, 0, 48, 173, 6,
        36,
    ];
    /// The same string as gzip (RFC 1952).
    const GZIP: &[u8] = &[
        31, 139, 8, 0, 0, 0, 0, 0, 0, 3, 75, 173, 40, 72, 77, 46, 73, 77, 81, 200, 47, 45, 41, 40,
        45, 1, 0, 176, 1, 57, 179, 15, 0, 0, 0,
    ];
    /// The same string as raw DEFLATE (RFC 1951).
    const DEFLATE_RAW: &[u8] = &[
        0x4b, 0xad, 0x28, 0x48, 0x4d, 0x2e, 0x49, 0x4d, 0x51, 0xc8, 0x2f, 0x2d, 0x29, 0x28, 0x2d,
        0x01, 0x00,
    ];
    const EXPECTED: &[u8] = b"expected output";

    fn vectors() -> [(&'static str, &'static [u8]); 3] {
        [
            ("deflate", DEFLATE),
            ("gzip", GZIP),
            ("deflate-raw", DEFLATE_RAW),
        ]
    }

    /// The whole point of BUG-846: the decoded bytes are available from the
    /// push itself, with no `cs_finish` (i.e. no `writer.close()`) in sight.
    #[test]
    fn decoder_emits_before_finish() {
        for (format, input) in vectors() {
            let h = cs_new(format, true);
            let out = cs_push(h, input);
            assert_eq!(out[0], status::OK, "{format}: push status");
            assert_eq!(&out[1..], EXPECTED, "{format}: output before finish");
            // Whatever finish adds must not repeat what push already produced.
            let tail = cs_finish(h);
            assert_eq!(tail, vec![status::OK], "{format}: nothing left at finish");
        }
    }

    /// Byte-at-a-time feeding is what `decompression-split-chunk` does; the
    /// codec has to hold its state across the pushes.
    #[test]
    fn decoder_accepts_one_byte_chunks() {
        for (format, input) in vectors() {
            let h = cs_new(format, true);
            let mut got = Vec::new();
            for b in input {
                let out = cs_push(h, &[*b]);
                assert_eq!(out[0], status::OK, "{format}: mid-stream status");
                got.extend_from_slice(&out[1..]);
            }
            let tail = cs_finish(h);
            assert_eq!(tail[0], status::OK, "{format}: finish status");
            got.extend_from_slice(&tail[1..]);
            assert_eq!(got, EXPECTED, "{format}: reassembled output");
        }
    }

    /// `decompression-extra-input`: the bytes decoded so far must come back
    /// *with* the junk verdict, not instead of it — erroring without them
    /// would lose the chunk the page is owed.
    #[test]
    fn trailing_junk_reports_output_and_the_error() {
        for (format, input) in vectors() {
            let mut padded = input.to_vec();
            padded.push(0);
            let h = cs_new(format, true);
            let out = cs_push(h, &padded);
            assert_eq!(out[0], status::TRAILING_JUNK, "{format}: junk status");
            assert_eq!(&out[1..], EXPECTED, "{format}: output despite junk");
            cs_free(h);
        }
    }

    /// `decompression-corrupt-input`: a body that stops one byte short of its
    /// own trailer is an error, not a successful decode. The buffer-then-flush
    /// model reported success for exactly this input.
    #[test]
    fn truncated_input_errors_at_finish() {
        for (format, input) in vectors() {
            let h = cs_new(format, true);
            let out = cs_push(h, &input[..input.len() - 1]);
            assert_eq!(out[0], status::OK, "{format}: truncated push is not yet an error");
            assert_eq!(
                cs_finish(h),
                vec![status::ERROR],
                "{format}: truncation must surface at finish"
            );
        }
    }

    /// The zlib trailer is a checksum, so a flipped data byte must be caught
    /// even though the DEFLATE block itself still decodes.
    #[test]
    fn corrupt_payload_errors() {
        let mut corrupt = DEFLATE.to_vec();
        corrupt[18] = 5; // WPT's `field DATA should be error for 5`
        let h = cs_new("deflate", true);
        let push = cs_push(h, &corrupt);
        let verdict = if push[0] == status::ERROR {
            status::ERROR
        } else {
            cs_finish(h)[0]
        };
        assert_eq!(verdict, status::ERROR, "corrupt payload must error");
    }

    /// A well-formed stream may legitimately decode to nothing; that is a
    /// success, not the "no output" of a failure.
    #[test]
    fn empty_stream_decodes_to_nothing() {
        // `decompression-empty-input`'s own vectors.
        for (format, input) in [
            ("gzip", &[31u8, 139, 8, 0, 0, 0, 0, 0, 0, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0][..]),
            ("deflate", &[120, 156, 3, 0, 0, 0, 0, 1][..]),
            ("deflate-raw", &[1, 0, 0, 255, 255][..]),
        ] {
            let h = cs_new(format, true);
            let out = cs_push(h, input);
            assert_eq!(out[0], status::OK, "{format}: empty-stream push");
            let tail = cs_finish(h);
            assert_eq!(tail, vec![status::OK], "{format}: empty-stream finish");
            assert_eq!(out.len(), 1, "{format}: no bytes produced");
        }
    }

    /// Encoders keep their state across chunks too — `compression-multiple-chunks`
    /// writes N chunks and expects one concatenated payload back.
    #[test]
    fn encoder_round_trips_multiple_chunks() {
        for format in ["deflate", "gzip", "deflate-raw"] {
            let enc = cs_new(format, false);
            let mut compressed = Vec::new();
            for _ in 0..4 {
                let out = cs_push(enc, b"Hello");
                assert_eq!(out[0], status::OK, "{format}: encode push");
                compressed.extend_from_slice(&out[1..]);
            }
            let tail = cs_finish(enc);
            assert_eq!(tail[0], status::OK, "{format}: encode finish");
            compressed.extend_from_slice(&tail[1..]);

            let dec = cs_new(format, true);
            let out = cs_push(dec, &compressed);
            assert_eq!(out[0], status::OK, "{format}: decode push");
            let mut got = out[1..].to_vec();
            let tail = cs_finish(dec);
            assert_eq!(tail[0], status::OK, "{format}: decode finish");
            got.extend_from_slice(&tail[1..]);
            assert_eq!(got, b"HelloHelloHelloHello", "{format}: round trip");
        }
    }

    /// An unknown format allocates nothing, and a freed handle answers with an
    /// error rather than resurrecting a codec.
    #[test]
    fn unknown_format_and_dead_handle() {
        assert_eq!(cs_new("brotli", true), 0);
        assert_eq!(cs_new("brotli", false), 0);
        let h = cs_new("gzip", true);
        cs_free(h);
        assert_eq!(cs_push(h, GZIP), vec![status::ERROR]);
        assert_eq!(cs_finish(h), vec![status::ERROR]);
    }
}
