//! QPACK field-section codec — RFC 9204 (static table only).
//!
//! QPACK is HTTP/3's header-compression format (the analogue of HPACK for
//! HTTP/2). It has two halves: the *field-section* format carried inside
//! HEADERS / PUSH_PROMISE frames (RFC 9204 §4.5), and the *encoder/decoder
//! streams* that mutate a shared dynamic table (RFC 9204 §4.3–§4.4). This
//! module implements the field-section format restricted to the **static
//! table**: no dynamic-table references are emitted, and any received
//! reference to the dynamic table is rejected.
//!
//! That restriction is exactly the wire behaviour of a peer that advertises
//! `SETTINGS_QPACK_MAX_TABLE_CAPACITY = 0` (RFC 9204 §3.2.3): with a zero-size
//! dynamic table the encoder MUST NOT reference it, so a decoder can be fully
//! spec-compliant while only understanding static + literal representations.
//! It is also *blocking-free* — the Required Insert Count is always 0, so a
//! field section never waits on encoder-stream inserts (RFC 9204 §2.1.2).
//!
//! ## Shared primitives
//!
//! RFC 9204 §4.1.1 defines its prefixed integer as "the prefixed integer from
//! Section 5.1 of [HPACK]", and §4.1.2 reuses the Huffman code from
//! [HPACK]'s Appendix B. Those primitives already live in
//! [`crate::h2::hpack`]; this module reuses them rather than duplicating the
//! 257-entry Huffman table.
//!
//! ## Scope
//!
//! Implemented (RFC 9204 §4.5):
//! - Encoded Field Section Prefix (§4.5.1) — always `Required Insert Count = 0`,
//!   `Base = 0` on encode; parsed and required to be 0 on decode.
//! - Indexed Field Line, static (§4.5.2, `T = 1`).
//! - Literal Field Line With Name Reference, static (§4.5.4, `T = 1`).
//! - Literal Field Line With Literal Name (§4.5.6).
//!
//! Rejected as [`QpackError::DynamicUnsupported`] (a peer honouring our zero
//! table capacity never sends these):
//! - Indexed Field Line into the dynamic table (§4.5.2, `T = 0`).
//! - Indexed Field Line With Post-Base Index (§4.5.3).
//! - Literal Field Line With Name Reference into the dynamic table (§4.5.4,
//!   `T = 0`).
//! - Literal Field Line With Post-Base Name Reference (§4.5.5).
//! - Any non-zero Required Insert Count in the section prefix (§4.5.1.1).
//!
//! ## Out of scope (deferred to later slices)
//!
//! - The dynamic table and the encoder/decoder instruction streams
//!   (RFC 9204 §4.3–§4.4).
//! - IO, stream framing, and wiring into the request path.

use crate::h2::hpack::{HpackError, decode_int, encode_int, huffman_decode, huffman_encode};

// ── Error ─────────────────────────────────────────────────────────────────

/// QPACK_DECOMPRESSION_FAILED (RFC 9204 §6) — the field section cannot be
/// decoded. It is a connection error on the HTTP/3 connection.
pub const QPACK_DECOMPRESSION_FAILED: u64 = 0x0200;

/// Field-section codec error. Every variant is a decompression failure at the
/// HTTP/3 layer; [`QpackError::code`] returns the single wire error code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QpackError {
    /// Input ended in the middle of a representation.
    UnexpectedEof,
    /// A prefixed integer exceeded the 2^32−1 implementation limit.
    IntegerOverflow,
    /// A static-table index is out of range (≥ [`STATIC_TABLE_SIZE`]).
    InvalidStaticIndex(u64),
    /// A representation referenced the dynamic table (or used a post-base
    /// form). Unreachable while we advertise zero table capacity, so it is an
    /// error rather than something to interpret.
    DynamicUnsupported,
    /// A Huffman-coded string held an invalid or incomplete code.
    InvalidHuffman,
    /// A string length field claimed more bytes than remain in the input.
    StringTooLong,
    /// The section prefix carried a non-zero Required Insert Count, which
    /// cannot occur with a zero-capacity dynamic table (RFC 9204 §4.5.1.1).
    NonZeroRequiredInsertCount(u64),
}

impl QpackError {
    /// The RFC 9204 §6 wire error code (always `QPACK_DECOMPRESSION_FAILED`).
    #[must_use]
    pub const fn code(&self) -> u64 {
        QPACK_DECOMPRESSION_FAILED
    }
}

impl core::fmt::Display for QpackError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "QPACK: unexpected EOF"),
            Self::IntegerOverflow => write!(f, "QPACK: prefixed integer overflow"),
            Self::InvalidStaticIndex(i) => write!(f, "QPACK: static index {i} out of range"),
            Self::DynamicUnsupported => {
                write!(f, "QPACK: dynamic-table reference with zero table capacity")
            }
            Self::InvalidHuffman => write!(f, "QPACK: invalid Huffman sequence"),
            Self::StringTooLong => write!(f, "QPACK: string length exceeds remaining input"),
            Self::NonZeroRequiredInsertCount(n) => {
                write!(f, "QPACK: non-zero Required Insert Count {n} with zero capacity")
            }
        }
    }
}

impl std::error::Error for QpackError {}

/// Map the shared HPACK primitive error into the QPACK error space. Only the
/// variants the reused primitives can actually raise are translated; the rest
/// are unreachable here and collapse to [`QpackError::UnexpectedEof`].
fn from_hpack(e: HpackError) -> QpackError {
    match e {
        HpackError::UnexpectedEof => QpackError::UnexpectedEof,
        HpackError::IntegerOverflow => QpackError::IntegerOverflow,
        HpackError::InvalidHuffman => QpackError::InvalidHuffman,
        HpackError::StringTooLong => QpackError::StringTooLong,
        HpackError::InvalidIndex(_) | HpackError::TableSizeTooLarge => QpackError::UnexpectedEof,
    }
}

// ── Static table (RFC 9204 Appendix A) ─────────────────────────────────────

/// The 99-entry QPACK static table (RFC 9204 Appendix A). Index is 0-based
/// (unlike HPACK's 1-based table). Entries with an empty value store `""`.
const STATIC_TABLE: [(&str, &str); 99] = [
    (":authority", ""),                                                        // 0
    (":path", "/"),                                                            // 1
    ("age", "0"),                                                              // 2
    ("content-disposition", ""),                                               // 3
    ("content-length", "0"),                                                   // 4
    ("cookie", ""),                                                            // 5
    ("date", ""),                                                              // 6
    ("etag", ""),                                                              // 7
    ("if-modified-since", ""),                                                 // 8
    ("if-none-match", ""),                                                     // 9
    ("last-modified", ""),                                                     // 10
    ("link", ""),                                                              // 11
    ("location", ""),                                                          // 12
    ("referer", ""),                                                           // 13
    ("set-cookie", ""),                                                        // 14
    (":method", "CONNECT"),                                                    // 15
    (":method", "DELETE"),                                                     // 16
    (":method", "GET"),                                                        // 17
    (":method", "HEAD"),                                                       // 18
    (":method", "OPTIONS"),                                                    // 19
    (":method", "POST"),                                                       // 20
    (":method", "PUT"),                                                        // 21
    (":scheme", "http"),                                                       // 22
    (":scheme", "https"),                                                      // 23
    (":status", "103"),                                                        // 24
    (":status", "200"),                                                        // 25
    (":status", "304"),                                                        // 26
    (":status", "404"),                                                        // 27
    (":status", "503"),                                                        // 28
    ("accept", "*/*"),                                                         // 29
    ("accept", "application/dns-message"),                                     // 30
    ("accept-encoding", "gzip, deflate, br"),                                  // 31
    ("accept-ranges", "bytes"),                                                // 32
    ("access-control-allow-headers", "cache-control"),                         // 33
    ("access-control-allow-headers", "content-type"),                          // 34
    ("access-control-allow-origin", "*"),                                      // 35
    ("cache-control", "max-age=0"),                                            // 36
    ("cache-control", "max-age=2592000"),                                      // 37
    ("cache-control", "max-age=604800"),                                       // 38
    ("cache-control", "no-cache"),                                             // 39
    ("cache-control", "no-store"),                                             // 40
    ("cache-control", "public, max-age=31536000"),                            // 41
    ("content-encoding", "br"),                                                // 42
    ("content-encoding", "gzip"),                                              // 43
    ("content-type", "application/dns-message"),                               // 44
    ("content-type", "application/javascript"),                                // 45
    ("content-type", "application/json"),                                      // 46
    ("content-type", "application/x-www-form-urlencoded"),                     // 47
    ("content-type", "image/gif"),                                             // 48
    ("content-type", "image/jpeg"),                                            // 49
    ("content-type", "image/png"),                                             // 50
    ("content-type", "text/css"),                                             // 51
    ("content-type", "text/html; charset=utf-8"),                              // 52
    ("content-type", "text/plain"),                                            // 53
    ("content-type", "text/plain;charset=utf-8"),                              // 54
    ("range", "bytes=0-"),                                                     // 55
    ("strict-transport-security", "max-age=31536000"),                         // 56
    ("strict-transport-security", "max-age=31536000; includesubdomains"),      // 57
    ("strict-transport-security", "max-age=31536000; includesubdomains; preload"), // 58
    ("vary", "accept-encoding"),                                               // 59
    ("vary", "origin"),                                                        // 60
    ("x-content-type-options", "nosniff"),                                     // 61
    ("x-xss-protection", "1; mode=block"),                                     // 62
    (":status", "100"),                                                        // 63
    (":status", "204"),                                                        // 64
    (":status", "206"),                                                        // 65
    (":status", "302"),                                                        // 66
    (":status", "400"),                                                        // 67
    (":status", "403"),                                                        // 68
    (":status", "421"),                                                        // 69
    (":status", "425"),                                                        // 70
    (":status", "500"),                                                        // 71
    ("accept-language", ""),                                                   // 72
    ("access-control-allow-credentials", "FALSE"),                             // 73
    ("access-control-allow-credentials", "TRUE"),                              // 74
    ("access-control-allow-headers", "*"),                                     // 75
    ("access-control-allow-methods", "get"),                                   // 76
    ("access-control-allow-methods", "get, post, options"),                    // 77
    ("access-control-allow-methods", "options"),                               // 78
    ("access-control-expose-headers", "content-length"),                       // 79
    ("access-control-request-headers", "content-type"),                        // 80
    ("access-control-request-method", "get"),                                  // 81
    ("access-control-request-method", "post"),                                 // 82
    ("alt-svc", "clear"),                                                      // 83
    ("authorization", ""),                                                     // 84
    ("content-security-policy", "script-src 'none'; object-src 'none'; base-uri 'none'"), // 85
    ("early-data", "1"),                                                       // 86
    ("expect-ct", ""),                                                         // 87
    ("forwarded", ""),                                                         // 88
    ("if-range", ""),                                                          // 89
    ("origin", ""),                                                            // 90
    ("purpose", "prefetch"),                                                   // 91
    ("server", ""),                                                            // 92
    ("timing-allow-origin", "*"),                                              // 93
    ("upgrade-insecure-requests", "1"),                                        // 94
    ("user-agent", ""),                                                        // 95
    ("x-forwarded-for", ""),                                                   // 96
    ("x-frame-options", "deny"),                                               // 97
    ("x-frame-options", "sameorigin"),                                         // 98
];

/// Number of entries in the QPACK static table (valid indices `0..99`).
pub const STATIC_TABLE_SIZE: usize = STATIC_TABLE.len();

/// Look up a static-table entry by 0-based index.
fn static_entry(index: u64) -> Result<(&'static str, &'static str), QpackError> {
    usize::try_from(index)
        .ok()
        .and_then(|i| STATIC_TABLE.get(i).copied())
        .ok_or(QpackError::InvalidStaticIndex(index))
}

/// First static index whose name and value both match, if any.
fn find_static_full(name: &[u8], value: &[u8]) -> Option<u64> {
    STATIC_TABLE
        .iter()
        .position(|&(n, v)| n.as_bytes() == name && v.as_bytes() == value)
        .map(|i| i as u64)
}

/// First static index whose name matches, if any.
fn find_static_name(name: &[u8]) -> Option<u64> {
    STATIC_TABLE
        .iter()
        .position(|&(n, _)| n.as_bytes() == name)
        .map(|i| i as u64)
}

// ── Header field ───────────────────────────────────────────────────────────

/// A decoded header field. `sensitive` reflects the QPACK "never index" (`N`)
/// bit (RFC 9204 §4.5.4/§4.5.6); with a static-only codec it changes only
/// whether the encoder sets that bit — there is no intermediary cache to
/// protect here, but the flag round-trips for callers that care.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderField {
    /// Lower-case header field name (or an HTTP/3 pseudo-header like `:path`).
    pub name: Vec<u8>,
    /// Header field value.
    pub value: Vec<u8>,
    /// The QPACK "never index" (`N`) bit was set (RFC 9204 §4.5.4).
    pub sensitive: bool,
}

impl HeaderField {
    /// Build a non-sensitive field from `name`/`value`.
    #[must_use]
    pub fn new(name: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self { name: name.into(), value: value.into(), sensitive: false }
    }

    /// Build a field with the "never index" (`N`) bit set.
    #[must_use]
    pub fn sensitive(name: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        Self { name: name.into(), value: value.into(), sensitive: true }
    }

    /// The name as UTF-8 (best-effort; non-UTF-8 yields `""`).
    #[must_use]
    pub fn name_str(&self) -> &str {
        std::str::from_utf8(&self.name).unwrap_or("")
    }

    /// The value as UTF-8 (best-effort; non-UTF-8 yields `""`).
    #[must_use]
    pub fn value_str(&self) -> &str {
        std::str::from_utf8(&self.value).unwrap_or("")
    }
}

// ── String literals (RFC 9204 §4.1.2) ──────────────────────────────────────

/// Decode a QPACK string literal from the front of `src`.
///
/// The first byte holds the Huffman flag `H` at bit `prefix_bits` and a
/// `prefix_bits`-wide length prefix in its low bits (the representation-type
/// bits above `H` are ignored — the caller already dispatched on them).
/// Returns `(bytes, consumed)`.
fn decode_string(src: &[u8], prefix_bits: u8) -> Result<(Vec<u8>, usize), QpackError> {
    let first = *src.first().ok_or(QpackError::UnexpectedEof)?;
    let huffman = first & (1 << prefix_bits) != 0;
    let (len, hdr) = decode_int(src, prefix_bits).map_err(from_hpack)?;
    let len = usize::try_from(len).map_err(|_| QpackError::StringTooLong)?;
    let end = hdr.checked_add(len).ok_or(QpackError::StringTooLong)?;
    if end > src.len() {
        return Err(QpackError::StringTooLong);
    }
    let raw = &src[hdr..end];
    let bytes = if huffman {
        huffman_decode(raw).map_err(from_hpack)?
    } else {
        raw.to_vec()
    };
    Ok((bytes, end))
}

/// Encode a QPACK string literal onto `out`.
///
/// `type_prefix` supplies the representation-type bits that share the first
/// byte (already positioned above the `H` flag); the `H` flag sits at bit
/// `prefix_bits`. Huffman coding is used only when it is not longer than the
/// raw bytes.
fn encode_string(out: &mut Vec<u8>, s: &[u8], prefix_bits: u8, type_prefix: u8, use_huffman: bool) {
    let huff_bit = 1u8 << prefix_bits;
    if use_huffman {
        let encoded = huffman_encode(s);
        if encoded.len() < s.len() {
            out.extend_from_slice(&encode_int(
                encoded.len() as u64,
                prefix_bits,
                type_prefix | huff_bit,
            ));
            out.extend_from_slice(&encoded);
            return;
        }
    }
    out.extend_from_slice(&encode_int(s.len() as u64, prefix_bits, type_prefix));
    out.extend_from_slice(s);
}

// ── Encoded Field Section Prefix (RFC 9204 §4.5.1) ─────────────────────────

/// Serialize the field-section prefix for a static-only encoding:
/// `Required Insert Count = 0` (§4.5.1.1) and `Base = 0` (`S = 0`,
/// `Delta Base = 0`, §4.5.1.2) — the two zero bytes `00 00`.
fn encode_prefix(out: &mut Vec<u8>) {
    // Required Insert Count, 8-bit prefix integer, value 0.
    out.push(0x00);
    // Sign bit S=0 in the top bit, Delta Base = 0 in the 7-bit prefix.
    out.push(0x00);
}

/// Parse and validate the field-section prefix. Returns the number of bytes
/// consumed. Any non-zero Required Insert Count is rejected because a
/// zero-capacity dynamic table makes it impossible (RFC 9204 §4.5.1.1).
fn decode_prefix(src: &[u8]) -> Result<usize, QpackError> {
    let (ric, n1) = decode_int(src, 8).map_err(from_hpack)?;
    if ric != 0 {
        return Err(QpackError::NonZeroRequiredInsertCount(ric));
    }
    // Base: sign bit S (top bit) then a 7-bit-prefix Delta Base. With
    // Required Insert Count 0 the Base is 0, but parse it to consume the bytes
    // and to surface a truncated prefix.
    let (_delta_base, n2) = decode_int(&src[n1..], 7).map_err(from_hpack)?;
    Ok(n1 + n2)
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Encode a list of header fields into a QPACK field section (RFC 9204 §4.5),
/// referencing only the static table.
///
/// Representation choice per field:
/// - exact static match (name + value) → Indexed Field Line (§4.5.2);
/// - static name match only → Literal Field Line With Name Reference (§4.5.4);
/// - otherwise → Literal Field Line With Literal Name (§4.5.6).
///
/// `use_huffman` enables Huffman coding of literal names/values when it does
/// not enlarge them.
#[must_use]
pub fn encode_field_section(fields: &[HeaderField], use_huffman: bool) -> Vec<u8> {
    let mut out = Vec::new();
    encode_prefix(&mut out);
    for field in fields {
        let name = &field.name;
        let value = &field.value;

        if !field.sensitive
            && let Some(idx) = find_static_full(name, value)
        {
            // §4.5.2 Indexed Field Line: `1 T(=1) Index(6+)`.
            out.extend_from_slice(&encode_int(idx, 6, 0xc0));
        } else if let Some(idx) = find_static_name(name) {
            // §4.5.4 Literal With Name Reference: `0 1 N T(=1) NameIndex(4+)`.
            // The "never index" bit N is bit 5 (0x20) in this representation.
            let n_bit = if field.sensitive { 0x20 } else { 0x00 };
            out.extend_from_slice(&encode_int(idx, 4, 0x50 | n_bit));
            // Value: standalone string, `H Length(7+)`.
            encode_string(&mut out, value, 7, 0x00, use_huffman);
        } else {
            // §4.5.6 Literal With Literal Name: `0 0 1 N H NameLen(3+)`.
            // Here the N bit is bit 4 (0x10); H is bit 3, carried by
            // `encode_string` via its 3-bit prefix.
            let n_bit = if field.sensitive { 0x10 } else { 0x00 };
            encode_string(&mut out, name, 3, 0x20 | n_bit, use_huffman);
            encode_string(&mut out, value, 7, 0x00, use_huffman);
        }
    }
    out
}

/// Decode a QPACK field section (RFC 9204 §4.5) that references only the static
/// table.
///
/// # Errors
///
/// Returns [`QpackError`] on a malformed section, a static index out of range,
/// a Huffman error, or any dynamic-table / post-base reference (which a peer
/// honouring a zero table capacity never emits).
pub fn decode_field_section(buf: &[u8]) -> Result<Vec<HeaderField>, QpackError> {
    let mut pos = decode_prefix(buf)?;
    let mut fields = Vec::new();

    while pos < buf.len() {
        let b = buf[pos];
        if b & 0x80 != 0 {
            // §4.5.2 Indexed Field Line: `1 T Index(6+)`.
            if b & 0x40 == 0 {
                // T = 0 → dynamic table.
                return Err(QpackError::DynamicUnsupported);
            }
            let (idx, consumed) = decode_int(&buf[pos..], 6).map_err(from_hpack)?;
            pos += consumed;
            let (name, value) = static_entry(idx)?;
            fields.push(HeaderField::new(name.as_bytes(), value.as_bytes()));
        } else if b & 0x40 != 0 {
            // §4.5.4 Literal With Name Reference: `0 1 N T NameIndex(4+)`.
            if b & 0x10 == 0 {
                // T = 0 → dynamic table.
                return Err(QpackError::DynamicUnsupported);
            }
            let sensitive = b & 0x20 != 0;
            let (idx, consumed) = decode_int(&buf[pos..], 4).map_err(from_hpack)?;
            pos += consumed;
            let (name, _) = static_entry(idx)?;
            let (value, adv) = decode_string(&buf[pos..], 7)?;
            pos += adv;
            fields.push(HeaderField {
                name: name.as_bytes().to_vec(),
                value,
                sensitive,
            });
        } else if b & 0x20 != 0 {
            // §4.5.6 Literal With Literal Name: `0 0 1 N H NameLen(3+)`.
            let sensitive = b & 0x10 != 0;
            let (name, adv) = decode_string(&buf[pos..], 3)?;
            pos += adv;
            let (value, adv2) = decode_string(&buf[pos..], 7)?;
            pos += adv2;
            fields.push(HeaderField { name, value, sensitive });
        } else if b & 0x10 != 0 {
            // §4.5.3 Indexed Field Line With Post-Base Index — dynamic only.
            return Err(QpackError::DynamicUnsupported);
        } else {
            // §4.5.5 Literal With Post-Base Name Reference — dynamic only.
            return Err(QpackError::DynamicUnsupported);
        }
    }

    Ok(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The static table has exactly 99 entries and its bookends match the RFC.
    #[test]
    fn static_table_shape() {
        assert_eq!(STATIC_TABLE_SIZE, 99);
        assert_eq!(STATIC_TABLE[0], (":authority", ""));
        assert_eq!(STATIC_TABLE[17], (":method", "GET"));
        assert_eq!(STATIC_TABLE[98], ("x-frame-options", "sameorigin"));
    }

    /// Encode → decode returns the original fields for every representation.
    fn roundtrip(fields: &[HeaderField], use_huffman: bool) {
        let encoded = encode_field_section(fields, use_huffman);
        let decoded = decode_field_section(&encoded).expect("decode");
        assert_eq!(decoded, fields, "roundtrip (huffman={use_huffman})");
    }

    #[test]
    fn roundtrip_all_representations() {
        for huff in [false, true] {
            // Exact static match, static name match, and fully literal.
            roundtrip(
                &[
                    HeaderField::new(":method", "GET"),   // Indexed (17)
                    HeaderField::new(":path", "/index"),   // Name ref (1)
                    HeaderField::new("x-custom", "hello"), // Literal name
                ],
                huff,
            );
            roundtrip(&[HeaderField::new(":status", "200")], huff); // Indexed (25)
            roundtrip(&[], huff); // empty section is legal
        }
    }

    #[test]
    fn empty_section_is_prefix_only() {
        let encoded = encode_field_section(&[], false);
        // Required Insert Count 0, Base 0 → two zero bytes, nothing else.
        assert_eq!(encoded, vec![0x00, 0x00]);
        assert_eq!(decode_field_section(&encoded).unwrap(), vec![]);
    }

    #[test]
    fn indexed_field_line_wire_shape() {
        // :method GET is static index 17; Indexed Field Line = 1 T(=1) 010001.
        // 0x80 | 0x40 | 17 = 0xd1, after the `00 00` prefix.
        let encoded = encode_field_section(&[HeaderField::new(":method", "GET")], false);
        assert_eq!(encoded, vec![0x00, 0x00, 0xd1]);
    }

    #[test]
    fn name_reference_prefers_static_name() {
        // :authority is static index 0; value is literal.
        let encoded = encode_field_section(
            &[HeaderField::new(":authority", "example.com")],
            false,
        );
        // prefix, then 0 1 N(=0) T(=1) 0000 = 0x50, name index 0.
        assert_eq!(encoded[2], 0x50);
        let decoded = decode_field_section(&encoded).unwrap();
        assert_eq!(decoded, vec![HeaderField::new(":authority", "example.com")]);
    }

    #[test]
    fn literal_name_round_trips_case_and_bytes() {
        // A header absent from the static table exercises §4.5.6.
        let f = HeaderField::new("x-request-id", "abc-123");
        let encoded = encode_field_section(std::slice::from_ref(&f), true);
        // First field byte has the `001` pattern.
        assert_eq!(encoded[2] & 0xe0, 0x20);
        assert_eq!(decode_field_section(&encoded).unwrap(), vec![f]);
    }

    #[test]
    fn sensitive_bit_round_trips() {
        // Never-indexed literal with a static name reference.
        let f = HeaderField::sensitive("authorization", "Bearer xyz");
        let encoded = encode_field_section(std::slice::from_ref(&f), false);
        // `0 1 N(=1) T(=1)` → 0x40 | 0x20 | 0x10 = 0x70 in the high nibble bits.
        assert_eq!(encoded[2] & 0xf0, 0x70);
        let decoded = decode_field_section(&encoded).unwrap();
        assert_eq!(decoded, vec![f]);
        assert!(decoded[0].sensitive);
    }

    #[test]
    fn sensitive_literal_name_sets_n_bit() {
        // Never-indexed literal whose name is not in the static table (§4.5.6).
        let f = HeaderField::sensitive("x-secret", "s");
        let encoded = encode_field_section(std::slice::from_ref(&f), false);
        // `0 0 1 N(=1) H` → high bits 0x20 | 0x10 = 0x30.
        assert_eq!(encoded[2] & 0xf0, 0x30);
        let decoded = decode_field_section(&encoded).unwrap();
        assert!(decoded[0].sensitive);
        assert_eq!(decoded, vec![f]);
    }

    #[test]
    fn full_request_field_section() {
        // A realistic GET request field section.
        let fields = vec![
            HeaderField::new(":method", "GET"),
            HeaderField::new(":scheme", "https"),
            HeaderField::new(":authority", "www.example.com"),
            HeaderField::new(":path", "/"),
            HeaderField::new("accept", "*/*"),
            HeaderField::new("user-agent", "lumen/0.5"),
        ];
        roundtrip(&fields, true);
        roundtrip(&fields, false);
    }

    #[test]
    fn decode_rejects_dynamic_indexed_field_line() {
        // Prefix `00 00`, then `1 T(=0) 000000` = 0x80 (Indexed, dynamic).
        let buf = [0x00, 0x00, 0x80];
        assert_eq!(
            decode_field_section(&buf),
            Err(QpackError::DynamicUnsupported)
        );
    }

    #[test]
    fn decode_rejects_dynamic_name_reference() {
        // Prefix, then `0 1 N(=0) T(=0) 0000` = 0x40 (Name ref, dynamic).
        let buf = [0x00, 0x00, 0x40];
        assert_eq!(
            decode_field_section(&buf),
            Err(QpackError::DynamicUnsupported)
        );
    }

    #[test]
    fn decode_rejects_post_base_forms() {
        // `0001....` Indexed Post-Base and `0000....` Literal Post-Base.
        assert_eq!(
            decode_field_section(&[0x00, 0x00, 0x10]),
            Err(QpackError::DynamicUnsupported)
        );
        assert_eq!(
            decode_field_section(&[0x00, 0x00, 0x00]),
            Err(QpackError::DynamicUnsupported)
        );
    }

    #[test]
    fn decode_rejects_nonzero_required_insert_count() {
        // Required Insert Count = 1 in the 8-bit prefix.
        let buf = [0x01, 0x00];
        assert_eq!(
            decode_field_section(&buf),
            Err(QpackError::NonZeroRequiredInsertCount(1))
        );
    }

    #[test]
    fn decode_rejects_out_of_range_static_index() {
        // Indexed static index 99 (one past the end): 0xc0 | value.
        let mut buf = vec![0x00, 0x00];
        buf.extend_from_slice(&encode_int(99, 6, 0xc0));
        assert_eq!(
            decode_field_section(&buf),
            Err(QpackError::InvalidStaticIndex(99))
        );
    }

    #[test]
    fn decode_truncated_prefix_is_eof() {
        assert_eq!(decode_field_section(&[]), Err(QpackError::UnexpectedEof));
        assert_eq!(decode_field_section(&[0x00]), Err(QpackError::UnexpectedEof));
    }

    #[test]
    fn decode_truncated_value_is_string_too_long() {
        // :path name ref (index 1) with a value claiming 5 bytes but empty.
        let mut buf = vec![0x00, 0x00];
        buf.extend_from_slice(&encode_int(1, 4, 0x50)); // name ref, static idx 1
        buf.push(0x05); // H=0, length 5, but no value bytes follow
        assert_eq!(decode_field_section(&buf), Err(QpackError::StringTooLong));
    }

    #[test]
    fn large_static_index_uses_multibyte_prefix() {
        // Index 98 needs the 6-bit prefix continuation (98 ≥ 63).
        let f = HeaderField::new("x-frame-options", "sameorigin");
        let encoded = encode_field_section(std::slice::from_ref(&f), false);
        // Indexed: high bits 0xc0, 6-bit prefix all-ones then continuation.
        assert_eq!(encoded[2], 0xff); // 0xc0 | 0x3f
        assert_eq!(decode_field_section(&encoded).unwrap(), vec![f]);
    }
}
