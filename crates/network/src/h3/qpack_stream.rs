//! QPACK dynamic table + encoder/decoder instruction streams — RFC 9204
//! §3.2, §4.3, §4.4.
//!
//! The field-section codec in [`super::qpack`] is deliberately static-only: it
//! is the wire behaviour of a peer advertising `SETTINGS_QPACK_MAX_TABLE_
//! CAPACITY = 0`. This module adds the other half of QPACK — the shared
//! **dynamic table** ([`DynamicTable`], RFC 9204 §3.2) and the two
//! unidirectional instruction streams that mutate and track it:
//!
//! - the **encoder stream** (RFC 9204 §4.3), carrying [`EncoderInstruction`]s
//!   from the encoder to the decoder — Set Dynamic Table Capacity, Insert With
//!   Name Reference, Insert With Literal Name, and Duplicate;
//! - the **decoder stream** (RFC 9204 §4.4), carrying [`DecoderInstruction`]s
//!   back — Section Acknowledgment, Stream Cancellation, and Insert Count
//!   Increment.
//!
//! Everything here is a pure codec plus an in-memory table: parse/serialize of
//! the instruction wire forms and the eviction/indexing arithmetic of the
//! table. There is no IO, no unidirectional-stream framing, and no wiring into
//! the request path — those belong to the connection layer in a later slice.
//! Applying a parsed encoder-instruction stream to a [`DynamicTable`] with
//! [`DynamicTable::apply`] realises the exact table state a peer's encoder
//! built, which is what a future field-section decoder will index into.
//!
//! ## Indexing (RFC 9204 §3.2.4–§3.2.6)
//!
//! The table assigns each inserted entry a monotonic **absolute index**,
//! starting at 0. The Insert Count is the number of insertions performed, so
//! the most recently inserted entry has absolute index `insert_count - 1`.
//! Encoder-stream instructions (Insert With Name Reference into the dynamic
//! table, and Duplicate) use **relative** indexing, where relative index 0 is
//! the most recent entry: `absolute = insert_count - 1 - relative`. Eviction
//! drops the oldest entries (lowest absolute index) first.

use super::qpack::{decode_string, encode_string, static_entry};
use crate::h2::hpack::{HpackError, decode_int, encode_int};
use std::collections::VecDeque;

// ── Error codes (RFC 9204 §6) ──────────────────────────────────────────────

/// `QPACK_ENCODER_STREAM_ERROR` (RFC 9204 §6) — the encoder stream (or the
/// dynamic-table state it drives) is malformed. A connection error.
pub const QPACK_ENCODER_STREAM_ERROR: u64 = 0x0201;

/// `QPACK_DECODER_STREAM_ERROR` (RFC 9204 §6) — the decoder stream is
/// malformed. A connection error.
pub const QPACK_DECODER_STREAM_ERROR: u64 = 0x0202;

/// Per-entry overhead in the dynamic-table size accounting (RFC 9204 §3.2.1):
/// `size = name.len() + value.len() + 32`.
pub const ENTRY_OVERHEAD: usize = 32;

// ── Error ──────────────────────────────────────────────────────────────────

/// An error decoding an instruction stream or mutating the dynamic table.
///
/// [`QpackStreamError::code`] maps each variant to its RFC 9204 §6 wire error
/// code; use [`QpackStreamError::decoder_code`] instead when the error arose
/// while parsing the *decoder* stream (the same generic parse failures can
/// occur on either side, and only the caller knows which stream it read).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QpackStreamError {
    /// Input ended in the middle of an instruction.
    UnexpectedEof,
    /// A prefixed integer exceeded the 2^32−1 implementation limit.
    IntegerOverflow,
    /// A Huffman-coded string held an invalid or incomplete code.
    InvalidHuffman,
    /// A string length field claimed more bytes than remain in the input.
    StringTooLong,
    /// An Insert With Name Reference used a static index out of range.
    InvalidStaticIndex(u64),
    /// A relative index referenced an entry that is not in the table (either
    /// already evicted or never inserted).
    InvalidRelativeIndex(u64),
    /// Set Dynamic Table Capacity asked for more than the maximum the decoder
    /// advertised (RFC 9204 §3.2.3).
    CapacityExceedsMaximum(u64),
    /// An insert produced an entry larger than the current table capacity
    /// (RFC 9204 §3.2.2) — it can never fit, so it is a stream error.
    EntryTooLarge,
    /// A Section Acknowledgment named a stream with no unacknowledged field
    /// section (RFC 9204 §4.4.1). Raised on the encoder while reading the
    /// *decoder* stream; use [`QpackStreamError::decoder_code`].
    UnexpectedSectionAck(u64),
    /// An Insert Count Increment raised the Known Received Count beyond the
    /// encoder's Insert Count (RFC 9204 §4.4.3). Raised on the encoder while
    /// reading the *decoder* stream; use [`QpackStreamError::decoder_code`].
    InsertCountIncrementOverflow(u64),
}

impl QpackStreamError {
    /// The RFC 9204 §6 wire error code when the failure arose on the **encoder**
    /// stream (the common case: every table-mutating instruction is read there).
    /// The two encoder-side decoder-stream faults
    /// ([`UnexpectedSectionAck`](Self::UnexpectedSectionAck),
    /// [`InsertCountIncrementOverflow`](Self::InsertCountIncrementOverflow))
    /// always originate on the decoder stream and so report the decoder code.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::UnexpectedSectionAck(_) | Self::InsertCountIncrementOverflow(_) => {
                QPACK_DECODER_STREAM_ERROR
            }
            _ => QPACK_ENCODER_STREAM_ERROR,
        }
    }

    /// The RFC 9204 §6 wire error code when the failure arose on the **decoder**
    /// stream. Only the generic parse variants (`UnexpectedEof`,
    /// `IntegerOverflow`) can occur there.
    #[must_use]
    pub const fn decoder_code(&self) -> u64 {
        QPACK_DECODER_STREAM_ERROR
    }
}

impl core::fmt::Display for QpackStreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "QPACK stream: unexpected EOF"),
            Self::IntegerOverflow => write!(f, "QPACK stream: prefixed integer overflow"),
            Self::InvalidHuffman => write!(f, "QPACK stream: invalid Huffman sequence"),
            Self::StringTooLong => write!(f, "QPACK stream: string length exceeds input"),
            Self::InvalidStaticIndex(i) => write!(f, "QPACK stream: static index {i} out of range"),
            Self::InvalidRelativeIndex(i) => {
                write!(f, "QPACK stream: relative index {i} not in dynamic table")
            }
            Self::CapacityExceedsMaximum(c) => {
                write!(f, "QPACK stream: capacity {c} exceeds advertised maximum")
            }
            Self::EntryTooLarge => write!(f, "QPACK stream: entry larger than table capacity"),
            Self::UnexpectedSectionAck(id) => {
                write!(f, "QPACK stream: Section Acknowledgment for stream {id} with no outstanding section")
            }
            Self::InsertCountIncrementOverflow(n) => {
                write!(f, "QPACK stream: Insert Count Increment {n} exceeds insert count")
            }
        }
    }
}

impl std::error::Error for QpackStreamError {}

/// Map the shared HPACK integer-codec error into the stream error space. The
/// index-specific variants are unreachable from [`decode_int`] and collapse to
/// [`QpackStreamError::UnexpectedEof`].
fn from_hpack(e: HpackError) -> QpackStreamError {
    match e {
        HpackError::UnexpectedEof => QpackStreamError::UnexpectedEof,
        HpackError::IntegerOverflow => QpackStreamError::IntegerOverflow,
        HpackError::InvalidHuffman => QpackStreamError::InvalidHuffman,
        HpackError::StringTooLong => QpackStreamError::StringTooLong,
        HpackError::InvalidIndex(_) | HpackError::TableSizeTooLarge => {
            QpackStreamError::UnexpectedEof
        }
    }
}

/// Translate a field-section string-codec error (from the shared
/// [`decode_string`]) into the stream error space.
fn from_qpack(e: super::qpack::QpackError) -> QpackStreamError {
    use super::qpack::QpackError;
    match e {
        QpackError::UnexpectedEof => QpackStreamError::UnexpectedEof,
        QpackError::IntegerOverflow => QpackStreamError::IntegerOverflow,
        QpackError::InvalidHuffman => QpackStreamError::InvalidHuffman,
        QpackError::StringTooLong => QpackStreamError::StringTooLong,
        QpackError::InvalidStaticIndex(i) => QpackStreamError::InvalidStaticIndex(i),
        // Field-section-only variants: unreachable from the string codec this
        // mapping serves, so they collapse to a generic parse failure.
        QpackError::DynamicUnsupported
        | QpackError::NonZeroRequiredInsertCount(_)
        | QpackError::InvalidRequiredInsertCount(_)
        | QpackError::InvalidDynamicReference(_) => QpackStreamError::UnexpectedEof,
    }
}

// ── Dynamic table (RFC 9204 §3.2) ──────────────────────────────────────────

/// The QPACK dynamic table: a FIFO of `(name, value)` entries with a
/// byte-budget capacity and monotonic absolute indexing (RFC 9204 §3.2).
///
/// Oldest entries occupy the lowest absolute indices and are evicted first.
/// The table never exceeds its current [`capacity`](DynamicTable::capacity),
/// which the encoder may raise (up to the advertised maximum) or lower via the
/// Set Dynamic Table Capacity instruction.
#[derive(Clone, Debug)]
pub struct DynamicTable {
    /// Live entries, front = oldest (lowest absolute index), back = newest.
    entries: VecDeque<(Vec<u8>, Vec<u8>)>,
    /// Current capacity in bytes (`≤ max_capacity`).
    capacity: usize,
    /// The maximum capacity the decoder advertised (`SETTINGS_QPACK_MAX_TABLE_
    /// CAPACITY`); the encoder may not set a capacity above this.
    max_capacity: usize,
    /// Current total size of all live entries (RFC 9204 §3.2.1 accounting).
    size: usize,
    /// Number of insertions ever performed = absolute index of the next insert.
    insert_count: u64,
}

impl DynamicTable {
    /// Create an empty table whose capacity starts at 0 and may be raised up to
    /// `max_capacity` bytes (the decoder's advertised maximum).
    #[must_use]
    pub fn new(max_capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: 0,
            max_capacity,
            size: 0,
            insert_count: 0,
        }
    }

    /// The size in bytes an entry occupies (RFC 9204 §3.2.1).
    #[must_use]
    pub fn entry_size(name: &[u8], value: &[u8]) -> usize {
        name.len() + value.len() + ENTRY_OVERHEAD
    }

    /// The current capacity in bytes.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The total size in bytes of all live entries.
    #[must_use]
    pub fn size(&self) -> usize {
        self.size
    }

    /// The number of entries currently in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table currently holds no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The Insert Count — the number of insertions performed so far, equal to
    /// the absolute index the next inserted entry will receive (RFC 9204
    /// §3.2.4).
    #[must_use]
    pub fn insert_count(&self) -> u64 {
        self.insert_count
    }

    /// `MaxEntries = floor(MaxTableCapacity / 32)` (RFC 9204 §3.2.2), the value
    /// the field-section prefix uses to encode the Required Insert Count in a
    /// wrapped, bounded form (RFC 9204 §4.5.1.1).
    #[must_use]
    pub fn max_entries(&self) -> u64 {
        (self.max_capacity / ENTRY_OVERHEAD) as u64
    }

    /// The absolute index of the most recent live entry whose name and value
    /// both match, or `None`. Searching newest-first keeps the resulting
    /// Required Insert Count for a field section as small as possible.
    #[must_use]
    pub fn find_absolute(&self, name: &[u8], value: &[u8]) -> Option<u64> {
        self.entries
            .iter()
            .rposition(|(n, v)| n.as_slice() == name && v.as_slice() == value)
            .map(|pos| self.dropped_count() + pos as u64)
    }

    /// The absolute index of the most recent live entry whose name matches, for
    /// use as a name reference when no full match exists, or `None`.
    #[must_use]
    pub fn find_name_absolute(&self, name: &[u8]) -> Option<u64> {
        self.entries
            .iter()
            .rposition(|(n, _)| n.as_slice() == name)
            .map(|pos| self.dropped_count() + pos as u64)
    }

    /// The lowest absolute index still present (entries below this were
    /// evicted). Equal to [`insert_count`](Self::insert_count) when empty.
    fn dropped_count(&self) -> u64 {
        self.insert_count - self.entries.len() as u64
    }

    /// Look up an entry by its absolute index (RFC 9204 §3.2.4). Returns `None`
    /// if the index was evicted or never inserted.
    #[must_use]
    pub fn get_absolute(&self, absolute: u64) -> Option<(&[u8], &[u8])> {
        if absolute < self.dropped_count() || absolute >= self.insert_count {
            return None;
        }
        let pos = (absolute - self.dropped_count()) as usize;
        self.entries.get(pos).map(|(n, v)| (n.as_slice(), v.as_slice()))
    }

    /// Resolve a relative index (0 = most recently inserted entry) to an
    /// absolute index (RFC 9204 §3.2.5), or an error if it points past the
    /// oldest live entry.
    fn relative_to_absolute(&self, relative: u64) -> Result<u64, QpackStreamError> {
        // Most recent absolute index is insert_count - 1; relative counts back.
        self.insert_count
            .checked_sub(1)
            .and_then(|newest| newest.checked_sub(relative))
            .filter(|&abs| abs >= self.dropped_count())
            .ok_or(QpackStreamError::InvalidRelativeIndex(relative))
    }

    /// Set the table capacity (RFC 9204 §3.2.3 / the Set Dynamic Table Capacity
    /// instruction). Evicts entries as needed to fit the new capacity.
    ///
    /// # Errors
    ///
    /// [`QpackStreamError::CapacityExceedsMaximum`] if `capacity` is above the
    /// advertised maximum.
    pub fn set_capacity(&mut self, capacity: u64) -> Result<(), QpackStreamError> {
        let capacity = usize::try_from(capacity)
            .ok()
            .filter(|&c| c <= self.max_capacity)
            .ok_or(QpackStreamError::CapacityExceedsMaximum(capacity))?;
        self.capacity = capacity;
        self.evict_to(capacity);
        Ok(())
    }

    /// Evict oldest entries until the total size is at most `target` bytes.
    fn evict_to(&mut self, target: usize) {
        while self.size > target {
            if let Some((n, v)) = self.entries.pop_front() {
                self.size -= Self::entry_size(&n, &v);
            } else {
                // size accounting guarantees this is unreachable, but never spin.
                break;
            }
        }
    }

    /// Insert a `(name, value)` entry, evicting older entries to make room
    /// (RFC 9204 §3.2.2). Returns the new entry's absolute index.
    ///
    /// # Errors
    ///
    /// [`QpackStreamError::EntryTooLarge`] if the entry cannot fit even in an
    /// otherwise-empty table at the current capacity.
    pub fn insert(&mut self, name: Vec<u8>, value: Vec<u8>) -> Result<u64, QpackStreamError> {
        let sz = Self::entry_size(&name, &value);
        if sz > self.capacity {
            return Err(QpackStreamError::EntryTooLarge);
        }
        // Free enough room, then append. `capacity - sz` cannot underflow given
        // the check above.
        self.evict_to(self.capacity - sz);
        let absolute = self.insert_count;
        self.entries.push_back((name, value));
        self.size += sz;
        self.insert_count += 1;
        Ok(absolute)
    }

    /// Apply a parsed [`EncoderInstruction`] to the table, resolving name and
    /// duplicate references (RFC 9204 §4.3). Returns the absolute index of the
    /// newly inserted entry for the three insert forms, or `None` for Set
    /// Dynamic Table Capacity.
    ///
    /// # Errors
    ///
    /// Propagates index/capacity/size errors from the underlying operations.
    pub fn apply(
        &mut self,
        instr: &EncoderInstruction,
    ) -> Result<Option<u64>, QpackStreamError> {
        match instr {
            EncoderInstruction::SetCapacity(cap) => {
                self.set_capacity(*cap)?;
                Ok(None)
            }
            EncoderInstruction::InsertWithNameRef { dynamic, index, value } => {
                let name = if *dynamic {
                    let abs = self.relative_to_absolute(*index)?;
                    self.get_absolute(abs)
                        .ok_or(QpackStreamError::InvalidRelativeIndex(*index))?
                        .0
                        .to_vec()
                } else {
                    static_entry(*index).map_err(from_qpack)?.0.as_bytes().to_vec()
                };
                self.insert(name, value.clone()).map(Some)
            }
            EncoderInstruction::InsertWithLiteralName { name, value } => {
                self.insert(name.clone(), value.clone()).map(Some)
            }
            EncoderInstruction::Duplicate(rel) => {
                let abs = self.relative_to_absolute(*rel)?;
                let (name, value) = self
                    .get_absolute(abs)
                    .ok_or(QpackStreamError::InvalidRelativeIndex(*rel))?;
                let (name, value) = (name.to_vec(), value.to_vec());
                self.insert(name, value).map(Some)
            }
        }
    }
}

// ── Encoder instructions (RFC 9204 §4.3) ───────────────────────────────────

/// An instruction on the QPACK encoder stream (RFC 9204 §4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncoderInstruction {
    /// Set Dynamic Table Capacity (§4.3.1): `001` + Capacity(5+).
    SetCapacity(u64),
    /// Insert With Name Reference (§4.3.2): `1 T` + Name Index(6+) + Value.
    /// `dynamic` is the inverse of the `T` bit (`T = 1` ⇒ static table).
    InsertWithNameRef {
        /// Whether `index` is a relative dynamic-table index (`T = 0`) rather
        /// than a static-table index (`T = 1`).
        dynamic: bool,
        /// Static index, or relative dynamic index, of the name to reuse.
        index: u64,
        /// The literal value bytes for the new entry.
        value: Vec<u8>,
    },
    /// Insert With Literal Name (§4.3.3): `01 H` + Name Length(5+) + Value.
    InsertWithLiteralName {
        /// The literal name bytes for the new entry.
        name: Vec<u8>,
        /// The literal value bytes for the new entry.
        value: Vec<u8>,
    },
    /// Duplicate (§4.3.4): `000` + Index(5+), a relative dynamic-table index.
    Duplicate(u64),
}

impl EncoderInstruction {
    /// Serialize this instruction onto `out`. `use_huffman` enables Huffman
    /// coding of literal names/values when it does not enlarge them.
    pub fn encode(&self, out: &mut Vec<u8>, use_huffman: bool) {
        match self {
            Self::SetCapacity(cap) => {
                // §4.3.1: `0 0 1` pattern, 5-bit prefix integer.
                out.extend_from_slice(&encode_int(*cap, 5, 0x20));
            }
            Self::InsertWithNameRef { dynamic, index, value } => {
                // §4.3.2: `1 T Name Index(6+)`. T = 1 ⇒ static ⇒ !dynamic.
                let t_bit = if *dynamic { 0x00 } else { 0x40 };
                out.extend_from_slice(&encode_int(*index, 6, 0x80 | t_bit));
                // Value string: `H Value Length(7+)`.
                encode_string(out, value, 7, 0x00, use_huffman);
            }
            Self::InsertWithLiteralName { name, value } => {
                // §4.3.3: `0 1 H Name Length(5+)`, H at bit 5, type bit 6 set.
                encode_string(out, name, 5, 0x40, use_huffman);
                encode_string(out, value, 7, 0x00, use_huffman);
            }
            Self::Duplicate(index) => {
                // §4.3.4: `0 0 0 Index(5+)`.
                out.extend_from_slice(&encode_int(*index, 5, 0x00));
            }
        }
    }

    /// Parse a single instruction from the front of `src`, returning it and the
    /// number of bytes consumed.
    ///
    /// # Errors
    ///
    /// [`QpackStreamError`] on a truncated or malformed instruction.
    pub fn decode(src: &[u8]) -> Result<(Self, usize), QpackStreamError> {
        let first = *src.first().ok_or(QpackStreamError::UnexpectedEof)?;
        if first & 0x80 != 0 {
            // §4.3.2 Insert With Name Reference: `1 T Name Index(6+)`.
            let dynamic = first & 0x40 == 0;
            let (index, n1) = decode_int(src, 6).map_err(from_hpack)?;
            let (value, n2) = decode_string(&src[n1..], 7).map_err(from_qpack)?;
            Ok((Self::InsertWithNameRef { dynamic, index, value }, n1 + n2))
        } else if first & 0x40 != 0 {
            // §4.3.3 Insert With Literal Name: `0 1 H Name Length(5+)`.
            let (name, n1) = decode_string(src, 5).map_err(from_qpack)?;
            let (value, n2) = decode_string(&src[n1..], 7).map_err(from_qpack)?;
            Ok((Self::InsertWithLiteralName { name, value }, n1 + n2))
        } else if first & 0x20 != 0 {
            // §4.3.1 Set Dynamic Table Capacity: `0 0 1 Capacity(5+)`.
            let (cap, n) = decode_int(src, 5).map_err(from_hpack)?;
            Ok((Self::SetCapacity(cap), n))
        } else {
            // §4.3.4 Duplicate: `0 0 0 Index(5+)`.
            let (index, n) = decode_int(src, 5).map_err(from_hpack)?;
            Ok((Self::Duplicate(index), n))
        }
    }
}

/// Decode a full encoder-stream buffer into a list of instructions.
///
/// # Errors
///
/// [`QpackStreamError`] on the first malformed instruction.
pub fn decode_encoder_stream(
    buf: &[u8],
) -> Result<Vec<EncoderInstruction>, QpackStreamError> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < buf.len() {
        let (instr, consumed) = EncoderInstruction::decode(&buf[pos..])?;
        pos += consumed;
        out.push(instr);
    }
    Ok(out)
}

// ── Decoder instructions (RFC 9204 §4.4) ───────────────────────────────────

/// An instruction on the QPACK decoder stream (RFC 9204 §4.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecoderInstruction {
    /// Section Acknowledgment (§4.4.1): `1` + Stream ID(7+).
    SectionAck(u64),
    /// Stream Cancellation (§4.4.2): `01` + Stream ID(6+).
    StreamCancellation(u64),
    /// Insert Count Increment (§4.4.3): `00` + Increment(6+).
    InsertCountIncrement(u64),
}

impl DecoderInstruction {
    /// Serialize this instruction onto `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        match self {
            // §4.4.1: `1 Stream ID(7+)`.
            Self::SectionAck(id) => out.extend_from_slice(&encode_int(*id, 7, 0x80)),
            // §4.4.2: `0 1 Stream ID(6+)`.
            Self::StreamCancellation(id) => out.extend_from_slice(&encode_int(*id, 6, 0x40)),
            // §4.4.3: `0 0 Increment(6+)`.
            Self::InsertCountIncrement(n) => out.extend_from_slice(&encode_int(*n, 6, 0x00)),
        }
    }

    /// Parse a single instruction from the front of `src`, returning it and the
    /// number of bytes consumed.
    ///
    /// # Errors
    ///
    /// [`QpackStreamError`] on a truncated instruction (map to
    /// [`QpackStreamError::decoder_code`] for the wire error).
    pub fn decode(src: &[u8]) -> Result<(Self, usize), QpackStreamError> {
        let first = *src.first().ok_or(QpackStreamError::UnexpectedEof)?;
        if first & 0x80 != 0 {
            let (id, n) = decode_int(src, 7).map_err(from_hpack)?;
            Ok((Self::SectionAck(id), n))
        } else if first & 0x40 != 0 {
            let (id, n) = decode_int(src, 6).map_err(from_hpack)?;
            Ok((Self::StreamCancellation(id), n))
        } else {
            let (n_incr, n) = decode_int(src, 6).map_err(from_hpack)?;
            Ok((Self::InsertCountIncrement(n_incr), n))
        }
    }
}

/// Decode a full decoder-stream buffer into a list of instructions.
///
/// # Errors
///
/// [`QpackStreamError`] on the first malformed instruction.
pub fn decode_decoder_stream(
    buf: &[u8],
) -> Result<Vec<DecoderInstruction>, QpackStreamError> {
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < buf.len() {
        let (instr, consumed) = DecoderInstruction::decode(&buf[pos..])?;
        pos += consumed;
        out.push(instr);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Dynamic table ──────────────────────────────────────────────────────

    #[test]
    fn entry_size_includes_overhead() {
        assert_eq!(DynamicTable::entry_size(b"name", b"value"), 4 + 5 + 32);
        assert_eq!(DynamicTable::entry_size(b"", b""), 32);
    }

    #[test]
    fn insert_assigns_monotonic_absolute_indices() {
        let mut t = DynamicTable::new(4096);
        t.set_capacity(4096).unwrap();
        assert_eq!(t.insert(b"a".to_vec(), b"1".to_vec()).unwrap(), 0);
        assert_eq!(t.insert(b"b".to_vec(), b"2".to_vec()).unwrap(), 1);
        assert_eq!(t.insert_count(), 2);
        assert_eq!(t.get_absolute(0), Some((&b"a"[..], &b"1"[..])));
        assert_eq!(t.get_absolute(1), Some((&b"b"[..], &b"2"[..])));
        assert_eq!(t.get_absolute(2), None);
    }

    #[test]
    fn eviction_drops_oldest_first() {
        // Two entries of size 34 each (1+1+32); capacity 68 holds exactly two.
        let mut t = DynamicTable::new(200);
        t.set_capacity(68).unwrap();
        t.insert(b"a".to_vec(), b"1".to_vec()).unwrap(); // abs 0
        t.insert(b"b".to_vec(), b"2".to_vec()).unwrap(); // abs 1
        assert_eq!(t.len(), 2);
        t.insert(b"c".to_vec(), b"3".to_vec()).unwrap(); // abs 2 evicts abs 0
        assert_eq!(t.len(), 2);
        assert_eq!(t.get_absolute(0), None); // evicted
        assert_eq!(t.get_absolute(1), Some((&b"b"[..], &b"2"[..])));
        assert_eq!(t.get_absolute(2), Some((&b"c"[..], &b"3"[..])));
        assert_eq!(t.insert_count(), 3);
    }

    #[test]
    fn lowering_capacity_evicts() {
        let mut t = DynamicTable::new(200);
        t.set_capacity(200).unwrap();
        t.insert(b"a".to_vec(), b"1".to_vec()).unwrap(); // size 34
        t.insert(b"b".to_vec(), b"2".to_vec()).unwrap(); // size 34, total 68
        assert_eq!(t.size(), 68);
        t.set_capacity(34).unwrap(); // only room for the newest
        assert_eq!(t.len(), 1);
        assert_eq!(t.get_absolute(1), Some((&b"b"[..], &b"2"[..])));
        assert_eq!(t.size(), 34);
    }

    #[test]
    fn set_capacity_above_maximum_errors() {
        let mut t = DynamicTable::new(100);
        assert_eq!(
            t.set_capacity(101),
            Err(QpackStreamError::CapacityExceedsMaximum(101))
        );
    }

    #[test]
    fn insert_larger_than_capacity_errors() {
        let mut t = DynamicTable::new(4096);
        t.set_capacity(40).unwrap(); // room for a 40-byte entry at most
        // name+value = 9 bytes ⇒ size 41 > 40.
        assert_eq!(
            t.insert(b"nnnn".to_vec(), b"vvvvv".to_vec()),
            Err(QpackStreamError::EntryTooLarge)
        );
    }

    #[test]
    fn insert_with_zero_capacity_always_errors() {
        let mut t = DynamicTable::new(4096); // capacity starts at 0
        assert_eq!(
            t.insert(b"".to_vec(), b"".to_vec()),
            Err(QpackStreamError::EntryTooLarge)
        );
    }

    // ── Instruction application ─────────────────────────────────────────────

    #[test]
    fn apply_insert_with_static_name_ref() {
        let mut t = DynamicTable::new(4096);
        t.apply(&EncoderInstruction::SetCapacity(4096)).unwrap();
        // Static index 17 = (":method", "GET"); override the value.
        let idx = t
            .apply(&EncoderInstruction::InsertWithNameRef {
                dynamic: false,
                index: 17,
                value: b"PATCH".to_vec(),
            })
            .unwrap();
        assert_eq!(idx, Some(0));
        assert_eq!(t.get_absolute(0), Some((&b":method"[..], &b"PATCH"[..])));
    }

    #[test]
    fn apply_insert_with_dynamic_name_ref_and_duplicate() {
        let mut t = DynamicTable::new(4096);
        t.apply(&EncoderInstruction::SetCapacity(4096)).unwrap();
        t.apply(&EncoderInstruction::InsertWithLiteralName {
            name: b"x-foo".to_vec(),
            value: b"1".to_vec(),
        })
        .unwrap(); // abs 0
        // Insert reusing the name of abs 0 (relative index 0).
        t.apply(&EncoderInstruction::InsertWithNameRef {
            dynamic: true,
            index: 0,
            value: b"2".to_vec(),
        })
        .unwrap(); // abs 1, name x-foo
        assert_eq!(t.get_absolute(1), Some((&b"x-foo"[..], &b"2"[..])));
        // Duplicate the oldest live entry (relative index 1 = abs 0).
        t.apply(&EncoderInstruction::Duplicate(1)).unwrap(); // abs 2
        assert_eq!(t.get_absolute(2), Some((&b"x-foo"[..], &b"1"[..])));
        assert_eq!(t.insert_count(), 3);
    }

    #[test]
    fn apply_static_name_ref_out_of_range_errors() {
        let mut t = DynamicTable::new(4096);
        t.apply(&EncoderInstruction::SetCapacity(4096)).unwrap();
        assert_eq!(
            t.apply(&EncoderInstruction::InsertWithNameRef {
                dynamic: false,
                index: 99,
                value: b"x".to_vec(),
            }),
            Err(QpackStreamError::InvalidStaticIndex(99))
        );
    }

    #[test]
    fn apply_dynamic_ref_out_of_range_errors() {
        let mut t = DynamicTable::new(4096);
        t.apply(&EncoderInstruction::SetCapacity(4096)).unwrap();
        // No entries yet ⇒ relative index 0 is invalid.
        assert_eq!(
            t.apply(&EncoderInstruction::Duplicate(0)),
            Err(QpackStreamError::InvalidRelativeIndex(0))
        );
    }

    #[test]
    fn dynamic_ref_to_evicted_entry_errors() {
        let mut t = DynamicTable::new(200);
        t.apply(&EncoderInstruction::SetCapacity(34)).unwrap(); // one entry
        t.apply(&EncoderInstruction::InsertWithLiteralName {
            name: b"a".to_vec(),
            value: b"1".to_vec(),
        })
        .unwrap(); // abs 0
        t.apply(&EncoderInstruction::InsertWithLiteralName {
            name: b"b".to_vec(),
            value: b"2".to_vec(),
        })
        .unwrap(); // abs 1, evicts abs 0
        // Relative index 1 would be abs 0, which is evicted.
        assert_eq!(
            t.apply(&EncoderInstruction::Duplicate(1)),
            Err(QpackStreamError::InvalidRelativeIndex(1))
        );
    }

    // ── Encoder-stream codec ────────────────────────────────────────────────

    fn enc_roundtrip(instr: &EncoderInstruction, huff: bool) {
        let mut buf = Vec::new();
        instr.encode(&mut buf, huff);
        let (decoded, consumed) = EncoderInstruction::decode(&buf).unwrap();
        assert_eq!(&decoded, instr, "roundtrip (huffman={huff})");
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn encoder_instruction_roundtrips() {
        for huff in [false, true] {
            enc_roundtrip(&EncoderInstruction::SetCapacity(4096), huff);
            enc_roundtrip(&EncoderInstruction::SetCapacity(0), huff);
            enc_roundtrip(
                &EncoderInstruction::InsertWithNameRef {
                    dynamic: false,
                    index: 17,
                    value: b"PUT".to_vec(),
                },
                huff,
            );
            enc_roundtrip(
                &EncoderInstruction::InsertWithNameRef {
                    dynamic: true,
                    index: 3,
                    value: b"custom-value".to_vec(),
                },
                huff,
            );
            enc_roundtrip(
                &EncoderInstruction::InsertWithLiteralName {
                    name: b"x-custom-header".to_vec(),
                    value: b"some-value".to_vec(),
                },
                huff,
            );
            enc_roundtrip(&EncoderInstruction::Duplicate(0), huff);
            enc_roundtrip(&EncoderInstruction::Duplicate(42), huff);
        }
    }

    #[test]
    fn set_capacity_wire_shape() {
        // `0 0 1` + 5-bit prefix. Capacity 0 ⇒ 0x20.
        let mut buf = Vec::new();
        EncoderInstruction::SetCapacity(0).encode(&mut buf, false);
        assert_eq!(buf, vec![0x20]);
    }

    #[test]
    fn insert_name_ref_static_wire_shape() {
        // `1 T(=1)` + 6-bit index 17 = 0x80 | 0x40 | 17 = 0xd1, then value.
        let mut buf = Vec::new();
        EncoderInstruction::InsertWithNameRef {
            dynamic: false,
            index: 17,
            value: b"X".to_vec(),
        }
        .encode(&mut buf, false);
        assert_eq!(buf[0], 0xd1);
    }

    #[test]
    fn duplicate_wire_shape() {
        // `0 0 0` + 5-bit index 5 = 0x05.
        let mut buf = Vec::new();
        EncoderInstruction::Duplicate(5).encode(&mut buf, false);
        assert_eq!(buf, vec![0x05]);
    }

    #[test]
    fn decode_encoder_stream_multiple() {
        let instrs = vec![
            EncoderInstruction::SetCapacity(4096),
            EncoderInstruction::InsertWithLiteralName {
                name: b"custom-key".to_vec(),
                value: b"custom-value".to_vec(),
            },
            EncoderInstruction::InsertWithNameRef {
                dynamic: true,
                index: 0,
                value: b"v2".to_vec(),
            },
            EncoderInstruction::Duplicate(0),
        ];
        let mut buf = Vec::new();
        for i in &instrs {
            i.encode(&mut buf, true);
        }
        assert_eq!(decode_encoder_stream(&buf).unwrap(), instrs);
    }

    #[test]
    fn encoder_stream_drives_table_state() {
        // A concrete encoder stream (RFC 9204 §4.3 flavour) applied end to end.
        let mut buf = Vec::new();
        EncoderInstruction::SetCapacity(4096).encode(&mut buf, false);
        EncoderInstruction::InsertWithLiteralName {
            name: b":authority".to_vec(),
            value: b"www.example.com".to_vec(),
        }
        .encode(&mut buf, false);
        EncoderInstruction::InsertWithNameRef {
            dynamic: false,
            index: 1, // static :path
            value: b"/index.html".to_vec(),
        }
        .encode(&mut buf, false);

        let mut t = DynamicTable::new(8192);
        for instr in decode_encoder_stream(&buf).unwrap() {
            t.apply(&instr).unwrap();
        }
        assert_eq!(t.insert_count(), 2);
        assert_eq!(t.get_absolute(0), Some((&b":authority"[..], &b"www.example.com"[..])));
        assert_eq!(t.get_absolute(1), Some((&b":path"[..], &b"/index.html"[..])));
    }

    #[test]
    fn decode_encoder_stream_truncated_value_errors() {
        // Insert With Literal Name, name "a", value claims 5 bytes, none follow.
        // `0 1 H(=0)` + 5-bit name length 1 = 0x41, then 'a', then value header
        // 0x05 (H=0, length 5) with no bytes following.
        let buf = vec![0x41, b'a', 0x05];
        assert_eq!(
            decode_encoder_stream(&buf),
            Err(QpackStreamError::StringTooLong)
        );
    }

    #[test]
    fn decode_empty_encoder_stream_is_empty() {
        assert_eq!(decode_encoder_stream(&[]).unwrap(), vec![]);
    }

    // ── Decoder-stream codec ────────────────────────────────────────────────

    fn dec_roundtrip(instr: &DecoderInstruction) {
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        let (decoded, consumed) = DecoderInstruction::decode(&buf).unwrap();
        assert_eq!(&decoded, instr);
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn decoder_instruction_roundtrips() {
        dec_roundtrip(&DecoderInstruction::SectionAck(0));
        dec_roundtrip(&DecoderInstruction::SectionAck(1000));
        dec_roundtrip(&DecoderInstruction::StreamCancellation(3));
        dec_roundtrip(&DecoderInstruction::StreamCancellation(500));
        dec_roundtrip(&DecoderInstruction::InsertCountIncrement(1));
        dec_roundtrip(&DecoderInstruction::InsertCountIncrement(63));
    }

    #[test]
    fn decoder_instruction_wire_shapes() {
        let mut buf = Vec::new();
        DecoderInstruction::SectionAck(0).encode(&mut buf);
        assert_eq!(buf, vec![0x80]); // `1` + 7-bit 0

        buf.clear();
        DecoderInstruction::StreamCancellation(0).encode(&mut buf);
        assert_eq!(buf, vec![0x40]); // `01` + 6-bit 0

        buf.clear();
        DecoderInstruction::InsertCountIncrement(0).encode(&mut buf);
        assert_eq!(buf, vec![0x00]); // `00` + 6-bit 0
    }

    #[test]
    fn decode_decoder_stream_multiple() {
        let instrs = vec![
            DecoderInstruction::SectionAck(4),
            DecoderInstruction::InsertCountIncrement(2),
            DecoderInstruction::StreamCancellation(4),
        ];
        let mut buf = Vec::new();
        for i in &instrs {
            i.encode(&mut buf);
        }
        assert_eq!(decode_decoder_stream(&buf).unwrap(), instrs);
    }

    #[test]
    fn error_codes_are_stream_specific() {
        let e = QpackStreamError::UnexpectedEof;
        assert_eq!(e.code(), QPACK_ENCODER_STREAM_ERROR);
        assert_eq!(e.decoder_code(), QPACK_DECODER_STREAM_ERROR);
    }

    #[test]
    fn decode_truncated_instruction_is_eof() {
        assert_eq!(
            EncoderInstruction::decode(&[]),
            Err(QpackStreamError::UnexpectedEof)
        );
        assert_eq!(
            DecoderInstruction::decode(&[]),
            Err(QpackStreamError::UnexpectedEof)
        );
    }
}
