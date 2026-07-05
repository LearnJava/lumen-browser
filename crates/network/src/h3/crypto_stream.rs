//! QUIC CRYPTO stream — reassembly and transmission of the TLS handshake byte
//! stream carried in CRYPTO frames (RFC 9000 §7.5, §19.6).
//!
//! Every QUIC handshake message the TLS layer produces or consumes travels in
//! CRYPTO frames ([`quic_frame::Frame::Crypto`], RFC 9001 §4), each a
//! `(offset, data)` slice of a per-encryption-level byte stream. Unlike a
//! [`stream::RecvStream`], the CRYPTO stream has **no stream ID, no flow-control
//! window, and no FIN** — it is an ordered, unbounded byte stream with an
//! offset space independent at each encryption level (Initial, Handshake, and
//! 1-RTT / Application; RFC 9000 §12.5). This slice adds the piece between the
//! decoded CRYPTO frames and the [`tls_message`] handshake codec: the
//! **receive reassembly buffer** that turns out-of-order / overlapping /
//! duplicated CRYPTO frames into a contiguous handshake byte stream, and the
//! **send buffer** that hands the TLS output back as CRYPTO frames to transmit.
//! Like every slice so far it is a pure state machine — no IO, no packet
//! protection, no timers of its own.
//!
//! [`quic_frame::Frame::Crypto`]: crate::h3::quic_frame::Frame::Crypto
//! [`stream::RecvStream`]: crate::h3::stream::RecvStream
//! [`tls_message`]: crate::h3::tls_message
//!
//! ## Encryption levels
//!
//! CRYPTO data flows at three encryption levels, each with its own offset space
//! (RFC 9000 §12.5): the connection layer keeps one [`CryptoRecvStream`] and one
//! [`CryptoSendStream`] per level (mapping to the Initial / Handshake /
//! Application [`loss::PacketNumberSpace`], RFC 9002). This module is strictly
//! per-level; coordinating the three is the connection layer's job.
//!
//! [`loss::PacketNumberSpace`]: crate::h3::loss::PacketNumberSpace
//!
//! ## Receive stream ([`CryptoRecvStream`], RFC 9000 §7.5)
//!
//! CRYPTO frames may arrive out of order, overlap a retransmission, or repeat
//! data already delivered. [`CryptoRecvStream::recv`] clips already-read bytes,
//! merges the segment into the buffered set (preferring bytes already held,
//! which QUIC guarantees are identical, RFC 9000 §7.5), and enforces a buffering
//! bound: data whose offset reaches past `read_offset + buffer_limit` is
//! rejected with [`CryptoStreamError::BufferExceeded`], the receiver-side trigger
//! for a `CRYPTO_BUFFER_EXCEEDED` connection error (RFC 9000 §7.5, §20.1).
//! [`CryptoRecvStream::read`] pops the contiguous prefix and advances the read
//! cursor, feeding the [`tls_message`] parser.
//!
//! ## Send stream ([`CryptoSendStream`], RFC 9000 §7.5)
//!
//! [`CryptoSendStream::write`] queues the handshake bytes TLS emits;
//! [`CryptoSendStream::poll_transmit`] hands back the next CRYPTO frame bounded
//! by a caller size cap; [`CryptoSendStream::on_ack`] tracks the acknowledged
//! byte ranges so the layer can tell when the handshake data it sent is fully
//! acknowledged. There is no flow control on CRYPTO data (RFC 9000 §7.5) and no
//! FIN, so the send side is simpler than [`stream::SendStream`].
//!
//! [`stream::SendStream`]: crate::h3::stream::SendStream
//!
//! ## Out of scope (later slices)
//!
//! - Retransmission / packetization of the CRYPTO frames it produces, and the
//!   mapping from ACK frames to [`CryptoSendStream::on_ack`] (the loss layer's
//!   role).
//! - Any IO, header protection, AEAD, or driving the TLS handshake itself.

use std::collections::BTreeMap;

use super::quic_frame;

// ── Wire error code (RFC 9000 §20.1) ─────────────────────────────────────────

/// `CRYPTO_BUFFER_EXCEEDED` — the peer sent CRYPTO data past the amount an
/// endpoint is willing to buffer for reassembly (RFC 9000 §20.1, §7.5).
pub const CRYPTO_BUFFER_EXCEEDED: u64 = 0x0d;

/// The default per-level CRYPTO reassembly bound (bytes past the read cursor).
///
/// The RFC sets no fixed value (RFC 9000 §7.5); 64 KiB comfortably holds a
/// ServerHello plus a full certificate chain and CertificateVerify at one
/// encryption level, while bounding the memory a peer can pin with out-of-order
/// CRYPTO frames.
pub const DEFAULT_CRYPTO_BUFFER_LIMIT: u64 = 65_536;

/// A CRYPTO-stream protocol violation. Maps to a single QUIC connection-error
/// code via [`CryptoStreamError::code`]; the variant preserves *why* for
/// diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CryptoStreamError {
    /// Received CRYPTO data reached past the reassembly bound (RFC 9000 §7.5).
    /// Maps to [`CRYPTO_BUFFER_EXCEEDED`].
    BufferExceeded {
        /// One past the highest byte offset the peer tried to reach.
        offset: u64,
        /// The reassembly bound (`read_offset + buffer_limit`) that was passed.
        limit: u64,
    },
}

impl CryptoStreamError {
    /// The RFC 9000 §20.1 connection-error code this violation maps to.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::BufferExceeded { .. } => CRYPTO_BUFFER_EXCEEDED,
        }
    }
}

impl core::fmt::Display for CryptoStreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BufferExceeded { offset, limit } => {
                write!(f, "QUIC crypto: offset {offset} exceeds reassembly bound {limit}")
            }
        }
    }
}

impl std::error::Error for CryptoStreamError {}

// ── Receive stream (RFC 9000 §7.5) ───────────────────────────────────────────

/// The receiving half of a QUIC CRYPTO stream: an out-of-order reassembly
/// buffer that yields the contiguous handshake byte prefix (RFC 9000 §7.5).
#[derive(Clone, Debug)]
pub struct CryptoRecvStream {
    /// Buffered received segments at or beyond [`Self::read_offset`], keyed by
    /// start offset. Segments are kept disjoint and touching-merged so the
    /// segment at key `read_offset` (if any) spans the whole readable prefix.
    buffered: BTreeMap<u64, Vec<u8>>,
    /// The next byte offset the TLS layer will read; all bytes below have been
    /// delivered by [`Self::read`].
    read_offset: u64,
    /// One past the highest byte offset ever received.
    highest_received: u64,
    /// How far past [`Self::read_offset`] this endpoint will buffer before
    /// signalling `CRYPTO_BUFFER_EXCEEDED` (RFC 9000 §7.5).
    buffer_limit: u64,
}

impl Default for CryptoRecvStream {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoRecvStream {
    /// Creates a receive stream buffering up to [`DEFAULT_CRYPTO_BUFFER_LIMIT`]
    /// bytes past the read cursor.
    #[must_use]
    pub fn new() -> Self {
        Self::with_buffer_limit(DEFAULT_CRYPTO_BUFFER_LIMIT)
    }

    /// Creates a receive stream buffering up to `buffer_limit` bytes past the
    /// read cursor before signalling `CRYPTO_BUFFER_EXCEEDED` (RFC 9000 §7.5).
    #[must_use]
    pub fn with_buffer_limit(buffer_limit: u64) -> Self {
        Self {
            buffered: BTreeMap::new(),
            read_offset: 0,
            highest_received: 0,
            buffer_limit,
        }
    }

    /// The next offset the TLS layer will read (bytes below are delivered).
    #[must_use]
    pub fn read_offset(&self) -> u64 {
        self.read_offset
    }

    /// One past the highest byte offset ever received.
    #[must_use]
    pub fn highest_received(&self) -> u64 {
        self.highest_received
    }

    /// The reassembly bound past the read cursor (RFC 9000 §7.5).
    #[must_use]
    pub fn buffer_limit(&self) -> u64 {
        self.buffer_limit
    }

    /// Whether contiguous data is available to [`Self::read`].
    #[must_use]
    pub fn is_readable(&self) -> bool {
        self.buffered.contains_key(&self.read_offset)
    }

    /// Processes a received CRYPTO frame (RFC 9000 §19.6): `offset`/`data` is the
    /// carried byte range of the handshake stream.
    ///
    /// Enforces the reassembly bound (RFC 9000 §7.5): returns
    /// [`CryptoStreamError::BufferExceeded`] if the data reaches past
    /// `read_offset + buffer_limit`. Bytes at or below the read cursor are
    /// clipped, and overlapping bytes prefer the copy already buffered (QUIC
    /// guarantees the overlap is identical, RFC 9000 §7.5).
    pub fn recv(&mut self, offset: u64, data: &[u8]) -> Result<(), CryptoStreamError> {
        let end = offset.saturating_add(data.len() as u64);

        // Reassembly bound (RFC 9000 §7.5): data must not reach past the window
        // this endpoint is willing to buffer beyond what it has consumed.
        let limit = self.read_offset.saturating_add(self.buffer_limit);
        if end > limit {
            return Err(CryptoStreamError::BufferExceeded { offset: end, limit });
        }

        self.highest_received = self.highest_received.max(end);
        if !data.is_empty() {
            self.insert_segment(offset, data);
        }
        Ok(())
    }

    /// Convenience wrapper over [`Self::recv`] for a decoded
    /// [`quic_frame::Frame::Crypto`]. Any other frame is a caller error and is
    /// ignored.
    ///
    /// [`quic_frame::Frame::Crypto`]: crate::h3::quic_frame::Frame::Crypto
    pub fn recv_frame(&mut self, frame: &quic_frame::Frame) -> Result<(), CryptoStreamError> {
        if let quic_frame::Frame::Crypto { offset, data } = frame {
            self.recv(*offset, data)
        } else {
            Ok(())
        }
    }

    /// Pops and returns the contiguous readable prefix, advancing the read
    /// cursor. Returns an empty vector when a gap precedes the next buffered
    /// bytes.
    pub fn read(&mut self) -> Vec<u8> {
        let Some(chunk) = self.buffered.remove(&self.read_offset) else {
            return Vec::new();
        };
        self.read_offset = self.read_offset.saturating_add(chunk.len() as u64);
        chunk
    }

    /// Merges `data` at `offset` into the buffered set, clipping bytes already
    /// read and preferring bytes already buffered on overlap (RFC 9000 §7.5
    /// guarantees overlapping data is identical). Segments that touch or overlap
    /// are coalesced so the readable prefix is a single segment.
    fn insert_segment(&mut self, offset: u64, data: &[u8]) {
        // Clip the portion at or below the read cursor (already delivered).
        let (offset, data) = if offset < self.read_offset {
            let skip = (self.read_offset - offset) as usize;
            if skip >= data.len() {
                return;
            }
            (self.read_offset, &data[skip..])
        } else {
            (offset, data)
        };
        let mut merge_start = offset;
        let mut merge_end = offset + data.len() as u64;

        // Collect existing segments that touch or overlap [merge_start, merge_end].
        // `range(..=merge_end)` bounds the search to segments starting no later
        // than our end; the filter keeps those whose end reaches our start.
        let touching: Vec<u64> = self
            .buffered
            .range(..=merge_end)
            .filter(|(k, v)| **k + v.len() as u64 >= merge_start)
            .map(|(k, _)| *k)
            .collect();
        for k in &touching {
            let seg = &self.buffered[k];
            merge_start = merge_start.min(*k);
            merge_end = merge_end.max(*k + seg.len() as u64);
        }

        // Build the merged buffer. Existing bytes are authoritative; new bytes
        // only fill positions no existing segment covered.
        let mut buf = vec![0u8; (merge_end - merge_start) as usize];
        let mut filled = vec![false; buf.len()];
        for k in &touching {
            if let Some(seg) = self.buffered.remove(k) {
                let base = (k - merge_start) as usize;
                for (i, b) in seg.into_iter().enumerate() {
                    buf[base + i] = b;
                    filled[base + i] = true;
                }
            }
        }
        let base = (offset - merge_start) as usize;
        for (i, &b) in data.iter().enumerate() {
            if !filled[base + i] {
                buf[base + i] = b;
            }
        }
        self.buffered.insert(merge_start, buf);
    }
}

// ── Send stream (RFC 9000 §7.5) ──────────────────────────────────────────────

/// A CRYPTO frame to transmit, produced by [`CryptoSendStream::poll_transmit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CryptoChunk {
    /// Byte offset of `data` on the crypto stream.
    pub offset: u64,
    /// The handshake data to send.
    pub data: Vec<u8>,
}

impl CryptoChunk {
    /// Converts this chunk into a [`quic_frame::Frame::Crypto`] for framing.
    ///
    /// [`quic_frame::Frame::Crypto`]: crate::h3::quic_frame::Frame::Crypto
    #[must_use]
    pub fn into_frame(self) -> quic_frame::Frame {
        quic_frame::Frame::Crypto { offset: self.offset, data: self.data }
    }
}

/// The sending half of a QUIC CRYPTO stream: an outgoing handshake buffer with
/// acknowledgement tracking (RFC 9000 §7.5). No flow control, no FIN.
#[derive(Clone, Debug, Default)]
pub struct CryptoSendStream {
    /// Handshake bytes queued but not yet emitted in a CRYPTO frame.
    unsent: Vec<u8>,
    /// Offset of the first byte in [`Self::unsent`] — the next offset to emit.
    send_offset: u64,
    /// Acknowledged byte ranges, keyed by start offset, merged and disjoint.
    acked: BTreeMap<u64, u64>,
}

impl CryptoSendStream {
    /// Creates an empty send stream at offset 0.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The total number of bytes written by the TLS layer so far (also one past
    /// the highest offset ever queued).
    #[must_use]
    pub fn write_offset(&self) -> u64 {
        self.send_offset + self.unsent.len() as u64
    }

    /// One past the highest offset ever emitted in a CRYPTO frame.
    #[must_use]
    pub fn send_offset(&self) -> u64 {
        self.send_offset
    }

    /// Whether there is unsent handshake data awaiting a [`Self::poll_transmit`].
    #[must_use]
    pub fn has_unsent(&self) -> bool {
        !self.unsent.is_empty()
    }

    /// Queues handshake `data` for transmission (RFC 9000 §7.5). Empty writes
    /// are ignored.
    pub fn write(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.unsent.extend_from_slice(data);
    }

    /// Produces the next CRYPTO frame to transmit, at most `max_len` data bytes.
    /// Returns `None` when there is no unsent data.
    pub fn poll_transmit(&mut self, max_len: usize) -> Option<CryptoChunk> {
        let take = self.unsent.len().min(max_len);
        if take == 0 {
            return None;
        }
        let offset = self.send_offset;
        let data: Vec<u8> = self.unsent.drain(..take).collect();
        self.send_offset += take as u64;
        Some(CryptoChunk { offset, data })
    }

    /// Records that the byte range `[offset, offset + len)` was acknowledged
    /// (RFC 9000 §7.5). Empty acknowledgements are ignored.
    pub fn on_ack(&mut self, offset: u64, len: u64) {
        if len > 0 {
            insert_range(&mut self.acked, offset, offset + len);
        }
    }

    /// Whether every byte written so far has been emitted and acknowledged, i.e.
    /// the handshake data on this stream is fully delivered.
    #[must_use]
    pub fn is_fully_acked(&self) -> bool {
        self.unsent.is_empty() && contiguous_end(&self.acked) >= self.send_offset
    }
}

/// Inserts the half-open byte range `[start, end)` into a merged, disjoint set
/// of ranges keyed by start offset, coalescing any touching or overlapping
/// neighbours.
fn insert_range(ranges: &mut BTreeMap<u64, u64>, start: u64, end: u64) {
    if start >= end {
        return;
    }
    let mut new_start = start;
    let mut new_end = end;
    let touching: Vec<u64> = ranges
        .range(..=new_end)
        .filter(|(_, e)| **e >= new_start)
        .map(|(k, _)| *k)
        .collect();
    for k in touching {
        if let Some(e) = ranges.remove(&k) {
            new_start = new_start.min(k);
            new_end = new_end.max(e);
        }
    }
    ranges.insert(new_start, new_end);
}

/// The end of the range that starts at offset 0, i.e. the highest offset
/// reachable contiguously from the stream start, or `0` if there is a gap at
/// the start.
fn contiguous_end(ranges: &BTreeMap<u64, u64>) -> u64 {
    ranges.get(&0).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CryptoRecvStream reassembly ─────────────────────────────────────────

    #[test]
    fn recv_in_order() {
        let mut s = CryptoRecvStream::new();
        s.recv(0, b"\x01\x00\x00").unwrap();
        s.recv(3, b"\x2aClientHello").unwrap();
        assert!(s.is_readable());
        assert_eq!(s.read(), b"\x01\x00\x00\x2aClientHello");
        assert_eq!(s.read_offset(), 15);
        assert!(!s.is_readable());
    }

    #[test]
    fn recv_out_of_order_holds_until_gap_filled() {
        let mut s = CryptoRecvStream::new();
        s.recv(6, b"world").unwrap();
        // Nothing readable while the [0,6) gap is open.
        assert!(!s.is_readable());
        assert_eq!(s.read(), b"");
        // Filling the gap unlocks the whole run.
        s.recv(0, b"hello ").unwrap();
        assert!(s.is_readable());
        assert_eq!(s.read(), b"hello world");
        assert_eq!(s.read_offset(), 11);
    }

    #[test]
    fn recv_overlap_prefers_existing_bytes() {
        let mut s = CryptoRecvStream::new();
        s.recv(0, b"ABCDEF").unwrap();
        // Overlapping retransmission carrying different bytes: the already-held
        // copy wins (RFC 9000 §7.5 guarantees they are identical on the wire).
        s.recv(3, b"xyzGH").unwrap();
        assert_eq!(s.read(), b"ABCDEFGH");
    }

    #[test]
    fn recv_duplicate_below_cursor_is_clipped() {
        let mut s = CryptoRecvStream::new();
        s.recv(0, b"hello").unwrap();
        assert_eq!(s.read(), b"hello");
        // A pure retransmission of already-read bytes adds nothing.
        s.recv(0, b"hello").unwrap();
        assert!(!s.is_readable());
        assert_eq!(s.read_offset(), 5);
        // A partly-overlapping retransmission delivers only the new tail.
        s.recv(3, b"loWORLD").unwrap();
        assert_eq!(s.read(), b"WORLD");
        assert_eq!(s.read_offset(), 10);
    }

    #[test]
    fn recv_three_disjoint_segments_merge() {
        let mut s = CryptoRecvStream::new();
        s.recv(0, b"aa").unwrap();
        s.recv(4, b"cc").unwrap();
        // Bridge segment closes both gaps at once.
        s.recv(2, b"bb").unwrap();
        assert_eq!(s.read(), b"aabbcc");
    }

    #[test]
    fn recv_empty_frame_is_noop() {
        let mut s = CryptoRecvStream::new();
        s.recv(0, b"").unwrap();
        assert_eq!(s.highest_received(), 0);
        assert!(!s.is_readable());
    }

    #[test]
    fn recv_buffer_bound_rejects_far_offset() {
        let mut s = CryptoRecvStream::with_buffer_limit(16);
        // A gap that reaches past the bound is refused.
        let err = s.recv(12, b"12345").unwrap_err();
        assert_eq!(err, CryptoStreamError::BufferExceeded { offset: 17, limit: 16 });
        assert_eq!(err.code(), CRYPTO_BUFFER_EXCEEDED);
    }

    #[test]
    fn recv_buffer_bound_moves_with_read_cursor() {
        let mut s = CryptoRecvStream::with_buffer_limit(8);
        s.recv(0, b"abcd").unwrap();
        assert_eq!(s.read(), b"abcd");
        // read_offset is now 4, so the bound reaches to offset 12; a segment at
        // [8,12) that would have been rejected from offset 0 now fits.
        s.recv(8, b"wxyz").unwrap();
        assert_eq!(s.highest_received(), 12);
    }

    #[test]
    fn recv_frame_dispatch() {
        let mut s = CryptoRecvStream::new();
        let f = quic_frame::Frame::Crypto { offset: 0, data: b"hi".to_vec() };
        s.recv_frame(&f).unwrap();
        // A non-CRYPTO frame is ignored, not an error.
        s.recv_frame(&quic_frame::Frame::Ping).unwrap();
        assert_eq!(s.read(), b"hi");
    }

    // ── CryptoSendStream ────────────────────────────────────────────────────

    #[test]
    fn send_chunks_bounded_by_max_len() {
        let mut s = CryptoSendStream::new();
        s.write(b"ClientHello-bytes");
        assert_eq!(s.write_offset(), 17);
        let c0 = s.poll_transmit(5).unwrap();
        assert_eq!(c0, CryptoChunk { offset: 0, data: b"Clien".to_vec() });
        let c1 = s.poll_transmit(100).unwrap();
        assert_eq!(c1, CryptoChunk { offset: 5, data: b"tHello-bytes".to_vec() });
        assert!(s.poll_transmit(100).is_none());
        assert_eq!(s.send_offset(), 17);
    }

    #[test]
    fn send_write_after_drain_continues_offset() {
        let mut s = CryptoSendStream::new();
        s.write(b"aaa");
        let _ = s.poll_transmit(100).unwrap();
        // A later flight (e.g. the client Finished) continues the offset space.
        s.write(b"bbbb");
        let c = s.poll_transmit(100).unwrap();
        assert_eq!(c, CryptoChunk { offset: 3, data: b"bbbb".to_vec() });
    }

    #[test]
    fn send_empty_write_and_poll_are_noops() {
        let mut s = CryptoSendStream::new();
        s.write(b"");
        assert!(!s.has_unsent());
        assert!(s.poll_transmit(10).is_none());
    }

    #[test]
    fn send_fully_acked_tracks_contiguous_ack() {
        let mut s = CryptoSendStream::new();
        s.write(b"handshake");
        let c = s.poll_transmit(100).unwrap();
        assert!(!s.is_fully_acked());
        // Out-of-order partial ack leaves a gap: not fully acked yet.
        s.on_ack(4, 5);
        assert!(!s.is_fully_acked());
        // The remaining prefix closes the gap.
        s.on_ack(c.offset, 4);
        assert!(s.is_fully_acked());
    }

    #[test]
    fn send_not_fully_acked_while_unsent_remains() {
        let mut s = CryptoSendStream::new();
        s.write(b"abcdef");
        let c = s.poll_transmit(3).unwrap();
        s.on_ack(c.offset, c.data.len() as u64);
        // Everything emitted is acked, but bytes remain unsent.
        assert!(s.has_unsent());
        assert!(!s.is_fully_acked());
    }

    #[test]
    fn send_chunk_into_frame_round_trips_through_recv() {
        let mut send = CryptoSendStream::new();
        send.write(b"\x08\x00\x00\x02EncryptedExtensions");
        let mut recv = CryptoRecvStream::new();
        // Emit in tiny chunks and reassemble on the receive side.
        while let Some(chunk) = send.poll_transmit(4) {
            let frame = chunk.into_frame();
            recv.recv_frame(&frame).unwrap();
        }
        assert_eq!(recv.read(), b"\x08\x00\x00\x02EncryptedExtensions");
    }
}
