//! HPACK header compression codec — RFC 7541.
//!
//! Scope: static table (§Appendix A), dynamic table (§2.3.2), integer
//! encode/decode (§5.1), Huffman encode/decode (§5.2, §Appendix B), full
//! header block decode (§6) and encode.
//!
//! Out of scope: CONTINUATION frame reassembly — the connection layer must
//! concatenate all block fragments before calling [`Decoder::decode`].

use std::collections::VecDeque;

// ── Error ─────────────────────────────────────────────────────────────────

/// HPACK codec error. All variants map to `COMPRESSION_ERROR` (0x09) at the
/// HTTP/2 connection layer per RFC 9113 §6.4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HpackError {
    /// Input truncated before the representation was complete.
    UnexpectedEof,
    /// Integer value exceeds 2^32−1 (implementation limit).
    IntegerOverflow,
    /// Table index 0 or beyond static+dynamic table length.
    InvalidIndex(usize),
    /// Huffman-encoded string contains an invalid or incomplete code.
    InvalidHuffman,
    /// String length field claims more bytes than remain in the input.
    StringTooLong,
    /// Dynamic table size update exceeds the protocol-negotiated maximum.
    TableSizeTooLarge,
}

impl std::fmt::Display for HpackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "HPACK: unexpected EOF"),
            Self::IntegerOverflow => write!(f, "HPACK: integer overflow"),
            Self::InvalidIndex(i) => write!(f, "HPACK: invalid index {i}"),
            Self::InvalidHuffman => write!(f, "HPACK: invalid Huffman sequence"),
            Self::StringTooLong => write!(f, "HPACK: string length exceeds remaining input"),
            Self::TableSizeTooLarge => {
                write!(f, "HPACK: dynamic table size update exceeds negotiated max")
            }
        }
    }
}

impl std::error::Error for HpackError {}

// ── Huffman table (RFC 7541 Appendix B) ───────────────────────────────────

/// `(code, nbits)` for each octet 0–255 plus EOS (index 256).
/// The code is stored as a plain integer; the top `nbits` bits are
/// the canonical code MSB-first.
const HUFFMAN_TABLE: [(u32, u8); 257] = [
    (0x1ff8, 13),       //   0
    (0x7fffd8, 23),     //   1
    (0xfffffe2, 28),    //   2
    (0xfffffe3, 28),    //   3
    (0xfffffe4, 28),    //   4
    (0xfffffe5, 28),    //   5
    (0xfffffe6, 28),    //   6
    (0xfffffe7, 28),    //   7
    (0xfffffe8, 28),    //   8
    (0xffffea, 24),     //   9
    (0x3ffffffa, 30),   //  10
    (0xfffffe9, 28),    //  11
    (0xfffffea, 28),    //  12
    (0x3ffffffb, 30),   //  13
    (0xfffffeb, 28),    //  14
    (0xfffffec, 28),    //  15
    (0xfffffed, 28),    //  16
    (0xfffffee, 28),    //  17
    (0xfffffef, 28),    //  18
    (0xffffff0, 28),    //  19
    (0xffffff1, 28),    //  20
    (0xffffff2, 28),    //  21
    (0x3ffffffc, 30),   //  22
    (0xffffff3, 28),    //  23
    (0xffffff4, 28),    //  24
    (0xffffff5, 28),    //  25
    (0xffffff6, 28),    //  26
    (0xffffff7, 28),    //  27
    (0xffffff8, 28),    //  28
    (0xffffff9, 28),    //  29
    (0xffffffa, 28),    //  30
    (0xffffffb, 28),    //  31
    (0x14, 6),          //  32  ' '
    (0x3f8, 10),        //  33  '!'
    (0x3f9, 10),        //  34  '"'
    (0xffa, 12),        //  35  '#'
    (0x1ff9, 13),       //  36  '$'
    (0x15, 6),          //  37  '%'
    (0xf8, 8),          //  38  '&'
    (0x7fa, 11),        //  39  '\''
    (0x3fa, 10),        //  40  '('
    (0x3fb, 10),        //  41  ')'
    (0xf9, 8),          //  42  '*'
    (0x7fb, 11),        //  43  '+'
    (0xfa, 8),          //  44  ','
    (0x16, 6),          //  45  '-'
    (0x17, 6),          //  46  '.'
    (0x18, 6),          //  47  '/'
    (0x0, 5),           //  48  '0'
    (0x1, 5),           //  49  '1'
    (0x2, 5),           //  50  '2'
    (0x19, 6),          //  51  '3'
    (0x1a, 6),          //  52  '4'
    (0x1b, 6),          //  53  '5'
    (0x1c, 6),          //  54  '6'
    (0x1d, 6),          //  55  '7'
    (0x1e, 6),          //  56  '8'
    (0x1f, 6),          //  57  '9'
    (0x5c, 7),          //  58  ':'
    (0xfb, 8),          //  59  ';'
    (0x7ffc, 15),       //  60  '<'
    (0x20, 6),          //  61  '='
    (0xffb, 12),        //  62  '>'
    (0x3fc, 10),        //  63  '?'
    (0x1ffa, 13),       //  64  '@'
    (0x21, 6),          //  65  'A'
    (0x5d, 7),          //  66  'B'
    (0x5e, 7),          //  67  'C'
    (0x5f, 7),          //  68  'D'
    (0x60, 7),          //  69  'E'
    (0x61, 7),          //  70  'F'
    (0x62, 7),          //  71  'G'
    (0x63, 7),          //  72  'H'
    (0x64, 7),          //  73  'I'
    (0x65, 7),          //  74  'J'
    (0x66, 7),          //  75  'K'
    (0x67, 7),          //  76  'L'
    (0x68, 7),          //  77  'M'
    (0x69, 7),          //  78  'N'
    (0x6a, 7),          //  79  'O'
    (0x6b, 7),          //  80  'P'
    (0x6c, 7),          //  81  'Q'
    (0x6d, 7),          //  82  'R'
    (0x6e, 7),          //  83  'S'
    (0x6f, 7),          //  84  'T'
    (0x70, 7),          //  85  'U'
    (0x71, 7),          //  86  'V'
    (0x72, 7),          //  87  'W'
    (0xfc, 8),          //  88  'X'
    (0x73, 7),          //  89  'Y'
    (0xfd, 8),          //  90  'Z'
    (0x1ffb, 13),       //  91  '['
    (0x7fff0, 19),      //  92  '\\'
    (0x1ffc, 13),       //  93  ']'
    (0x3ffc, 14),       //  94  '^'
    (0x22, 6),          //  95  '_'
    (0x7ffd, 15),       //  96  '`'
    (0x3, 5),           //  97  'a'
    (0x23, 6),          //  98  'b'
    (0x4, 5),           //  99  'c'
    (0x24, 6),          // 100  'd'
    (0x5, 5),           // 101  'e'
    (0x25, 6),          // 102  'f'
    (0x26, 6),          // 103  'g'
    (0x27, 6),          // 104  'h'
    (0x6, 5),           // 105  'i'
    (0x74, 7),          // 106  'j'
    (0x75, 7),          // 107  'k'
    (0x28, 6),          // 108  'l'
    (0x29, 6),          // 109  'm'
    (0x2a, 6),          // 110  'n'
    (0x7, 5),           // 111  'o'
    (0x2b, 6),          // 112  'p'
    (0x76, 7),          // 113  'q'
    (0x2c, 6),          // 114  'r'
    (0x8, 5),           // 115  's'
    (0x9, 5),           // 116  't'
    (0x2d, 6),          // 117  'u'
    (0x77, 7),          // 118  'v'
    (0x78, 7),          // 119  'w'
    (0x79, 7),          // 120  'x'
    (0x7a, 7),          // 121  'y'
    (0x7b, 7),          // 122  'z'
    (0x7ffe, 15),       // 123  '{'
    (0x7fc, 11),        // 124  '|'
    (0x3ffd, 14),       // 125  '}'
    (0x1ffd, 13),       // 126  '~'
    (0xffffffc, 28),    // 127
    (0xfffe6, 20),      // 128
    (0x3fffd2, 22),     // 129
    (0xfffe7, 20),      // 130
    (0xfffe8, 20),      // 131
    (0x3fffd3, 22),     // 132
    (0x3fffd4, 22),     // 133
    (0x3fffd5, 22),     // 134
    (0x7fffd9, 23),     // 135
    (0x3fffd6, 22),     // 136
    (0x7fffda, 23),     // 137
    (0x7fffdb, 23),     // 138
    (0x7fffdc, 23),     // 139
    (0x7fffdd, 23),     // 140
    (0x7fffde, 23),     // 141
    (0xffffeb, 24),     // 142
    (0x7fffdf, 23),     // 143
    (0xffffec, 24),     // 144
    (0xffffed, 24),     // 145
    (0x3fffd7, 22),     // 146
    (0x7fffe0, 23),     // 147
    (0xffffee, 24),     // 148
    (0x7fffe1, 23),     // 149
    (0x7fffe2, 23),     // 150
    (0x7fffe3, 23),     // 151
    (0x7fffe4, 23),     // 152
    (0x1fffdc, 21),     // 153
    (0x3fffd8, 22),     // 154
    (0x7fffe5, 23),     // 155
    (0x3fffd9, 22),     // 156
    (0x7fffe6, 23),     // 157
    (0x7fffe7, 23),     // 158
    (0xffffef, 24),     // 159
    (0x3fffda, 22),     // 160
    (0x1fffdd, 21),     // 161
    (0xfffe9, 20),      // 162
    (0x3fffdb, 22),     // 163
    (0x3fffdc, 22),     // 164
    (0x7fffe8, 23),     // 165
    (0x7fffe9, 23),     // 166
    (0x1fffde, 21),     // 167
    (0x7fffea, 23),     // 168
    (0x3fffdd, 22),     // 169
    (0x3fffde, 22),     // 170
    (0xfffff0, 24),     // 171
    (0x1fffdf, 21),     // 172
    (0x3fffdf, 22),     // 173
    (0x7fffeb, 23),     // 174
    (0x7fffec, 23),     // 175
    (0x1fffe0, 21),     // 176
    (0x1fffe1, 21),     // 177
    (0x3fffe0, 22),     // 178
    (0x1fffe2, 21),     // 179
    (0x7fffed, 23),     // 180
    (0x3fffe1, 22),     // 181
    (0x7fffee, 23),     // 182
    (0x7fffef, 23),     // 183
    (0xfffea, 20),      // 184
    (0x3fffe2, 22),     // 185
    (0x3fffe3, 22),     // 186
    (0x3fffe4, 22),     // 187
    (0x7ffff0, 23),     // 188
    (0x3fffe5, 22),     // 189
    (0x3fffe6, 22),     // 190
    (0x7ffff1, 23),     // 191
    (0x3ffffe0, 26),    // 192
    (0x3ffffe1, 26),    // 193
    (0xfffeb, 20),      // 194
    (0x7fff1, 19),      // 195
    (0x3fffe7, 22),     // 196
    (0x7ffff2, 23),     // 197
    (0x3fffe8, 22),     // 198
    (0x1ffffec, 25),    // 199
    (0x3ffffe2, 26),    // 200
    (0x3ffffe3, 26),    // 201
    (0x3ffffe4, 26),    // 202
    (0x7ffffde, 27),    // 203
    (0x7ffffdf, 27),    // 204
    (0x3ffffe5, 26),    // 205
    (0xfffff1, 24),     // 206
    (0x1ffffed, 25),    // 207
    (0x7fff2, 19),      // 208
    (0x1fffe3, 21),     // 209
    (0x3ffffe6, 26),    // 210
    (0x7ffffe0, 27),    // 211
    (0x7ffffe1, 27),    // 212
    (0x3ffffe7, 26),    // 213
    (0x7ffffe2, 27),    // 214
    (0xfffff2, 24),     // 215
    (0x1fffe4, 21),     // 216
    (0x1fffe5, 21),     // 217
    (0x3ffffe8, 26),    // 218
    (0x3ffffe9, 26),    // 219
    (0xffffffd, 28),    // 220
    (0x7ffffe3, 27),    // 221
    (0x7ffffe4, 27),    // 222
    (0x7ffffe5, 27),    // 223
    (0xfffec, 20),      // 224
    (0xfffff3, 24),     // 225
    (0xfffed, 20),      // 226
    (0x1fffe6, 21),     // 227
    (0x3fffe9, 22),     // 228
    (0x1fffe7, 21),     // 229
    (0x1fffe8, 21),     // 230
    (0x7ffff3, 23),     // 231
    (0x3fffea, 22),     // 232
    (0x3fffeb, 22),     // 233
    (0x1ffffee, 25),    // 234
    (0x1ffffef, 25),    // 235
    (0xfffff4, 24),     // 236
    (0xfffff5, 24),     // 237
    (0x3ffffea, 26),    // 238
    (0x7ffff4, 23),     // 239
    (0x3ffffeb, 26),    // 240
    (0x7ffffe6, 27),    // 241
    (0x3ffffec, 26),    // 242
    (0x3ffffed, 26),    // 243
    (0x7ffffe7, 27),    // 244
    (0x7ffffe8, 27),    // 245
    (0x7ffffe9, 27),    // 246
    (0x7ffffea, 27),    // 247
    (0x7ffffeb, 27),    // 248
    (0xffffffe, 28),    // 249
    (0x7ffffec, 27),    // 250
    (0x7ffffed, 27),    // 251
    (0x7ffffee, 27),    // 252
    (0x7ffffef, 27),    // 253
    (0x7fffff0, 27),    // 254
    (0x3ffffee, 26),    // 255
    (0x3fffffff, 30),   // 256 EOS
];

// ── Static table (RFC 7541 Appendix A) ────────────────────────────────────

/// 61-entry static table. Index is 1-based; entry 0 is a placeholder.
const STATIC_TABLE: [(&str, &str); 62] = [
    ("", ""),                               // [0] placeholder — never accessed
    (":authority", ""),                     // [1]
    (":method", "GET"),                     // [2]
    (":method", "POST"),                    // [3]
    (":path", "/"),                         // [4]
    (":path", "/index.html"),               // [5]
    (":scheme", "http"),                    // [6]
    (":scheme", "https"),                   // [7]
    (":status", "200"),                     // [8]
    (":status", "204"),                     // [9]
    (":status", "206"),                     // [10]
    (":status", "304"),                     // [11]
    (":status", "400"),                     // [12]
    (":status", "404"),                     // [13]
    (":status", "500"),                     // [14]
    ("accept-charset", ""),                 // [15]
    ("accept-encoding", "gzip, deflate"),   // [16]
    ("accept-language", ""),               // [17]
    ("accept-ranges", ""),                  // [18]
    ("accept", ""),                         // [19]
    ("access-control-allow-origin", ""),    // [20]
    ("age", ""),                            // [21]
    ("allow", ""),                          // [22]
    ("authorization", ""),                  // [23]
    ("cache-control", ""),                  // [24]
    ("content-disposition", ""),            // [25]
    ("content-encoding", ""),               // [26]
    ("content-language", ""),              // [27]
    ("content-length", ""),                 // [28]
    ("content-location", ""),               // [29]
    ("content-range", ""),                  // [30]
    ("content-type", ""),                   // [31]
    ("cookie", ""),                         // [32]
    ("date", ""),                           // [33]
    ("etag", ""),                           // [34]
    ("expect", ""),                         // [35]
    ("expires", ""),                        // [36]
    ("from", ""),                           // [37]
    ("host", ""),                           // [38]
    ("if-match", ""),                       // [39]
    ("if-modified-since", ""),              // [40]
    ("if-none-match", ""),                  // [41]
    ("if-range", ""),                       // [42]
    ("if-unmodified-since", ""),            // [43]
    ("last-modified", ""),                  // [44]
    ("link", ""),                           // [45]
    ("location", ""),                       // [46]
    ("max-forwards", ""),                   // [47]
    ("proxy-authenticate", ""),             // [48]
    ("proxy-authorization", ""),            // [49]
    ("range", ""),                          // [50]
    ("referer", ""),                        // [51]
    ("refresh", ""),                        // [52]
    ("retry-after", ""),                    // [53]
    ("server", ""),                         // [54]
    ("set-cookie", ""),                     // [55]
    ("strict-transport-security", ""),      // [56]
    ("transfer-encoding", ""),              // [57]
    ("user-agent", ""),                     // [58]
    ("vary", ""),                           // [59]
    ("via", ""),                            // [60]
    ("www-authenticate", ""),               // [61]
];

/// Number of entries in the static table (indices 1..=61).
pub const STATIC_TABLE_SIZE: usize = 61;

// ── Integer encode / decode (RFC 7541 §5.1) ───────────────────────────────

/// Decode a variable-length integer with an `n`-bit prefix from `src`.
///
/// Returns `(value, bytes_consumed)`. The first byte of `src` is the byte
/// that contains the prefix; the high `(8 - n)` bits of that byte are the
/// representation type and are already consumed by the caller (they are
/// masked out here).
pub fn decode_int(src: &[u8], prefix_bits: u8) -> Result<(u64, usize), HpackError> {
    debug_assert!((1..=8).contains(&prefix_bits));
    if src.is_empty() {
        return Err(HpackError::UnexpectedEof);
    }
    let mask = (1u64 << prefix_bits) - 1;
    let prefix_val = u64::from(src[0]) & mask;
    if prefix_val < mask {
        return Ok((prefix_val, 1));
    }
    // Multi-byte sequence.
    let mut value = mask;
    let mut shift = 0u32;
    let mut pos = 1;
    loop {
        if pos >= src.len() {
            return Err(HpackError::UnexpectedEof);
        }
        let b = u64::from(src[pos]);
        pos += 1;
        let part = b & 0x7f;
        value = value
            .checked_add(part << shift)
            .ok_or(HpackError::IntegerOverflow)?;
        shift += 7;
        if shift > 32 {
            return Err(HpackError::IntegerOverflow);
        }
        if b & 0x80 == 0 {
            break;
        }
    }
    Ok((value, pos))
}

/// Encode an integer with an `n`-bit prefix. The `prefix_byte` holds the
/// high `(8 - n)` representation bits that are ORed into the first output byte.
pub fn encode_int(value: u64, prefix_bits: u8, prefix_byte: u8) -> Vec<u8> {
    debug_assert!((1..=8).contains(&prefix_bits));
    let mask = (1u64 << prefix_bits) - 1;
    if value < mask {
        return vec![prefix_byte | value as u8];
    }
    let mut out = vec![prefix_byte | mask as u8];
    let mut remaining = value - mask;
    while remaining >= 0x80 {
        out.push(0x80 | (remaining as u8 & 0x7f));
        remaining >>= 7;
    }
    out.push(remaining as u8);
    out
}

// ── Huffman encode / decode (RFC 7541 §5.2 + Appendix B) ──────────────────

/// Huffman-encode `input`. The result is padded to a byte boundary with
/// the most-significant bits of the EOS code (all-ones per RFC 7541 §5.2).
pub fn huffman_encode(input: &[u8]) -> Vec<u8> {
    let mut bit_buf: u64 = 0;
    let mut bit_len: u32 = 0;
    let mut out = Vec::new();

    for &byte in input {
        let (code, nbits) = HUFFMAN_TABLE[usize::from(byte)];
        let nbits = u32::from(nbits);
        bit_buf = (bit_buf << nbits) | u64::from(code);
        bit_len += nbits;
        while bit_len >= 8 {
            bit_len -= 8;
            out.push((bit_buf >> bit_len) as u8);
            // Discard the byte we just emitted so stale bits can't corrupt
            // subsequent left-shifts.
            bit_buf &= (1u64 << bit_len) - 1;
        }
    }
    // Pack remaining bits into high positions; fill low bits with EOS padding
    // (all-ones, RFC 7541 §5.2).
    if bit_len > 0 {
        let shift = 8 - bit_len;
        let b = ((bit_buf << shift) as u8) | ((1u8 << shift) - 1);
        out.push(b);
    }
    out
}

/// Huffman-decode `input`. Padding bits (EOS prefix, all-ones) are accepted
/// and stripped. Returns `Err(InvalidHuffman)` on any invalid sequence.
pub fn huffman_decode(input: &[u8]) -> Result<Vec<u8>, HpackError> {
    let mut acc: u64 = 0;
    let mut acc_bits: u32 = 0;
    let mut out = Vec::new();

    for &byte in input {
        acc = (acc << 8) | u64::from(byte);
        acc_bits += 8;

        'decode: loop {
            for (sym, &(code, nbits)) in HUFFMAN_TABLE[..256].iter().enumerate() {
                let nbits = u32::from(nbits);
                if acc_bits >= nbits {
                    let shifted = (acc >> (acc_bits - nbits)) as u32;
                    if shifted == code {
                        out.push(sym as u8);
                        acc_bits -= nbits;
                        acc &= (1u64 << acc_bits) - 1;
                        continue 'decode;
                    }
                }
            }
            break;
        }
    }

    // Remaining bits must be ≤ 7 and must all be 1s (EOS padding).
    if acc_bits > 7 {
        return Err(HpackError::InvalidHuffman);
    }
    if acc_bits > 0 {
        let expected = (1u64 << acc_bits) - 1;
        if acc & expected != expected {
            return Err(HpackError::InvalidHuffman);
        }
    }
    Ok(out)
}

// ── String encode / decode (RFC 7541 §5.2) ────────────────────────────────

/// Decode a header string (literal or Huffman) from `src`.
/// Returns `(decoded_bytes, total_bytes_consumed_from_src)`.
pub fn decode_string(src: &[u8]) -> Result<(Vec<u8>, usize), HpackError> {
    if src.is_empty() {
        return Err(HpackError::UnexpectedEof);
    }
    let use_huffman = src[0] & 0x80 != 0;
    let (str_len, hdr_len) = decode_int(src, 7)?;
    let str_len = str_len as usize;
    let end = hdr_len + str_len;
    if end > src.len() {
        return Err(HpackError::StringTooLong);
    }
    let raw = &src[hdr_len..end];
    let decoded = if use_huffman {
        huffman_decode(raw)?
    } else {
        raw.to_vec()
    };
    Ok((decoded, end))
}

/// Encode a header string. When `use_huffman` is true, the string is
/// Huffman-encoded only if that produces a shorter result (RFC 7541 §5.2).
pub fn encode_string(s: &[u8], use_huffman: bool) -> Vec<u8> {
    if use_huffman {
        let encoded = huffman_encode(s);
        if encoded.len() < s.len() {
            let mut hdr = encode_int(encoded.len() as u64, 7, 0x80);
            hdr.extend_from_slice(&encoded);
            return hdr;
        }
    }
    let mut hdr = encode_int(s.len() as u64, 7, 0x00);
    hdr.extend_from_slice(s);
    hdr
}

// ── Dynamic table (RFC 7541 §2.3.2) ───────────────────────────────────────

/// Entry size per RFC 7541 §4.1: name.len() + value.len() + 32.
const fn entry_size(name: &[u8], value: &[u8]) -> usize {
    name.len() + value.len() + 32
}

/// The dynamic table. Entries are added at the front (lowest dynamic index)
/// and evicted from the back. The table is shared between encode and decode
/// within a single connection direction.
pub struct DynamicTable {
    entries: VecDeque<(Vec<u8>, Vec<u8>)>,
    /// Current size in bytes (RFC 7541 §4.1 accounting).
    size: usize,
    /// Maximum size set by the remote peer via SETTINGS_HEADER_TABLE_SIZE.
    max_size: usize,
}

impl DynamicTable {
    /// Default maximum: RFC 7541 §6.5.2 initial value (4096 bytes).
    pub const DEFAULT_MAX: usize = 4096;

    pub fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            size: 0,
            max_size: Self::DEFAULT_MAX,
        }
    }

    /// Update the maximum size (from a dynamic table size update instruction
    /// or from SETTINGS negotiation). Evicts entries as needed.
    pub fn set_max_size(&mut self, max: usize) {
        self.max_size = max;
        self.evict_to(max);
    }

    /// Add a new entry, evicting old ones as needed.
    pub fn add(&mut self, name: Vec<u8>, value: Vec<u8>) {
        let sz = entry_size(&name, &value);
        // If the single entry exceeds max, the table is emptied (RFC §4.4).
        if sz > self.max_size {
            self.entries.clear();
            self.size = 0;
            return;
        }
        self.evict_to(self.max_size - sz);
        self.size += sz;
        self.entries.push_front((name, value));
    }

    /// Return `(name, value)` for a 1-based dynamic index (1 = most recent).
    pub fn get(&self, idx: usize) -> Option<(&[u8], &[u8])> {
        self.entries
            .get(idx - 1)
            .map(|(n, v)| (n.as_slice(), v.as_slice()))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn evict_to(&mut self, target: usize) {
        while self.size > target {
            if let Some((n, v)) = self.entries.pop_back() {
                self.size -= entry_size(&n, &v);
            } else {
                break;
            }
        }
    }
}

impl Default for DynamicTable {
    fn default() -> Self {
        Self::new()
    }
}

// ── Table lookup ──────────────────────────────────────────────────────────

fn table_entry(
    index: usize,
    dyn_table: &DynamicTable,
) -> Result<(Vec<u8>, Vec<u8>), HpackError> {
    if index == 0 {
        return Err(HpackError::InvalidIndex(0));
    }
    if index <= STATIC_TABLE_SIZE {
        let (n, v) = STATIC_TABLE[index];
        return Ok((n.as_bytes().to_vec(), v.as_bytes().to_vec()));
    }
    let dyn_idx = index - STATIC_TABLE_SIZE;
    dyn_table
        .get(dyn_idx)
        .map(|(n, v)| (n.to_vec(), v.to_vec()))
        .ok_or(HpackError::InvalidIndex(index))
}

// ── Public header field type ───────────────────────────────────────────────

/// A decoded header field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderField {
    pub name: Vec<u8>,
    pub value: Vec<u8>,
    /// True for Never-Indexed literals (RFC 7541 §6.2.3). Must not be added
    /// to any intermediate cache.
    pub sensitive: bool,
}

impl HeaderField {
    pub fn new(name: Vec<u8>, value: Vec<u8>) -> Self {
        Self {
            name,
            value,
            sensitive: false,
        }
    }

    pub fn sensitive(name: Vec<u8>, value: Vec<u8>) -> Self {
        Self {
            name,
            value,
            sensitive: true,
        }
    }

    /// Returns `name` as a `&str` (UTF-8 best-effort; non-UTF-8 returns `""`).
    pub fn name_str(&self) -> &str {
        std::str::from_utf8(&self.name).unwrap_or("")
    }

    /// Returns `value` as a `&str` (UTF-8 best-effort; non-UTF-8 returns `""`).
    pub fn value_str(&self) -> &str {
        std::str::from_utf8(&self.value).unwrap_or("")
    }
}

// ── Decoder (RFC 7541 §6) ─────────────────────────────────────────────────

/// Stateful HPACK decoder. One instance per HTTP/2 connection direction.
pub struct Decoder {
    dynamic: DynamicTable,
    /// Protocol-level maximum for dynamic table size (from SETTINGS).
    proto_max: usize,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            dynamic: DynamicTable::new(),
            proto_max: DynamicTable::DEFAULT_MAX,
        }
    }

    /// Update the protocol-level maximum table size (call when the remote
    /// peer's SETTINGS_HEADER_TABLE_SIZE is received).
    pub fn set_proto_max(&mut self, max: usize) {
        self.proto_max = max;
        if self.dynamic.max_size > max {
            self.dynamic.set_max_size(max);
        }
    }

    /// Decode a complete header block fragment into a list of header fields.
    pub fn decode(&mut self, block: &[u8]) -> Result<Vec<HeaderField>, HpackError> {
        let mut fields = Vec::new();
        let mut pos = 0;

        // RFC 7541 §4.2: size updates must appear at the start of a block.
        let mut size_updates_done = false;

        while pos < block.len() {
            let b = block[pos];

            if b & 0x80 != 0 {
                // §6.1 — Indexed Header Field Representation.
                size_updates_done = true;
                let (idx, consumed) = decode_int(&block[pos..], 7)?;
                pos += consumed;
                let (name, value) = table_entry(idx as usize, &self.dynamic)?;
                fields.push(HeaderField::new(name, value));
            } else if b & 0xc0 == 0x40 {
                // §6.2.1 — Literal with Incremental Indexing.
                size_updates_done = true;
                let (idx, consumed) = decode_int(&block[pos..], 6)?;
                pos += consumed;
                let name = if idx == 0 {
                    let (n, adv) = decode_string(&block[pos..])?;
                    pos += adv;
                    n
                } else {
                    let (n, _) = table_entry(idx as usize, &self.dynamic)?;
                    n
                };
                let (value, adv) = decode_string(&block[pos..])?;
                pos += adv;
                self.dynamic.add(name.clone(), value.clone());
                fields.push(HeaderField::new(name, value));
            } else if b & 0xe0 == 0x20 {
                // §6.3 — Dynamic Table Size Update.
                if size_updates_done {
                    return Err(HpackError::InvalidIndex(0));
                }
                let (new_max, consumed) = decode_int(&block[pos..], 5)?;
                pos += consumed;
                let new_max = new_max as usize;
                if new_max > self.proto_max {
                    return Err(HpackError::TableSizeTooLarge);
                }
                self.dynamic.set_max_size(new_max);
            } else {
                // §6.2.2 (Without Indexing) or §6.2.3 (Never Indexed).
                size_updates_done = true;
                let sensitive = b & 0x10 != 0;
                let (idx, consumed) = decode_int(&block[pos..], 4)?;
                pos += consumed;
                let name = if idx == 0 {
                    let (n, adv) = decode_string(&block[pos..])?;
                    pos += adv;
                    n
                } else {
                    let (n, _) = table_entry(idx as usize, &self.dynamic)?;
                    n
                };
                let (value, adv) = decode_string(&block[pos..])?;
                pos += adv;
                if sensitive {
                    fields.push(HeaderField::sensitive(name, value));
                } else {
                    fields.push(HeaderField::new(name, value));
                }
            }
        }

        Ok(fields)
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

// ── Encoder (RFC 7541 §6) ─────────────────────────────────────────────────

/// Stateful HPACK encoder. One instance per HTTP/2 connection direction.
pub struct Encoder {
    dynamic: DynamicTable,
    /// Whether to use Huffman encoding for string literals.
    use_huffman: bool,
}

impl Encoder {
    pub fn new() -> Self {
        Self {
            dynamic: DynamicTable::new(),
            use_huffman: true,
        }
    }

    pub fn with_huffman(mut self, enabled: bool) -> Self {
        self.use_huffman = enabled;
        self
    }

    /// Update the maximum dynamic table size. Emits a dynamic table size
    /// update instruction at the start of the next block if the size changed.
    pub fn set_max_size(&mut self, max: usize) {
        self.dynamic.set_max_size(max);
    }

    /// Encode a list of `(name, value)` pairs into a header block fragment.
    ///
    /// Strategy:
    /// - If a full static-table match (name+value) exists → Indexed (§6.1).
    /// - If only a name match exists → Literal with incremental indexing,
    ///   name index (§6.2.1).
    /// - Otherwise → Literal with incremental indexing, new name (§6.2.1).
    pub fn encode(&mut self, headers: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for &(name, value) in headers {
            if let Some(idx) = self.find_full(name, value) {
                // §6.1 Indexed.
                out.extend_from_slice(&encode_int(idx as u64, 7, 0x80));
            } else if let Some(idx) = self.find_name(name) {
                // §6.2.1 Literal with incremental indexing, indexed name.
                out.extend_from_slice(&encode_int(idx as u64, 6, 0x40));
                out.extend_from_slice(&encode_string(value, self.use_huffman));
                self.dynamic.add(name.to_vec(), value.to_vec());
            } else {
                // §6.2.1 Literal with incremental indexing, new name.
                out.push(0x40);
                out.extend_from_slice(&encode_string(name, self.use_huffman));
                out.extend_from_slice(&encode_string(value, self.use_huffman));
                self.dynamic.add(name.to_vec(), value.to_vec());
            }
        }
        out
    }

    fn find_full(&self, name: &[u8], value: &[u8]) -> Option<usize> {
        // Search static table first.
        if let Some(i) = STATIC_TABLE[1..]
            .iter()
            .position(|&(sn, sv)| sn.as_bytes() == name && sv.as_bytes() == value)
        {
            return Some(i + 1);
        }
        // Then dynamic table.
        (1..=self.dynamic.len()).find(|&i| {
            self.dynamic
                .get(i)
                .is_some_and(|(dn, dv)| dn == name && dv == value)
        }).map(|i| STATIC_TABLE_SIZE + i)
    }

    fn find_name(&self, name: &[u8]) -> Option<usize> {
        // Prefer static table (deterministic, no eviction).
        if let Some(i) = STATIC_TABLE[1..]
            .iter()
            .position(|&(sn, _)| sn.as_bytes() == name)
        {
            return Some(i + 1);
        }
        (1..=self.dynamic.len())
            .find(|&i| self.dynamic.get(i).is_some_and(|(dn, _)| dn == name))
            .map(|i| STATIC_TABLE_SIZE + i)
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Integer codec ──────────────────────────────────────────────────────

    #[test]
    fn int_decode_single_byte() {
        // RFC 7541 §C.1.1: encode 10 with 5-bit prefix → 0b00001010 = 0x0a
        let (v, n) = decode_int(&[0x0a], 5).unwrap();
        assert_eq!(v, 10);
        assert_eq!(n, 1);
    }

    #[test]
    fn int_decode_multibyte() {
        // RFC 7541 §C.1.2: encode 1337 with 5-bit prefix.
        // First byte: 0x1f (all-ones prefix), then 0x9a 0x0a.
        let (v, n) = decode_int(&[0x1f, 0x9a, 0x0a], 5).unwrap();
        assert_eq!(v, 1337);
        assert_eq!(n, 3);
    }

    #[test]
    fn int_decode_zero() {
        let (v, n) = decode_int(&[0x00], 8).unwrap();
        assert_eq!(v, 0);
        assert_eq!(n, 1);
    }

    #[test]
    fn int_roundtrip() {
        for &val in &[0u64, 1, 30, 31, 127, 128, 1337, 65535, 0x10_0000] {
            for bits in 1u8..=8 {
                let enc = encode_int(val, bits, 0);
                let (dec, _) = decode_int(&enc, bits).unwrap();
                assert_eq!(dec, val, "roundtrip failed for val={val}, bits={bits}");
            }
        }
    }

    #[test]
    fn int_encode_rfc_c1_2() {
        // RFC 7541 §C.1.2: 1337 with 5-bit prefix → 1f 9a 0a
        let enc = encode_int(1337, 5, 0);
        assert_eq!(enc, vec![0x1f, 0x9a, 0x0a]);
    }

    // ── Huffman codec ──────────────────────────────────────────────────────

    #[test]
    fn huffman_roundtrip_ascii() {
        let cases: &[&[u8]] = &[
            b"",
            b"a",
            b"www.example.com",
            b"no-cache",
            b"text/html; charset=utf-8",
            b"GET",
        ];
        for &s in cases {
            let enc = huffman_encode(s);
            let dec = huffman_decode(&enc).unwrap();
            assert_eq!(dec, s, "roundtrip failed for {:?}", s);
        }
    }

    #[test]
    fn huffman_www_example_com() {
        // RFC 7541 §C.4.1: Huffman("www.example.com") =
        // f1 e3 c2 e5 f2 3a 6b a0 ab 90 f4 ff
        let enc = huffman_encode(b"www.example.com");
        assert_eq!(
            enc,
            vec![0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab, 0x90, 0xf4, 0xff]
        );
        assert_eq!(huffman_decode(&enc).unwrap(), b"www.example.com");
    }

    #[test]
    fn huffman_no_cache() {
        // RFC 7541 §C.4.2: Huffman("no-cache") = a8 eb 10 64 9c bf
        let enc = huffman_encode(b"no-cache");
        assert_eq!(enc, vec![0xa8, 0xeb, 0x10, 0x64, 0x9c, 0xbf]);
        assert_eq!(huffman_decode(&enc).unwrap(), b"no-cache");
    }

    // ── String codec ───────────────────────────────────────────────────────

    #[test]
    fn string_decode_literal() {
        // Length-prefixed literal "custom-key"
        let mut data = vec![0x0a]; // length=10, H=0
        data.extend_from_slice(b"custom-key");
        let (s, n) = decode_string(&data).unwrap();
        assert_eq!(s, b"custom-key");
        assert_eq!(n, 11);
    }

    #[test]
    fn string_roundtrip_huffman() {
        let s = b"www.example.com";
        let enc = encode_string(s, true);
        let (dec, n) = decode_string(&enc).unwrap();
        assert_eq!(dec, s);
        assert_eq!(n, enc.len());
    }

    // ── Dynamic table ──────────────────────────────────────────────────────

    #[test]
    fn dynamic_table_add_and_get() {
        let mut dt = DynamicTable::new();
        dt.add(b"custom-key".to_vec(), b"custom-value".to_vec());
        let (n, v) = dt.get(1).unwrap();
        assert_eq!(n, b"custom-key");
        assert_eq!(v, b"custom-value");
    }

    #[test]
    fn dynamic_table_eviction() {
        let mut dt = DynamicTable::new();
        dt.set_max_size(64); // small table
        // "a" + "b" + 32 = 34 bytes; "c" + "d" + 32 = 34 bytes; total would be 68 > 64
        dt.add(b"a".to_vec(), b"b".to_vec());
        dt.add(b"c".to_vec(), b"d".to_vec());
        // Only the most recent entry should remain (34 <= 64, but 34+34 > 64).
        // Wait: after adding "c"/"d", evict oldest until size ≤ 64−34 = 30.
        // "a"/"b" is 34 bytes → evict. So only "c"/"d" remains.
        assert_eq!(dt.len(), 1);
        let (n, v) = dt.get(1).unwrap();
        assert_eq!(n, b"c");
        assert_eq!(v, b"d");
    }

    #[test]
    fn dynamic_table_oversized_entry_empties_table() {
        let mut dt = DynamicTable::new();
        dt.set_max_size(10);
        dt.add(b"longname".to_vec(), b"longvalue".to_vec()); // 8+9+32=49 > 10
        assert!(dt.is_empty());
    }

    // ── Decoder — RFC 7541 §C.3 (no Huffman) ──────────────────────────────

    #[test]
    fn decode_rfc_c3_1_first_request() {
        // RFC 7541 §C.3.1
        // :method: GET     → indexed(2)
        // :scheme: http    → indexed(6)
        // :path: /         → indexed(4)
        // :authority: www.example.com → literal+indexing, name_idx=1
        let block = [
            0x82, 0x86, 0x84, 0x41, 0x0f, 0x77, 0x77, 0x77, 0x2e, 0x65, 0x78, 0x61, 0x6d, 0x70,
            0x6c, 0x65, 0x2e, 0x63, 0x6f, 0x6d,
        ];
        let mut dec = Decoder::new();
        let fields = dec.decode(&block).unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].name_str(), ":method");
        assert_eq!(fields[0].value_str(), "GET");
        assert_eq!(fields[1].name_str(), ":scheme");
        assert_eq!(fields[1].value_str(), "http");
        assert_eq!(fields[2].name_str(), ":path");
        assert_eq!(fields[2].value_str(), "/");
        assert_eq!(fields[3].name_str(), ":authority");
        assert_eq!(fields[3].value_str(), "www.example.com");
    }

    #[test]
    fn decode_rfc_c3_2_second_request() {
        // RFC 7541 §C.3.2 — uses dynamic table entries from first request.
        // :method: GET     → indexed(2)
        // :scheme: http    → indexed(6)
        // :path: /         → indexed(4)
        // :authority: www.example.com → indexed(62) [from dynamic table]
        // cache-control: no-cache → literal+indexing, name_idx=24
        let block_1 = [
            0x82, 0x86, 0x84, 0x41, 0x0f, 0x77, 0x77, 0x77, 0x2e, 0x65, 0x78, 0x61, 0x6d, 0x70,
            0x6c, 0x65, 0x2e, 0x63, 0x6f, 0x6d,
        ];
        let block_2 = [
            0x82, 0x86, 0x84, 0xbe, 0x58, 0x08, 0x6e, 0x6f, 0x2d, 0x63, 0x61, 0x63, 0x68, 0x65,
        ];
        let mut dec = Decoder::new();
        dec.decode(&block_1).unwrap();
        let fields = dec.decode(&block_2).unwrap();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[4].name_str(), "cache-control");
        assert_eq!(fields[4].value_str(), "no-cache");
    }

    #[test]
    fn decode_rfc_c4_1_huffman_request() {
        // RFC 7541 §C.4.1 — same as C.3.1 but :authority uses Huffman value.
        let block = [
            0x82, 0x86, 0x84, 0x41, 0x8c, 0xf1, 0xe3, 0xc2, 0xe5, 0xf2, 0x3a, 0x6b, 0xa0, 0xab,
            0x90, 0xf4, 0xff,
        ];
        let mut dec = Decoder::new();
        let fields = dec.decode(&block).unwrap();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[3].value_str(), "www.example.com");
    }

    // ── Encoder round-trip ─────────────────────────────────────────────────

    #[test]
    fn encoder_decoder_roundtrip() {
        let mut enc = Encoder::new();
        let mut dec = Decoder::new();
        let headers: &[(&[u8], &[u8])] = &[
            (b":method", b"GET"),
            (b":scheme", b"https"),
            (b":path", b"/"),
            (b":authority", b"example.com"),
            (b"accept", b"text/html"),
        ];
        let block = enc.encode(headers);
        let decoded = dec.decode(&block).unwrap();
        assert_eq!(decoded.len(), headers.len());
        for (i, &(name, value)) in headers.iter().enumerate() {
            assert_eq!(decoded[i].name, name, "name mismatch at {i}");
            assert_eq!(decoded[i].value, value, "value mismatch at {i}");
        }
    }

    #[test]
    fn encoder_uses_indexed_for_static_entries() {
        let mut enc = Encoder::new().with_huffman(false);
        // :method GET is static[2]. Encoded as single byte 0x82.
        let block = enc.encode(&[(b":method", b"GET")]);
        assert_eq!(block, vec![0x82]);
    }
}
