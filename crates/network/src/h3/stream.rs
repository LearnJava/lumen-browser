//! QUIC stream data model — reassembly, flow control, and stream state
//! machines (RFC 9000 §2, §3, §4).
//!
//! Slices 1–9 built the wire codecs (QUIC varints, transport and HTTP/3 frames,
//! packet headers, QPACK) and the loss-recovery control logic (RTT, congestion,
//! sent-packet registry, PTO timer). This slice adds the piece that sits between
//! the decoded [`quic_frame::Frame::Stream`] frames and the HTTP/3 layer: the
//! per-stream **receive reassembly buffer**, the **flow-control** accounting on
//! both directions (RFC 9000 §4), and the **send/receive stream state machines**
//! (RFC 9000 §3). Like every slice so far it is a pure state machine — no IO, no
//! packet protection, no timers of its own. The caller drives it with decoded
//! STREAM / RESET_STREAM / MAX_STREAM_DATA frames and reads back ordered
//! application bytes plus the STREAM frames to transmit.
//!
//! ## Stream identifiers (RFC 9000 §2.1)
//!
//! A stream ID's two least-significant bits encode its initiator and
//! directionality: bit 0 is `0` for client-initiated and `1` for
//! server-initiated streams, bit 1 is `0` for bidirectional and `1` for
//! unidirectional. [`is_client_initiated`], [`is_server_initiated`],
//! [`is_bidirectional`], and [`is_unidirectional`] decode those bits; HTTP/3
//! uses them to tell a client request stream from a server push or a QPACK
//! control stream.
//!
//! ## Receive stream ([`RecvStream`], RFC 9000 §3.2, §2.2)
//!
//! STREAM frames may arrive out of order, overlap a retransmission, or repeat
//! data already delivered. [`RecvStream::recv`] clips already-read bytes, merges
//! the segment into the buffered set (preferring bytes already held, which QUIC
//! guarantees are identical, RFC 9000 §2.2), and enforces the receive
//! flow-control limit (RFC 9000 §4.1) and final-size invariants (RFC 9000 §4.5).
//! [`RecvStream::read`] pops the contiguous prefix and advances the read cursor;
//! [`RecvStream::window_update`] re-advertises the limit as the application
//! consumes data (the trigger for a MAX_STREAM_DATA frame in a later slice).
//!
//! ## Send stream ([`SendStream`], RFC 9000 §3.1, §2.2)
//!
//! [`SendStream::write`] queues application bytes; [`SendStream::poll_transmit`]
//! hands back the next STREAM frame bounded by both a caller size cap and the
//! peer's flow-control limit (RFC 9000 §4.1); [`SendStream::on_ack`] tracks the
//! acknowledged byte ranges so the state advances to `DataRecvd` once every byte
//! (and the FIN) is acknowledged. Retransmission of lost stream data is the loss
//! layer's job (a later slice); this module only models the send buffer, flow
//! control, and state.
//!
//! ## Out of scope (later slices)
//!
//! - Connection-level flow control (RFC 9000 §4.1, the MAX_DATA budget shared
//!   across streams) and the stream-count limits (RFC 9000 §4.6). This module is
//!   strictly per-stream.
//! - Retransmission / packetization of the STREAM frames it produces, and the
//!   mapping from ACK frames to `on_ack` calls (the loss layer's role).
//! - Any IO, header protection, AEAD, or TLS.

use std::collections::BTreeMap;

// ── Wire error codes (RFC 9000 §20.1) ───────────────────────────────────────

/// `FLOW_CONTROL_ERROR` — the peer sent data beyond an advertised flow-control
/// limit (RFC 9000 §20.1).
pub const FLOW_CONTROL_ERROR: u64 = 0x03;

/// `FINAL_SIZE_ERROR` — the peer violated the final-size invariants of a stream
/// (RFC 9000 §20.1, §4.5).
pub const FINAL_SIZE_ERROR: u64 = 0x06;

/// A stream-layer protocol violation. Each variant maps to a single QUIC
/// connection-error code via [`StreamError::code`]; the variant preserves *why*
/// for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// Received data pushed the highest offset past the receive flow-control
    /// limit (RFC 9000 §4.1). Maps to [`FLOW_CONTROL_ERROR`].
    FlowControl {
        /// Highest byte offset the peer tried to reach.
        offset: u64,
        /// The receive limit that was exceeded.
        limit: u64,
    },
    /// A final size was signalled that conflicts with data already received, or
    /// two final sizes disagreed (RFC 9000 §4.5). Maps to [`FINAL_SIZE_ERROR`].
    FinalSize {
        /// The final size being asserted now.
        offset: u64,
    },
}

impl StreamError {
    /// The RFC 9000 §20.1 connection-error code this violation maps to.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::FlowControl { .. } => FLOW_CONTROL_ERROR,
            Self::FinalSize { .. } => FINAL_SIZE_ERROR,
        }
    }
}

impl core::fmt::Display for StreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FlowControl { offset, limit } => {
                write!(f, "QUIC stream: offset {offset} exceeds flow-control limit {limit}")
            }
            Self::FinalSize { offset } => {
                write!(f, "QUIC stream: final size {offset} violates a known final size")
            }
        }
    }
}

impl std::error::Error for StreamError {}

// ── Stream identifiers (RFC 9000 §2.1) ──────────────────────────────────────

/// Whether a stream was initiated by the client (bit 0 clear, RFC 9000 §2.1).
#[must_use]
pub const fn is_client_initiated(stream_id: u64) -> bool {
    stream_id & 0x1 == 0
}

/// Whether a stream was initiated by the server (bit 0 set, RFC 9000 §2.1).
#[must_use]
pub const fn is_server_initiated(stream_id: u64) -> bool {
    stream_id & 0x1 != 0
}

/// Whether a stream is bidirectional (bit 1 clear, RFC 9000 §2.1).
#[must_use]
pub const fn is_bidirectional(stream_id: u64) -> bool {
    stream_id & 0x2 == 0
}

/// Whether a stream is unidirectional (bit 1 set, RFC 9000 §2.1).
#[must_use]
pub const fn is_unidirectional(stream_id: u64) -> bool {
    stream_id & 0x2 != 0
}

// ── Receive stream (RFC 9000 §3.2) ──────────────────────────────────────────

/// The state of the receiving part of a stream (RFC 9000 §3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecvState {
    /// Data is being received; the final size is not yet known.
    Recv,
    /// A FIN has fixed the final size, but not all data has arrived yet.
    SizeKnown,
    /// Every byte up to the final size has been received (but not all read).
    DataRecvd,
    /// The application has read every byte up to the final size.
    DataRead,
    /// A RESET_STREAM was received; the stream is reset (but not yet observed
    /// by the application).
    ResetRecvd,
    /// The application has observed the reset.
    ResetRead,
}

/// The receiving half of a QUIC stream: reassembly buffer, receive flow-control
/// accounting, and the RFC 9000 §3.2 state machine.
#[derive(Clone, Debug)]
pub struct RecvStream {
    /// Current receive state (RFC 9000 §3.2).
    state: RecvState,
    /// Buffered received segments at or beyond [`Self::read_offset`], keyed by
    /// start offset. Segments are kept disjoint and non-adjacent-merged so the
    /// segment at key `read_offset` (if any) spans the whole readable prefix.
    buffered: BTreeMap<u64, Vec<u8>>,
    /// The next byte offset the application will read; all bytes below have been
    /// delivered by [`Self::read`].
    read_offset: u64,
    /// One past the highest byte offset ever received, for flow control
    /// (RFC 9000 §4.1).
    highest_received: u64,
    /// The receive flow-control limit we have advertised (MAX_STREAM_DATA).
    max_data: u64,
    /// The final size once known via a FIN or RESET_STREAM (RFC 9000 §4.5).
    final_size: Option<u64>,
    /// The application error code carried by a RESET_STREAM, if reset.
    reset_error: Option<u64>,
}

impl RecvStream {
    /// Creates a receive stream advertising `initial_max_data` bytes of receive
    /// flow-control window (the peer's `initial_max_stream_data` transport
    /// parameter, RFC 9000 §18.2).
    #[must_use]
    pub fn new(initial_max_data: u64) -> Self {
        Self {
            state: RecvState::Recv,
            buffered: BTreeMap::new(),
            read_offset: 0,
            highest_received: 0,
            max_data: initial_max_data,
            final_size: None,
            reset_error: None,
        }
    }

    /// The current receive state (RFC 9000 §3.2).
    #[must_use]
    pub fn state(&self) -> RecvState {
        self.state
    }

    /// The next offset the application will read (bytes below are delivered).
    #[must_use]
    pub fn read_offset(&self) -> u64 {
        self.read_offset
    }

    /// The currently advertised receive flow-control limit (RFC 9000 §4.1).
    #[must_use]
    pub fn max_data(&self) -> u64 {
        self.max_data
    }

    /// Whether contiguous data is available to [`Self::read`].
    #[must_use]
    pub fn is_readable(&self) -> bool {
        self.buffered.contains_key(&self.read_offset)
    }

    /// The application error code if the stream was reset (RFC 9000 §19.4).
    #[must_use]
    pub fn reset_error(&self) -> Option<u64> {
        self.reset_error
    }

    /// Whether the application has consumed the whole stream (`DataRead`).
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.state == RecvState::DataRead
    }

    /// Processes a received STREAM frame (RFC 9000 §19.8): `offset`/`data` is the
    /// carried byte range and `fin` marks the final frame.
    ///
    /// Enforces the receive flow-control limit (RFC 9000 §4.1) and the final-size
    /// invariants (RFC 9000 §4.5): returns [`StreamError::FlowControl`] if the
    /// data exceeds the advertised limit, or [`StreamError::FinalSize`] if a FIN
    /// disagrees with an earlier final size or with data already received. A frame
    /// arriving after the stream is reset is ignored.
    pub fn recv(&mut self, offset: u64, data: &[u8], fin: bool) -> Result<(), StreamError> {
        if matches!(self.state, RecvState::ResetRecvd | RecvState::ResetRead) {
            return Ok(());
        }
        let end = offset.saturating_add(data.len() as u64);

        // Flow control (RFC 9000 §4.1): the highest received offset must stay
        // within the advertised limit.
        if end > self.max_data {
            return Err(StreamError::FlowControl { offset: end, limit: self.max_data });
        }

        // Final-size checks (RFC 9000 §4.5).
        if let Some(known) = self.final_size {
            // No byte may be received past a known final size.
            if end > known {
                return Err(StreamError::FinalSize { offset: end });
            }
            // A FIN must agree with the known final size.
            if fin && end != known {
                return Err(StreamError::FinalSize { offset: end });
            }
        }
        if fin {
            // A FIN's final size must not contradict data already received.
            if end < self.highest_received {
                return Err(StreamError::FinalSize { offset: end });
            }
            match self.final_size {
                Some(known) if known != end => {
                    return Err(StreamError::FinalSize { offset: end });
                }
                _ => self.final_size = Some(end),
            }
        }

        self.highest_received = self.highest_received.max(end);
        if !data.is_empty() {
            self.insert_segment(offset, data);
        }
        self.recompute_recv_state();
        Ok(())
    }

    /// Processes a received RESET_STREAM frame (RFC 9000 §19.4): the peer aborts
    /// the sending half at `final_size` with `error_code`.
    ///
    /// Returns [`StreamError::FinalSize`] if the reset's final size contradicts a
    /// previously known final size or data already received (RFC 9000 §4.5). A
    /// reset after the stream is already reset is ignored.
    pub fn recv_reset(&mut self, final_size: u64, error_code: u64) -> Result<(), StreamError> {
        if matches!(self.state, RecvState::ResetRecvd | RecvState::ResetRead) {
            return Ok(());
        }
        if final_size < self.highest_received {
            return Err(StreamError::FinalSize { offset: final_size });
        }
        if let Some(known) = self.final_size
            && known != final_size
        {
            return Err(StreamError::FinalSize { offset: final_size });
        }
        self.final_size = Some(final_size);
        self.reset_error = Some(error_code);
        self.state = RecvState::ResetRecvd;
        Ok(())
    }

    /// Pops and returns the contiguous readable prefix, advancing the read
    /// cursor. Returns an empty vector when no contiguous data is available (a
    /// gap precedes the next buffered bytes). Reading the last byte of a
    /// finished stream advances the state to `DataRead`; observing a reset
    /// advances it to `ResetRead`.
    pub fn read(&mut self) -> Vec<u8> {
        if self.state == RecvState::ResetRecvd {
            self.state = RecvState::ResetRead;
            return Vec::new();
        }
        let Some(chunk) = self.buffered.remove(&self.read_offset) else {
            return Vec::new();
        };
        self.read_offset = self.read_offset.saturating_add(chunk.len() as u64);
        self.recompute_recv_state();
        chunk
    }

    /// Re-advertises the receive flow-control limit as `read_offset + window`
    /// and returns the new limit (RFC 9000 §4.1). The limit only ever grows.
    /// The caller sends a MAX_STREAM_DATA frame with the returned value.
    pub fn window_update(&mut self, window: u64) -> u64 {
        let candidate = self.read_offset.saturating_add(window);
        self.max_data = self.max_data.max(candidate);
        self.max_data
    }

    /// Merges `data` at `offset` into the buffered set, clipping bytes already
    /// read and preferring bytes already buffered on overlap (RFC 9000 §2.2
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

    /// One past the highest contiguously-received offset from the read cursor,
    /// i.e. `read_offset + len(readable prefix)`.
    fn contiguous_end(&self) -> u64 {
        match self.buffered.get(&self.read_offset) {
            Some(seg) => self.read_offset + seg.len() as u64,
            None => self.read_offset,
        }
    }

    /// Recomputes the receive state from the buffered data and the final size
    /// (RFC 9000 §3.2). Never leaves a reset state.
    fn recompute_recv_state(&mut self) {
        if matches!(self.state, RecvState::ResetRecvd | RecvState::ResetRead) {
            return;
        }
        let Some(final_size) = self.final_size else {
            self.state = RecvState::Recv;
            return;
        };
        // All data has been read once the read cursor reaches the final size.
        if self.read_offset >= final_size {
            self.state = RecvState::DataRead;
        } else if self.contiguous_end() >= final_size {
            // Every byte received, but the tail is unread.
            self.state = RecvState::DataRecvd;
        } else {
            self.state = RecvState::SizeKnown;
        }
    }
}

// ── Send stream (RFC 9000 §3.1) ─────────────────────────────────────────────

/// The state of the sending part of a stream (RFC 9000 §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendState {
    /// No stream data has been sent yet.
    Ready,
    /// Stream data is being sent (some data remains unacknowledged).
    Send,
    /// All stream data and the FIN have been emitted, but not all acknowledged.
    DataSent,
    /// Every byte and the FIN have been acknowledged.
    DataRecvd,
    /// A RESET_STREAM has been sent.
    ResetSent,
    /// The RESET_STREAM has been acknowledged.
    ResetRecvd,
}

/// A STREAM frame to transmit, produced by [`SendStream::poll_transmit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamChunk {
    /// Byte offset of `data` on the stream.
    pub offset: u64,
    /// The stream data to send.
    pub data: Vec<u8>,
    /// Whether this chunk carries the FIN bit.
    pub fin: bool,
}

/// The sending half of a QUIC stream: outgoing buffer, send flow-control
/// accounting, and the RFC 9000 §3.1 state machine.
#[derive(Clone, Debug)]
pub struct SendStream {
    /// Current send state (RFC 9000 §3.1).
    state: SendState,
    /// Application bytes queued but not yet emitted in a STREAM frame.
    unsent: Vec<u8>,
    /// Offset of the first byte in [`Self::unsent`] — the next offset to emit.
    send_offset: u64,
    /// The peer's flow-control limit: the highest offset we may send to
    /// (their `initial_max_stream_data` raised by MAX_STREAM_DATA, RFC 9000 §4.1).
    max_data: u64,
    /// Whether the application has requested a FIN (RFC 9000 §3.1).
    fin_requested: bool,
    /// Whether the FIN has been emitted in a chunk.
    fin_sent: bool,
    /// Acknowledged byte ranges, keyed by start offset, merged and disjoint.
    acked: BTreeMap<u64, u64>,
    /// Whether the FIN has been acknowledged.
    fin_acked: bool,
    /// The application error code carried by a sent RESET_STREAM, if reset.
    reset_error: Option<u64>,
}

impl SendStream {
    /// Creates a send stream bounded by `initial_max_data` (the peer's
    /// `initial_max_stream_data` transport parameter, RFC 9000 §18.2).
    #[must_use]
    pub fn new(initial_max_data: u64) -> Self {
        Self {
            state: SendState::Ready,
            unsent: Vec::new(),
            send_offset: 0,
            max_data: initial_max_data,
            fin_requested: false,
            fin_sent: false,
            acked: BTreeMap::new(),
            fin_acked: false,
            reset_error: None,
        }
    }

    /// The current send state (RFC 9000 §3.1).
    #[must_use]
    pub fn state(&self) -> SendState {
        self.state
    }

    /// The peer's current flow-control limit for this stream (RFC 9000 §4.1).
    #[must_use]
    pub fn max_data(&self) -> u64 {
        self.max_data
    }

    /// The total number of bytes written by the application so far.
    #[must_use]
    pub fn write_offset(&self) -> u64 {
        self.send_offset + self.unsent.len() as u64
    }

    /// Queues application `data` for transmission (RFC 9000 §3.1). Ignored once
    /// a FIN has been requested or the stream has been reset.
    pub fn write(&mut self, data: &[u8]) {
        if self.fin_requested || self.reset_error.is_some() {
            return;
        }
        if data.is_empty() {
            return;
        }
        self.unsent.extend_from_slice(data);
        if self.state == SendState::Ready {
            self.state = SendState::Send;
        }
    }

    /// Marks the end of the stream (RFC 9000 §3.1). No further [`Self::write`]
    /// is accepted. If all data was already emitted, the FIN is still pending a
    /// [`Self::poll_transmit`] to carry it.
    pub fn finish(&mut self) {
        if self.reset_error.is_some() {
            return;
        }
        self.fin_requested = true;
        if self.state == SendState::Ready {
            self.state = SendState::Send;
        }
    }

    /// Raises the peer's flow-control limit from a received MAX_STREAM_DATA
    /// frame (RFC 9000 §19.10). The limit only ever grows.
    pub fn update_max_data(&mut self, new_max: u64) {
        self.max_data = self.max_data.max(new_max);
    }

    /// One past the highest offset the peer's flow-control limit lets us send.
    /// Equal to [`Self::max_data`]; named for the framing arithmetic.
    #[must_use]
    fn send_window_end(&self) -> u64 {
        self.max_data
    }

    /// Whether the stream has unsent data but is blocked by flow control
    /// (RFC 9000 §4.1) — the trigger for a STREAM_DATA_BLOCKED frame.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        !self.unsent.is_empty() && self.send_offset >= self.send_window_end()
    }

    /// The offset at which the sender is flow-control blocked, if blocked
    /// (RFC 9000 §19.13).
    #[must_use]
    pub fn blocked_at(&self) -> Option<u64> {
        self.is_blocked().then_some(self.max_data)
    }

    /// Produces the next STREAM frame to transmit, at most `max_len` data bytes
    /// and no further than the peer's flow-control limit (RFC 9000 §4.1). The
    /// FIN is attached to the chunk that carries the last byte, or emitted alone
    /// when the application finished an already-drained stream.
    ///
    /// Returns `None` when there is nothing to send (no data within the window
    /// and no pending FIN), or once the stream is reset.
    pub fn poll_transmit(&mut self, max_len: usize) -> Option<StreamChunk> {
        if matches!(self.state, SendState::ResetSent | SendState::ResetRecvd) {
            return None;
        }
        // How many bytes the flow-control window still permits from send_offset.
        let window = self.send_window_end().saturating_sub(self.send_offset);
        let take = (self.unsent.len() as u64).min(window).min(max_len as u64) as usize;

        // Nothing to send unless there is data within the window or a lone FIN.
        let fin_only = self.unsent.is_empty() && self.fin_requested && !self.fin_sent;
        if take == 0 && !fin_only {
            return None;
        }

        let offset = self.send_offset;
        let data: Vec<u8> = self.unsent.drain(..take).collect();
        self.send_offset += take as u64;

        // The FIN rides the chunk that empties the send buffer.
        let fin = self.fin_requested && self.unsent.is_empty();
        if fin {
            self.fin_sent = true;
        }
        if self.fin_sent && self.unsent.is_empty() {
            self.state = SendState::DataSent;
        }
        Some(StreamChunk { offset, data, fin })
    }

    /// Records that the byte range `[offset, offset + len)` was acknowledged,
    /// and whether that chunk's FIN was acknowledged. Advances the state to
    /// `DataRecvd` once every byte and the FIN are acknowledged (RFC 9000 §3.1).
    pub fn on_ack(&mut self, offset: u64, len: u64, fin: bool) {
        if matches!(self.state, SendState::ResetSent | SendState::ResetRecvd) {
            return;
        }
        if len > 0 {
            insert_range(&mut self.acked, offset, offset + len);
        }
        if fin {
            self.fin_acked = true;
        }
        // DataRecvd requires the FIN sent, the FIN acked, and every data byte
        // (0..final_size) contiguously acknowledged.
        if self.fin_sent && self.fin_acked && contiguous_end(&self.acked) >= self.send_offset {
            self.state = SendState::DataRecvd;
        }
    }

    /// Abruptly terminates the sending half with `error_code`, discarding any
    /// unsent data and moving to `ResetSent` (RFC 9000 §3.1, §19.4).
    pub fn reset(&mut self, error_code: u64) {
        if matches!(self.state, SendState::DataRecvd | SendState::ResetRecvd) {
            return;
        }
        self.unsent.clear();
        self.reset_error = Some(error_code);
        self.state = SendState::ResetSent;
    }

    /// Acknowledges the RESET_STREAM, moving to `ResetRecvd` (RFC 9000 §3.1).
    pub fn on_reset_ack(&mut self) {
        if self.state == SendState::ResetSent {
            self.state = SendState::ResetRecvd;
        }
    }

    /// The application error code if the stream was reset (RFC 9000 §19.4).
    #[must_use]
    pub fn reset_error(&self) -> Option<u64> {
        self.reset_error
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

    // ── Stream identifiers (RFC 9000 §2.1) ──────────────────────────────────

    #[test]
    fn stream_id_bits() {
        // 0x00 client bidi, 0x01 server bidi, 0x02 client uni, 0x03 server uni.
        assert!(is_client_initiated(0) && is_bidirectional(0));
        assert!(is_server_initiated(1) && is_bidirectional(1));
        assert!(is_client_initiated(2) && is_unidirectional(2));
        assert!(is_server_initiated(3) && is_unidirectional(3));
        assert!(is_client_initiated(4) && is_bidirectional(4));
    }

    // ── RecvStream reassembly ───────────────────────────────────────────────

    #[test]
    fn recv_in_order() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"hello ", false).unwrap();
        s.recv(6, b"world", false).unwrap();
        assert!(s.is_readable());
        assert_eq!(s.read(), b"hello world");
        assert_eq!(s.read_offset(), 11);
        assert!(!s.is_readable());
    }

    #[test]
    fn recv_out_of_order_holds_until_gap_filled() {
        let mut s = RecvStream::new(1024);
        s.recv(6, b"world", false).unwrap();
        // Nothing readable while the [0,6) gap is open.
        assert!(!s.is_readable());
        assert_eq!(s.read(), b"");
        s.recv(0, b"hello ", false).unwrap();
        assert_eq!(s.read(), b"hello world");
    }

    #[test]
    fn recv_overlapping_retransmit_prefers_existing() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"abcdef", false).unwrap();
        // Overlapping retransmit with identical data — merged, no duplication.
        s.recv(3, b"defgh", false).unwrap();
        assert_eq!(s.read(), b"abcdefgh");
    }

    #[test]
    fn recv_duplicate_already_read_is_ignored() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"abcd", false).unwrap();
        assert_eq!(s.read(), b"abcd");
        // Re-delivering already-read bytes yields nothing new.
        s.recv(0, b"abcd", false).unwrap();
        assert_eq!(s.read(), b"");
        assert_eq!(s.read_offset(), 4);
    }

    #[test]
    fn recv_partial_already_read_clipped() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"abcd", false).unwrap();
        assert_eq!(s.read(), b"abcd");
        // Half old, half new: only the new tail is buffered.
        s.recv(2, b"cdef", false).unwrap();
        assert_eq!(s.read(), b"ef");
    }

    #[test]
    fn recv_three_segments_merge() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"aa", false).unwrap();
        s.recv(4, b"cc", false).unwrap();
        s.recv(2, b"bb", false).unwrap();
        assert_eq!(s.read(), b"aabbcc");
    }

    // ── RecvStream flow control (RFC 9000 §4.1) ──────────────────────────────

    #[test]
    fn recv_flow_control_violation() {
        let mut s = RecvStream::new(4);
        let err = s.recv(0, b"abcde", false).unwrap_err();
        assert_eq!(err, StreamError::FlowControl { offset: 5, limit: 4 });
        assert_eq!(err.code(), FLOW_CONTROL_ERROR);
    }

    #[test]
    fn recv_at_exact_limit_ok() {
        let mut s = RecvStream::new(4);
        s.recv(0, b"abcd", false).unwrap();
        assert_eq!(s.read(), b"abcd");
    }

    #[test]
    fn window_update_grows_limit() {
        let mut s = RecvStream::new(4);
        s.recv(0, b"abcd", false).unwrap();
        assert_eq!(s.read(), b"abcd");
        // After reading 4 bytes, re-advertise a 4-byte window → limit 8.
        assert_eq!(s.window_update(4), 8);
        assert_eq!(s.max_data(), 8);
        s.recv(4, b"efgh", false).unwrap();
        assert_eq!(s.read(), b"efgh");
    }

    #[test]
    fn window_update_never_shrinks() {
        let mut s = RecvStream::new(100);
        assert_eq!(s.window_update(4), 100);
    }

    // ── RecvStream final size + state (RFC 9000 §3.2, §4.5) ──────────────────

    #[test]
    fn recv_fin_drives_state_to_dataread() {
        let mut s = RecvStream::new(1024);
        assert_eq!(s.state(), RecvState::Recv);
        s.recv(0, b"hi", true).unwrap();
        assert_eq!(s.state(), RecvState::DataRecvd);
        assert_eq!(s.read(), b"hi");
        assert_eq!(s.state(), RecvState::DataRead);
        assert!(s.is_finished());
    }

    #[test]
    fn recv_fin_with_gap_is_sizeknown() {
        let mut s = RecvStream::new(1024);
        s.recv(4, b"cd", true).unwrap();
        // Final size known (6) but [0,4) missing.
        assert_eq!(s.state(), RecvState::SizeKnown);
        s.recv(0, b"abcd", false).unwrap();
        assert_eq!(s.state(), RecvState::DataRecvd);
    }

    #[test]
    fn recv_empty_fin_marks_end() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"data", false).unwrap();
        assert_eq!(s.read(), b"data");
        // Empty STREAM frame carrying only the FIN at the current offset.
        s.recv(4, b"", true).unwrap();
        assert_eq!(s.state(), RecvState::DataRead);
    }

    #[test]
    fn recv_data_past_final_size_errs() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"hi", true).unwrap();
        let err = s.recv(2, b"more", false).unwrap_err();
        assert_eq!(err, StreamError::FinalSize { offset: 6 });
        assert_eq!(err.code(), FINAL_SIZE_ERROR);
    }

    #[test]
    fn recv_conflicting_fin_size_errs() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"abcd", false).unwrap();
        // FIN claiming a final size below already-received data.
        let err = s.recv(0, b"ab", true).unwrap_err();
        assert_eq!(err, StreamError::FinalSize { offset: 2 });
    }

    // ── RecvStream reset (RFC 9000 §3.2, §19.4) ──────────────────────────────

    #[test]
    fn recv_reset_transitions() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"partial", false).unwrap();
        s.recv_reset(7, 0x101).unwrap();
        assert_eq!(s.state(), RecvState::ResetRecvd);
        assert_eq!(s.reset_error(), Some(0x101));
        // Reading observes the reset and yields no data.
        assert_eq!(s.read(), b"");
        assert_eq!(s.state(), RecvState::ResetRead);
    }

    #[test]
    fn recv_reset_below_received_errs() {
        let mut s = RecvStream::new(1024);
        s.recv(0, b"abcdef", false).unwrap();
        let err = s.recv_reset(3, 0).unwrap_err();
        assert_eq!(err, StreamError::FinalSize { offset: 3 });
    }

    #[test]
    fn recv_after_reset_ignored() {
        let mut s = RecvStream::new(1024);
        s.recv_reset(0, 0).unwrap();
        // Late STREAM frame after reset is silently dropped, not an error.
        s.recv(0, b"late", false).unwrap();
        assert_eq!(s.state(), RecvState::ResetRecvd);
    }

    // ── SendStream (RFC 9000 §3.1) ───────────────────────────────────────────

    #[test]
    fn send_basic_transmit() {
        let mut s = SendStream::new(1024);
        assert_eq!(s.state(), SendState::Ready);
        s.write(b"hello world");
        assert_eq!(s.state(), SendState::Send);
        let chunk = s.poll_transmit(5).unwrap();
        assert_eq!(chunk, StreamChunk { offset: 0, data: b"hello".to_vec(), fin: false });
        let chunk = s.poll_transmit(100).unwrap();
        assert_eq!(chunk, StreamChunk { offset: 5, data: b" world".to_vec(), fin: false });
        assert!(s.poll_transmit(100).is_none());
    }

    #[test]
    fn send_fin_rides_last_chunk() {
        let mut s = SendStream::new(1024);
        s.write(b"data");
        s.finish();
        let chunk = s.poll_transmit(100).unwrap();
        assert_eq!(chunk, StreamChunk { offset: 0, data: b"data".to_vec(), fin: true });
        assert_eq!(s.state(), SendState::DataSent);
        assert!(s.poll_transmit(100).is_none());
    }

    #[test]
    fn send_lone_fin_after_drain() {
        let mut s = SendStream::new(1024);
        s.write(b"data");
        let chunk = s.poll_transmit(100).unwrap();
        assert!(!chunk.fin);
        // finish() after data drained → a lone FIN chunk.
        s.finish();
        let chunk = s.poll_transmit(100).unwrap();
        assert_eq!(chunk, StreamChunk { offset: 4, data: vec![], fin: true });
        assert_eq!(s.state(), SendState::DataSent);
    }

    #[test]
    fn send_flow_control_caps_chunk() {
        let mut s = SendStream::new(4);
        s.write(b"abcdefgh");
        let chunk = s.poll_transmit(100).unwrap();
        // Only 4 bytes permitted by the peer's limit.
        assert_eq!(chunk.data, b"abcd");
        assert!(s.poll_transmit(100).is_none());
        assert!(s.is_blocked());
        assert_eq!(s.blocked_at(), Some(4));
        // Peer raises the limit → the rest flows.
        s.update_max_data(8);
        assert!(!s.is_blocked());
        let chunk = s.poll_transmit(100).unwrap();
        assert_eq!(chunk, StreamChunk { offset: 4, data: b"efgh".to_vec(), fin: false });
    }

    #[test]
    fn send_max_data_never_shrinks() {
        let mut s = SendStream::new(100);
        s.update_max_data(4);
        assert_eq!(s.max_data(), 100);
    }

    #[test]
    fn send_ack_reaches_datarecvd() {
        let mut s = SendStream::new(1024);
        s.write(b"hello");
        s.finish();
        let chunk = s.poll_transmit(100).unwrap();
        assert!(chunk.fin);
        assert_eq!(s.state(), SendState::DataSent);
        s.on_ack(0, 5, true);
        assert_eq!(s.state(), SendState::DataRecvd);
    }

    #[test]
    fn send_partial_ack_stays_datasent() {
        let mut s = SendStream::new(1024);
        s.write(b"hello");
        s.finish();
        s.poll_transmit(100).unwrap();
        // FIN acked but a data gap remains → not yet DataRecvd.
        s.on_ack(2, 3, true);
        assert_eq!(s.state(), SendState::DataSent);
        s.on_ack(0, 2, false);
        assert_eq!(s.state(), SendState::DataRecvd);
    }

    #[test]
    fn send_reset_transitions() {
        let mut s = SendStream::new(1024);
        s.write(b"data");
        s.reset(0x99);
        assert_eq!(s.state(), SendState::ResetSent);
        assert_eq!(s.reset_error(), Some(0x99));
        // No transmission after reset.
        assert!(s.poll_transmit(100).is_none());
        s.on_reset_ack();
        assert_eq!(s.state(), SendState::ResetRecvd);
    }

    #[test]
    fn send_write_after_finish_ignored() {
        let mut s = SendStream::new(1024);
        s.write(b"a");
        s.finish();
        s.write(b"b");
        assert_eq!(s.write_offset(), 1);
    }

    // ── Range helper ─────────────────────────────────────────────────────────

    #[test]
    fn insert_range_merges() {
        let mut r = BTreeMap::new();
        insert_range(&mut r, 0, 4);
        insert_range(&mut r, 4, 8); // adjacent → coalesced
        assert_eq!(r.len(), 1);
        assert_eq!(contiguous_end(&r), 8);
        insert_range(&mut r, 12, 16); // disjoint → separate
        assert_eq!(r.len(), 2);
        assert_eq!(contiguous_end(&r), 8);
        insert_range(&mut r, 6, 14); // bridges the gap
        assert_eq!(r.len(), 1);
        assert_eq!(contiguous_end(&r), 16);
    }
}
