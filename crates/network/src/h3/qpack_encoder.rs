//! The QPACK encoder driver — the request-path half of QPACK (RFC 9204 §2.1).
//!
//! Slices 2, 6, and 30 built the QPACK codec pieces in isolation: the
//! static-only field-section codec ([`super::qpack`]), the dynamic table plus
//! encoder/decoder instruction streams ([`super::qpack_stream`]), and the
//! dynamic-table-aware field-section codec
//! ([`super::qpack::encode_field_section_dynamic`]). None of them decide *policy*
//! — whether to insert a header into the dynamic table, which representation to
//! use, when a stream would block, or when an entry may be evicted.
//!
//! [`QpackEncoder`] is that policy layer. It owns the encoder's mirror of the
//! decoder's dynamic table, and for each request it:
//!
//! - inserts beneficial header fields into the table, emitting the matching
//!   encoder-stream instructions (RFC 9204 §4.3) — the "wiring of the QPACK
//!   instruction stream into the request path" the HTTP/3 slice plan called for;
//! - encodes the field section against the resulting table
//!   ([`super::qpack::encode_field_section_dynamic_bounded`]), never referencing
//!   an entry the decoder has not acknowledged unless the connection's
//!   blocked-stream budget (`SETTINGS_QPACK_BLOCKED_STREAMS`) allows it
//!   (RFC 9204 §2.1.2);
//! - tracks each outstanding section so a referenced entry is not evicted before
//!   the decoder acknowledges the section (RFC 9204 §2.1.3), and advances the
//!   Known Received Count as the decoder stream acknowledges sections and
//!   insertions (RFC 9204 §2.1.4, §4.4).
//!
//! Everything here is a pure state machine over the two QPACK codecs: no IO, no
//! unidirectional-stream framing. The caller writes [`EncodedRequest::encoder_stream`]
//! on the QPACK encoder stream and [`EncodedRequest::field_section`] inside the
//! request stream's `HEADERS` frame, and feeds decoder-stream instructions back
//! through [`QpackEncoder::on_decoder_instruction`].

use super::qpack::{self, HeaderField};
use super::qpack_stream::{DecoderInstruction, DynamicTable, EncoderInstruction, QpackStreamError};

/// One field section the encoder emitted and the decoder has not yet
/// acknowledged. Retained so the entries it references are protected from
/// eviction (RFC 9204 §2.1.3) and so a Section Acknowledgment can advance the
/// Known Received Count (RFC 9204 §4.4.1).
#[derive(Clone, Copy, Debug)]
struct Outstanding {
    /// The stream the section was sent on (Section Acknowledgment / Stream
    /// Cancellation name a stream, not a section).
    stream_id: u64,
    /// The section's Required Insert Count. The section is *blocking* while this
    /// exceeds the Known Received Count.
    required_insert_count: u64,
    /// The smallest absolute index the section references (`None` if it makes no
    /// dynamic reference — such sections are never tracked here).
    min_referenced: u64,
}

/// The result of encoding one request's field section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EncodedRequest {
    /// Bytes to append to the QPACK **encoder stream** (the Insert / Set
    /// Capacity instructions this request added to the dynamic table). Empty
    /// when the request inserted nothing.
    pub encoder_stream: Vec<u8>,
    /// The encoded field section to place inside the request stream's `HEADERS`
    /// frame (RFC 9114 §4.1, §7.2.2).
    pub field_section: Vec<u8>,
    /// The section's Required Insert Count (RFC 9204 §4.5.1.1).
    pub required_insert_count: u64,
    /// Whether the section references an entry the decoder has not yet
    /// acknowledged (Required Insert Count > Known Received Count) — a blocked
    /// stream (RFC 9204 §2.1.2).
    pub blocked: bool,
}

/// The connection-layer QPACK encoder: dynamic-table insertion policy, blocked-
/// stream accounting, and eviction-safe reference tracking (RFC 9204 §2.1).
#[derive(Clone, Debug)]
pub struct QpackEncoder {
    /// The encoder's mirror of the decoder's dynamic table.
    table: DynamicTable,
    /// The Known Received Count (RFC 9204 §2.1.4): the number of insertions the
    /// decoder has acknowledged receiving. References below this never block.
    known_received_count: u64,
    /// `SETTINGS_QPACK_BLOCKED_STREAMS` (RFC 9204 §5): the maximum number of
    /// streams the encoder may leave blocked at once.
    max_blocked_streams: usize,
    /// Whether to Huffman-code literal names/values when it does not enlarge them.
    use_huffman: bool,
    /// Field sections the decoder has not yet acknowledged, oldest first.
    outstanding: Vec<Outstanding>,
}

impl QpackEncoder {
    /// Create an encoder whose dynamic table may grow to `max_table_capacity`
    /// bytes (the decoder's advertised `SETTINGS_QPACK_MAX_TABLE_CAPACITY`) and
    /// which keeps at most `max_blocked_streams` streams blocked at once
    /// (`SETTINGS_QPACK_BLOCKED_STREAMS`). The table capacity starts at 0; call
    /// [`set_capacity`](Self::set_capacity) to enable insertions.
    #[must_use]
    pub fn new(max_table_capacity: usize, max_blocked_streams: usize) -> Self {
        Self {
            table: DynamicTable::new(max_table_capacity),
            known_received_count: 0,
            max_blocked_streams,
            use_huffman: true,
            outstanding: Vec::new(),
        }
    }

    /// Disable Huffman coding of literal names/values (enabled by default).
    #[must_use]
    pub fn without_huffman(mut self) -> Self {
        self.use_huffman = false;
        self
    }

    /// Raise (or lower) the dynamic table capacity, applying it to the local
    /// table and appending the Set Dynamic Table Capacity instruction (RFC 9204
    /// §4.3.1) to `encoder_stream`.
    ///
    /// # Errors
    ///
    /// [`QpackStreamError::CapacityExceedsMaximum`] if `capacity` exceeds the
    /// advertised maximum.
    pub fn set_capacity(
        &mut self,
        capacity: u64,
        encoder_stream: &mut Vec<u8>,
    ) -> Result<(), QpackStreamError> {
        self.table.set_capacity(capacity)?;
        EncoderInstruction::SetCapacity(capacity).encode(encoder_stream, self.use_huffman);
        Ok(())
    }

    /// The current dynamic table capacity in bytes.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.table.capacity()
    }

    /// The number of insertions performed so far (the table's Insert Count).
    #[must_use]
    pub fn insert_count(&self) -> u64 {
        self.table.insert_count()
    }

    /// The Known Received Count (RFC 9204 §2.1.4).
    #[must_use]
    pub fn known_received_count(&self) -> u64 {
        self.known_received_count
    }

    /// The number of streams currently blocked (RFC 9204 §2.1.2): distinct
    /// streams with an outstanding section whose Required Insert Count exceeds
    /// the Known Received Count.
    #[must_use]
    pub fn blocked_stream_count(&self) -> usize {
        let mut ids: Vec<u64> = self
            .outstanding
            .iter()
            .filter(|o| o.required_insert_count > self.known_received_count)
            .map(|o| o.stream_id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    }

    /// Whether `stream_id` already holds a blocking outstanding section.
    fn stream_is_blocked(&self, stream_id: u64) -> bool {
        self.outstanding.iter().any(|o| {
            o.stream_id == stream_id && o.required_insert_count > self.known_received_count
        })
    }

    /// The lowest absolute index that any outstanding section references, below
    /// which entries are free to evict (`None` when nothing is outstanding).
    fn protect_floor(&self) -> Option<u64> {
        self.outstanding.iter().map(|o| o.min_referenced).min()
    }

    /// Whether inserting an entry of `sz` bytes would evict only entries below
    /// `protect_floor` (RFC 9204 §2.1.3 — a referenced entry must never be
    /// evicted). `None` floor means nothing is protected.
    fn eviction_safe(&self, sz: usize, protect_floor: Option<u64>) -> bool {
        if sz > self.table.capacity() {
            return false;
        }
        let target = self.table.capacity() - sz;
        let mut size = self.table.size();
        // Oldest live entry sits at `insert_count - len`; eviction walks upward.
        let mut abs = self.table.insert_count() - self.table.len() as u64;
        while size > target {
            let Some((n, v)) = self.table.get_absolute(abs) else {
                break;
            };
            if protect_floor.is_some_and(|floor| abs >= floor) {
                return false;
            }
            size -= DynamicTable::entry_size(n, v);
            abs += 1;
        }
        true
    }

    /// Choose the encoder-stream instruction that inserts `field`, reusing a
    /// dynamic or static name reference where one exists (RFC 9204 §4.3.2/§4.3.3).
    fn insert_instruction(&self, field: &HeaderField) -> EncoderInstruction {
        if let Some(abs) = self.table.find_name_absolute(&field.name) {
            // Relative index on the encoder stream: 0 = most recent entry.
            let relative = self.table.insert_count() - 1 - abs;
            EncoderInstruction::InsertWithNameRef {
                dynamic: true,
                index: relative,
                value: field.value.clone(),
            }
        } else if let Some(idx) = qpack::find_static_name(&field.name) {
            EncoderInstruction::InsertWithNameRef {
                dynamic: false,
                index: idx,
                value: field.value.clone(),
            }
        } else {
            EncoderInstruction::InsertWithLiteralName {
                name: field.name.clone(),
                value: field.value.clone(),
            }
        }
    }

    /// Encode `fields` as a field section for `stream_id`, inserting beneficial
    /// entries into the dynamic table and emitting the matching encoder-stream
    /// instructions.
    ///
    /// The insertion pass adds a field to the table when the table has room, the
    /// field is not sensitive, the field is not already representable by a full
    /// static or dynamic match, and no still-referenced entry would be evicted.
    /// The section then references those entries; when the blocked-stream budget
    /// is exhausted it references only entries already acknowledged by the
    /// decoder, so no new blocked stream is created (RFC 9204 §2.1.2).
    #[must_use]
    pub fn encode_section(&mut self, stream_id: u64, fields: &[HeaderField]) -> EncodedRequest {
        // A new blocking reference is permitted if this stream is already blocked
        // (no new distinct blocked stream) or the budget has room.
        let allow_blocking = self.stream_is_blocked(stream_id)
            || self.blocked_stream_count() < self.max_blocked_streams;

        let mut encoder_stream = Vec::new();
        // Entries inserted this call are referenced by the section being built,
        // so protect them from a later insert's eviction too.
        let mut floor = self.protect_floor();

        for field in fields {
            if field.sensitive || self.table.capacity() == 0 {
                continue;
            }
            // A full match already lets the section index the entry — inserting a
            // duplicate would only waste table space and encoder-stream bytes.
            if self.table.find_absolute(&field.name, &field.value).is_some()
                || qpack::find_static_full(&field.name, &field.value).is_some()
            {
                continue;
            }
            let sz = DynamicTable::entry_size(&field.name, &field.value);
            if sz > self.table.capacity() || !self.eviction_safe(sz, floor) {
                continue;
            }
            let instr = self.insert_instruction(field);
            instr.encode(&mut encoder_stream, self.use_huffman);
            // Capacity and eviction were both checked above, so this cannot fail.
            if let Ok(Some(abs)) = self.table.apply(&instr) {
                floor = Some(floor.map_or(abs, |f| f.min(abs)));
            }
        }

        // Reference only acknowledged entries when a new blocked stream is not
        // permitted, so the Required Insert Count never exceeds the Known
        // Received Count.
        let ceiling = if allow_blocking { u64::MAX } else { self.known_received_count };
        let (field_section, info) =
            qpack::encode_field_section_dynamic_bounded(fields, &self.table, self.use_huffman, ceiling);

        let blocked = info.required_insert_count > self.known_received_count;
        if let Some(min_referenced) = info.min_referenced {
            self.outstanding.push(Outstanding {
                stream_id,
                required_insert_count: info.required_insert_count,
                min_referenced,
            });
        }

        EncodedRequest {
            encoder_stream,
            field_section,
            required_insert_count: info.required_insert_count,
            blocked,
        }
    }

    /// Apply a decoder-stream instruction (RFC 9204 §4.4) received from the peer.
    ///
    /// - Section Acknowledgment advances the Known Received Count to the
    ///   acknowledged section's Required Insert Count and releases its references
    ///   (RFC 9204 §4.4.1).
    /// - Stream Cancellation drops every outstanding section on the stream
    ///   without advancing the Known Received Count (RFC 9204 §4.4.2).
    /// - Insert Count Increment advances the Known Received Count directly
    ///   (RFC 9204 §4.4.3).
    ///
    /// # Errors
    ///
    /// [`QpackStreamError::UnexpectedSectionAck`] if a Section Acknowledgment
    /// names a stream with no outstanding section, or
    /// [`QpackStreamError::InsertCountIncrementOverflow`] if an Insert Count
    /// Increment would raise the Known Received Count beyond the Insert Count —
    /// both `QPACK_DECODER_STREAM_ERROR` connection errors.
    pub fn on_decoder_instruction(
        &mut self,
        instr: &DecoderInstruction,
    ) -> Result<(), QpackStreamError> {
        match instr {
            DecoderInstruction::SectionAck(stream_id) => {
                let pos = self
                    .outstanding
                    .iter()
                    .position(|o| o.stream_id == *stream_id)
                    .ok_or(QpackStreamError::UnexpectedSectionAck(*stream_id))?;
                let section = self.outstanding.remove(pos);
                // The section could not have been sent unless its entries were
                // inserted, so acknowledging it confirms their receipt.
                self.known_received_count =
                    self.known_received_count.max(section.required_insert_count);
                Ok(())
            }
            DecoderInstruction::StreamCancellation(stream_id) => {
                self.outstanding.retain(|o| o.stream_id != *stream_id);
                Ok(())
            }
            DecoderInstruction::InsertCountIncrement(increment) => {
                let updated = self
                    .known_received_count
                    .checked_add(*increment)
                    .filter(|&krc| krc <= self.table.insert_count())
                    .ok_or(QpackStreamError::InsertCountIncrementOverflow(*increment))?;
                self.known_received_count = updated;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::qpack::{self, HeaderField};
    use crate::h3::qpack_stream::DynamicTable;

    /// Decode a field section against a table the decoder rebuilt from the
    /// encoder stream, asserting it round-trips to `expected`.
    fn assert_roundtrip(
        encoder_stream: &[u8],
        field_section: &[u8],
        max_capacity: usize,
        expected: &[HeaderField],
    ) {
        // Rebuild the decoder's table by replaying the encoder stream. The
        // decoder learns the capacity from a Set Dynamic Table Capacity
        // instruction; the tests emit it separately, so apply it here directly.
        let mut decoder_table = DynamicTable::new(max_capacity);
        decoder_table.set_capacity(max_capacity as u64).unwrap();
        for instr in crate::h3::qpack_stream::decode_encoder_stream(encoder_stream).unwrap() {
            decoder_table.apply(&instr).unwrap();
        }
        let decoded = qpack::decode_field_section_dynamic(field_section, &decoder_table).unwrap();
        assert_eq!(decoded, expected);
    }

    fn req_fields() -> Vec<HeaderField> {
        vec![
            HeaderField::new(b":method".to_vec(), b"GET".to_vec()),
            HeaderField::new(b":path".to_vec(), b"/index.html".to_vec()),
            HeaderField::new(b"custom-key".to_vec(), b"custom-value".to_vec()),
        ]
    }

    #[test]
    fn zero_capacity_never_inserts() {
        let mut enc = QpackEncoder::new(4096, 16);
        // No set_capacity ⇒ capacity 0 ⇒ static/literal only.
        let out = enc.encode_section(0, &req_fields());
        assert!(out.encoder_stream.is_empty());
        assert_eq!(out.required_insert_count, 0);
        assert!(!out.blocked);
        assert_eq!(enc.insert_count(), 0);
        assert_roundtrip(&out.encoder_stream, &out.field_section, 4096, &req_fields());
    }

    #[test]
    fn inserts_and_references_dynamic_entries() {
        let mut enc = QpackEncoder::new(4096, 16);
        let mut cap_stream = Vec::new();
        enc.set_capacity(4096, &mut cap_stream).unwrap();
        assert!(!cap_stream.is_empty());

        let out = enc.encode_section(0, &req_fields());
        // The custom field (no full static match) is inserted; :path /index.html
        // has no full static match either, so it is inserted too. :method GET is
        // a full static match (index 17) and is not inserted.
        assert!(enc.insert_count() >= 1);
        assert!(!out.encoder_stream.is_empty());
        assert_eq!(out.required_insert_count, enc.insert_count());
        // No acknowledgments yet ⇒ referencing fresh inserts blocks the stream.
        assert!(out.blocked);
        assert_eq!(enc.blocked_stream_count(), 1);

        // The decoder replays the encoder stream, then decodes the section.
        assert_roundtrip(&out.encoder_stream, &out.field_section, 4096, &req_fields());
    }

    #[test]
    fn full_static_match_is_not_inserted() {
        let mut enc = QpackEncoder::new(4096, 16);
        let mut s = Vec::new();
        enc.set_capacity(4096, &mut s).unwrap();
        // :method GET is a full static entry — must not be inserted.
        let fields = vec![HeaderField::new(b":method".to_vec(), b"GET".to_vec())];
        let out = enc.encode_section(0, &fields);
        assert_eq!(enc.insert_count(), 0);
        assert!(out.encoder_stream.is_empty());
        assert_eq!(out.required_insert_count, 0);
        assert!(!out.blocked);
    }

    #[test]
    fn sensitive_field_is_never_inserted() {
        let mut enc = QpackEncoder::new(4096, 16);
        let mut s = Vec::new();
        enc.set_capacity(4096, &mut s).unwrap();
        let fields = vec![HeaderField::sensitive(b"authorization".to_vec(), b"Bearer xyz".to_vec())];
        let out = enc.encode_section(0, &fields);
        assert_eq!(enc.insert_count(), 0, "sensitive field must not enter the shared table");
        assert!(out.encoder_stream.is_empty());
        assert_roundtrip(&out.encoder_stream, &out.field_section, 4096, &fields);
    }

    #[test]
    fn second_request_reuses_inserted_entry_after_ack() {
        let mut enc = QpackEncoder::new(4096, 16);
        let mut s = Vec::new();
        enc.set_capacity(4096, &mut s).unwrap();
        let fields = vec![HeaderField::new(b"x-token".to_vec(), b"abc".to_vec())];

        let first = enc.encode_section(0, &fields);
        assert_eq!(enc.insert_count(), 1);
        assert!(first.blocked);
        // Decoder acknowledges the section, advancing the Known Received Count.
        enc.on_decoder_instruction(&DecoderInstruction::SectionAck(0)).unwrap();
        assert_eq!(enc.known_received_count(), 1);
        assert_eq!(enc.blocked_stream_count(), 0);

        // Second request references the same entry without inserting again — and
        // without blocking, because the entry is now acknowledged.
        let second = enc.encode_section(4, &fields);
        assert_eq!(enc.insert_count(), 1, "entry reused, not re-inserted");
        assert!(second.encoder_stream.is_empty());
        assert_eq!(second.required_insert_count, 1);
        assert!(!second.blocked);
    }

    #[test]
    fn blocked_budget_prevents_new_blocking() {
        // Budget of one blocked stream.
        let mut enc = QpackEncoder::new(4096, 1);
        let mut s = Vec::new();
        enc.set_capacity(4096, &mut s).unwrap();

        let a = enc.encode_section(0, &[HeaderField::new(b"x-a".to_vec(), b"1".to_vec())]);
        assert!(a.blocked);
        assert_eq!(enc.blocked_stream_count(), 1);

        // Second stream: budget exhausted ⇒ must not create a blocking reference.
        let fields_b = vec![HeaderField::new(b"x-b".to_vec(), b"2".to_vec())];
        let b = enc.encode_section(4, &fields_b);
        assert!(!b.blocked, "budget exhausted ⇒ no new blocked stream");
        assert_eq!(b.required_insert_count, 0, "referenced only acknowledged (none) entries");
        // The entry may still be inserted to prime the table for the future.
        assert_roundtrip(&b.encoder_stream, &b.field_section, 4096, &fields_b);
    }

    #[test]
    fn insert_count_increment_advances_krc() {
        let mut enc = QpackEncoder::new(4096, 16);
        let mut s = Vec::new();
        enc.set_capacity(4096, &mut s).unwrap();
        let _ = enc.encode_section(0, &[HeaderField::new(b"x-a".to_vec(), b"1".to_vec())]);
        assert_eq!(enc.insert_count(), 1);
        enc.on_decoder_instruction(&DecoderInstruction::InsertCountIncrement(1)).unwrap();
        assert_eq!(enc.known_received_count(), 1);
    }

    #[test]
    fn insert_count_increment_overflow_is_error() {
        let mut enc = QpackEncoder::new(4096, 16);
        let mut s = Vec::new();
        enc.set_capacity(4096, &mut s).unwrap();
        // Insert count is 0; any increment overflows.
        let err = enc
            .on_decoder_instruction(&DecoderInstruction::InsertCountIncrement(1))
            .unwrap_err();
        assert_eq!(err, QpackStreamError::InsertCountIncrementOverflow(1));
        assert_eq!(err.code(), crate::h3::qpack_stream::QPACK_DECODER_STREAM_ERROR);
    }

    #[test]
    fn section_ack_for_unknown_stream_is_error() {
        let mut enc = QpackEncoder::new(4096, 16);
        let err = enc
            .on_decoder_instruction(&DecoderInstruction::SectionAck(99))
            .unwrap_err();
        assert_eq!(err, QpackStreamError::UnexpectedSectionAck(99));
        assert_eq!(err.code(), crate::h3::qpack_stream::QPACK_DECODER_STREAM_ERROR);
    }

    #[test]
    fn stream_cancellation_releases_without_advancing_krc() {
        let mut enc = QpackEncoder::new(4096, 16);
        let mut s = Vec::new();
        enc.set_capacity(4096, &mut s).unwrap();
        let out = enc.encode_section(0, &[HeaderField::new(b"x-a".to_vec(), b"1".to_vec())]);
        assert!(out.blocked);
        assert_eq!(enc.blocked_stream_count(), 1);
        enc.on_decoder_instruction(&DecoderInstruction::StreamCancellation(0)).unwrap();
        // References released, but the Known Received Count is unchanged.
        assert_eq!(enc.known_received_count(), 0);
        assert_eq!(enc.blocked_stream_count(), 0);
    }

    #[test]
    fn eviction_protects_referenced_entries() {
        // A small table that holds ~one entry, so a second insert must evict the
        // first — but the first is still referenced by an unacknowledged section.
        let entry = DynamicTable::entry_size(b"x-aaaa", b"1");
        let mut enc = QpackEncoder::new(entry * 2 - 1, 16);
        let mut s = Vec::new();
        enc.set_capacity((entry * 2 - 1) as u64, &mut s).unwrap();

        let first = enc.encode_section(0, &[HeaderField::new(b"x-aaaa".to_vec(), b"1".to_vec())]);
        assert_eq!(enc.insert_count(), 1);
        assert!(first.blocked);

        // Second stream would need to evict entry 0, which is still referenced ⇒
        // the encoder must not insert; it falls back to a literal instead.
        let fields_b = vec![HeaderField::new(b"x-bbbb".to_vec(), b"2".to_vec())];
        let before = enc.insert_count();
        let b = enc.encode_section(4, &fields_b);
        assert_eq!(enc.insert_count(), before, "referenced entry must not be evicted");
        assert!(b.encoder_stream.is_empty());

        // Once the first section is acknowledged, the entry is free to evict.
        enc.on_decoder_instruction(&DecoderInstruction::SectionAck(0)).unwrap();
        let c = enc.encode_section(8, &fields_b);
        assert_eq!(enc.insert_count(), before + 1, "entry now evictable ⇒ insert proceeds");
        assert!(!c.encoder_stream.is_empty());
    }

    #[test]
    fn set_capacity_above_maximum_errors() {
        let mut enc = QpackEncoder::new(1024, 16);
        let mut s = Vec::new();
        let err = enc.set_capacity(2048, &mut s).unwrap_err();
        assert_eq!(err, QpackStreamError::CapacityExceedsMaximum(2048));
        assert!(s.is_empty(), "no instruction emitted on failure");
    }

    #[test]
    fn multiple_sections_per_stream_ack_fifo() {
        let mut enc = QpackEncoder::new(4096, 16);
        let mut s = Vec::new();
        enc.set_capacity(4096, &mut s).unwrap();
        // Two sections on the same stream (HEADERS + trailers).
        let _ = enc.encode_section(0, &[HeaderField::new(b"x-a".to_vec(), b"1".to_vec())]);
        let _ = enc.encode_section(0, &[HeaderField::new(b"x-b".to_vec(), b"2".to_vec())]);
        assert_eq!(enc.insert_count(), 2);
        // First ack advances KRC to the first section's RIC (1).
        enc.on_decoder_instruction(&DecoderInstruction::SectionAck(0)).unwrap();
        assert_eq!(enc.known_received_count(), 1);
        // Second ack advances to the second section's RIC (2).
        enc.on_decoder_instruction(&DecoderInstruction::SectionAck(0)).unwrap();
        assert_eq!(enc.known_received_count(), 2);
        // No sections left ⇒ a third ack errors.
        assert!(enc.on_decoder_instruction(&DecoderInstruction::SectionAck(0)).is_err());
    }
}
