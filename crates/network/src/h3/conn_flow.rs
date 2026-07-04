//! Connection-level flow control and stream-count limits (RFC 9000 §4.1, §4.6).
//!
//! Slice 10 ([`super::stream`]) modelled a single QUIC stream: its reassembly
//! buffer, its *per-stream* flow-control limit (`MAX_STREAM_DATA`), and its state
//! machine. This slice adds the two connection-wide budgets that sit above the
//! individual streams and were explicitly left out of scope there:
//!
//! - **Connection-level flow control** (RFC 9000 §4.1): the single `MAX_DATA`
//!   budget that caps the *sum* of stream data across every stream, independent
//!   of each stream's own `MAX_STREAM_DATA`. A sender must respect both limits;
//!   a receiver enforces the connection budget and re-advertises it as the
//!   application consumes data. Modelled by [`SendConnFlow`] (our view of the
//!   peer's limit) and [`RecvConnFlow`] (the limit we advertise and police).
//! - **Stream-count limits** (RFC 9000 §4.6): the `MAX_STREAMS` budget that caps
//!   how many streams of a given type (bidirectional or unidirectional) each
//!   endpoint may open. Modelled by [`SendStreamLimit`] (how many streams the
//!   peer lets *us* open) and [`RecvStreamLimit`] (how many we let the peer
//!   open, plus the re-advertisement as streams complete).
//!
//! Like every slice so far this is a pure state machine — no IO, no packet
//! protection, no timers. The connection layer drives it: it reports bytes sent
//! and received and streams opened and closed, and reads back the limits to put
//! in `MAX_DATA` / `MAX_STREAMS` frames and the block signals that trigger
//! `DATA_BLOCKED` / `STREAMS_BLOCKED` frames (RFC 9000 §19.9, §19.12–§19.14).
//!
//! ## Out of scope (later slices)
//!
//! - The per-stream flow control and reassembly of [`super::stream`] — this
//!   module is strictly connection-wide accounting on top of it.
//! - Emitting the actual `MAX_DATA` / `MAX_STREAMS` / `*_BLOCKED` frames (that is
//!   [`super::quic_frame`]'s codec plus the connection layer's framing loop); this
//!   module only decides the values and whether a block signal is warranted.
//! - Any IO, header protection, AEAD, or TLS.

use super::stream::FLOW_CONTROL_ERROR;

/// `STREAM_LIMIT_ERROR` — the peer opened a stream past the advertised
/// `MAX_STREAMS` limit (RFC 9000 §20.1, §4.6).
pub const STREAM_LIMIT_ERROR: u64 = 0x04;

// ── Stream directionality (RFC 9000 §2.1) ───────────────────────────────────

/// A stream's directionality, the axis `MAX_STREAMS` accounts on separately
/// (RFC 9000 §4.6): a bidirectional or a unidirectional stream count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamDir {
    /// A bidirectional stream (stream-id bit 1 clear, RFC 9000 §2.1).
    Bidi,
    /// A unidirectional stream (stream-id bit 1 set, RFC 9000 §2.1).
    Uni,
}

impl StreamDir {
    /// The directionality encoded in `stream_id`'s second-least-significant bit
    /// (RFC 9000 §2.1).
    #[must_use]
    pub const fn of(stream_id: u64) -> Self {
        if stream_id & 0x2 == 0 { Self::Bidi } else { Self::Uni }
    }
}

/// The number of streams the cumulative `MAX_STREAMS` limit accounts for a
/// stream `stream_id` to exist: `(stream_id >> 2) + 1` (RFC 9000 §4.6). A limit
/// of N permits stream IDs whose count is at most N, i.e. the low-numbered N
/// streams of that type.
#[must_use]
pub const fn stream_count(stream_id: u64) -> u64 {
    (stream_id >> 2) + 1
}

// ── Connection-level errors (RFC 9000 §20.1) ────────────────────────────────

/// A connection-level protocol violation. Each variant maps to a single QUIC
/// connection-error code via [`ConnError::code`]; the variant preserves *why*
/// for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnError {
    /// The peer sent stream data whose connection-wide total exceeded the
    /// advertised `MAX_DATA` (RFC 9000 §4.1). Maps to [`FLOW_CONTROL_ERROR`].
    FlowControl {
        /// Connection-wide total the peer tried to reach.
        received: u64,
        /// The connection flow-control limit that was exceeded.
        limit: u64,
    },
    /// The peer opened more streams of a direction than the advertised
    /// `MAX_STREAMS` (RFC 9000 §4.6). Maps to [`STREAM_LIMIT_ERROR`].
    StreamLimit {
        /// The stream count the peer tried to reach.
        count: u64,
        /// The stream-count limit that was exceeded.
        limit: u64,
        /// Which stream-count axis was exceeded.
        dir: StreamDir,
    },
}

impl ConnError {
    /// The RFC 9000 §20.1 connection-error code this violation maps to.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::FlowControl { .. } => FLOW_CONTROL_ERROR,
            Self::StreamLimit { .. } => STREAM_LIMIT_ERROR,
        }
    }
}

impl core::fmt::Display for ConnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FlowControl { received, limit } => {
                write!(f, "QUIC connection: received {received} exceeds MAX_DATA limit {limit}")
            }
            Self::StreamLimit { count, limit, dir } => {
                write!(f, "QUIC connection: {dir:?} stream count {count} exceeds MAX_STREAMS limit {limit}")
            }
        }
    }
}

impl std::error::Error for ConnError {}

// ── Send-side connection flow control (RFC 9000 §4.1) ───────────────────────

/// Our view of the peer's connection-level flow-control limit: the sum of stream
/// data we have sent across all streams, bounded by the peer's advertised
/// `MAX_DATA` (RFC 9000 §4.1). Complements each stream's own [`super::stream::
/// SendStream`] limit — a sender must respect both.
#[derive(Clone, Debug)]
pub struct SendConnFlow {
    /// Connection-wide total of stream bytes sent across all streams.
    sent: u64,
    /// The peer's advertised connection limit (their `initial_max_data`
    /// transport parameter, raised by `MAX_DATA` frames, RFC 9000 §18.2, §19.9).
    max_data: u64,
}

impl SendConnFlow {
    /// Creates the send-side accounting bounded by `initial_max_data` (the peer's
    /// `initial_max_data` transport parameter, RFC 9000 §18.2).
    #[must_use]
    pub fn new(initial_max_data: u64) -> Self {
        Self { sent: 0, max_data: initial_max_data }
    }

    /// The peer's current connection-level limit (RFC 9000 §4.1).
    #[must_use]
    pub fn max_data(&self) -> u64 {
        self.max_data
    }

    /// The connection-wide total of stream bytes sent so far.
    #[must_use]
    pub fn sent(&self) -> u64 {
        self.sent
    }

    /// How many more stream bytes the connection limit currently permits
    /// (RFC 9000 §4.1). Saturates at zero.
    #[must_use]
    pub fn available(&self) -> u64 {
        self.max_data.saturating_sub(self.sent)
    }

    /// The largest number of bytes, up to `want`, that may be sent right now
    /// without exceeding the connection limit — `min(want, available)`.
    #[must_use]
    pub fn allowed(&self, want: u64) -> u64 {
        want.min(self.available())
    }

    /// Records that `n` stream bytes were sent across the connection
    /// (RFC 9000 §4.1). The caller must not send more than [`Self::available`].
    pub fn on_sent(&mut self, n: u64) {
        self.sent = self.sent.saturating_add(n);
    }

    /// Raises the peer's connection limit from a received `MAX_DATA` frame
    /// (RFC 9000 §19.9). The limit only ever grows.
    pub fn update_max_data(&mut self, new_max: u64) {
        self.max_data = self.max_data.max(new_max);
    }

    /// Whether the connection limit is exhausted (no bytes may be sent). The
    /// trigger, when the sender also has data to send, for a `DATA_BLOCKED`
    /// frame (RFC 9000 §19.12).
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.available() == 0
    }

    /// The offset at which the sender is connection-flow-control blocked, if
    /// blocked (RFC 9000 §19.12).
    #[must_use]
    pub fn blocked_at(&self) -> Option<u64> {
        self.is_blocked().then_some(self.max_data)
    }
}

// ── Receive-side connection flow control (RFC 9000 §4.1) ────────────────────

/// The connection-level flow-control limit we advertise and police: it caps the
/// sum of stream data the peer may send across all streams (RFC 9000 §4.1). We
/// enforce the limit against the total received and re-advertise it via
/// `MAX_DATA` as the application consumes data.
#[derive(Clone, Debug)]
pub struct RecvConnFlow {
    /// Connection-wide total of the highest offsets received across all streams,
    /// the quantity the limit is enforced against (RFC 9000 §4.1).
    received: u64,
    /// Connection-wide total of stream bytes the application has consumed, the
    /// basis for re-advertising the limit.
    read: u64,
    /// The connection limit we have advertised (`MAX_DATA`).
    limit: u64,
}

impl RecvConnFlow {
    /// Creates the receive-side accounting advertising `initial_max_data` bytes
    /// (our `initial_max_data` transport parameter, RFC 9000 §18.2).
    #[must_use]
    pub fn new(initial_max_data: u64) -> Self {
        Self { received: 0, read: 0, limit: initial_max_data }
    }

    /// The connection limit currently advertised (RFC 9000 §4.1).
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// The connection-wide total of the highest offsets received so far.
    #[must_use]
    pub fn received(&self) -> u64 {
        self.received
    }

    /// The connection-wide total of bytes the application has consumed so far.
    #[must_use]
    pub fn read(&self) -> u64 {
        self.read
    }

    /// Records that a stream's highest received offset advanced by `delta` bytes
    /// (the caller computes `delta` from a stream's [`super::stream::RecvStream`]
    /// as its highest offset grows). Returns [`ConnError::FlowControl`] if the
    /// connection-wide total now exceeds the advertised limit (RFC 9000 §4.1).
    pub fn record_received(&mut self, delta: u64) -> Result<(), ConnError> {
        self.received = self.received.saturating_add(delta);
        if self.received > self.limit {
            return Err(ConnError::FlowControl { received: self.received, limit: self.limit });
        }
        Ok(())
    }

    /// Records that the application consumed `delta` more stream bytes across the
    /// connection (RFC 9000 §4.1). Feeds the re-advertisement in
    /// [`Self::window_update`].
    pub fn record_read(&mut self, delta: u64) {
        self.read = self.read.saturating_add(delta);
    }

    /// Re-advertises the connection limit as `read + window` and returns the new
    /// limit (RFC 9000 §4.1). The limit only ever grows. The caller sends a
    /// `MAX_DATA` frame with the returned value.
    pub fn window_update(&mut self, window: u64) -> u64 {
        let candidate = self.read.saturating_add(window);
        self.limit = self.limit.max(candidate);
        self.limit
    }
}

// ── Send-side stream-count limit (RFC 9000 §4.6) ────────────────────────────

/// How many streams of one direction the peer lets *us* open, and the IDs of the
/// streams we open under it (RFC 9000 §4.6). Assumes the local endpoint is the
/// client, so the streams it hands out are client-initiated (stream-id bit 0
/// clear, RFC 9000 §2.1).
#[derive(Clone, Debug)]
pub struct SendStreamLimit {
    /// Which stream-count axis this tracks (RFC 9000 §4.6).
    dir: StreamDir,
    /// How many streams of this direction we have opened so far.
    opened: u64,
    /// The peer's advertised limit (their `initial_max_streams_*` transport
    /// parameter, raised by `MAX_STREAMS` frames, RFC 9000 §18.2, §19.11).
    max_streams: u64,
}

impl SendStreamLimit {
    /// Creates the send-side count bounded by `initial_max_streams` (the peer's
    /// `initial_max_streams_bidi` / `_uni` transport parameter, RFC 9000 §18.2).
    #[must_use]
    pub fn new(dir: StreamDir, initial_max_streams: u64) -> Self {
        Self { dir, opened: 0, max_streams: initial_max_streams }
    }

    /// The stream-count axis this tracks (RFC 9000 §4.6).
    #[must_use]
    pub fn dir(&self) -> StreamDir {
        self.dir
    }

    /// How many streams of this direction we have opened so far.
    #[must_use]
    pub fn opened(&self) -> u64 {
        self.opened
    }

    /// The peer's current limit on streams of this direction (RFC 9000 §4.6).
    #[must_use]
    pub fn max_streams(&self) -> u64 {
        self.max_streams
    }

    /// Whether another stream of this direction may be opened right now
    /// (RFC 9000 §4.6).
    #[must_use]
    pub fn can_open(&self) -> bool {
        self.opened < self.max_streams
    }

    /// Opens the next client-initiated stream of this direction and returns its
    /// stream ID, or `None` if the peer's limit is reached (RFC 9000 §4.6). The
    /// nth (0-based) client stream of a direction has ID `n * 4 + dir_bit`
    /// (RFC 9000 §2.1): bidirectional streams are `0, 4, 8, …`, unidirectional
    /// streams are `2, 6, 10, …`.
    pub fn open(&mut self) -> Option<u64> {
        if !self.can_open() {
            return None;
        }
        let dir_bit = match self.dir {
            StreamDir::Bidi => 0,
            StreamDir::Uni => 2,
        };
        let stream_id = self.opened * 4 + dir_bit;
        self.opened += 1;
        Some(stream_id)
    }

    /// Raises the peer's limit from a received `MAX_STREAMS` frame
    /// (RFC 9000 §19.11). The limit only ever grows.
    pub fn update_max_streams(&mut self, new_max: u64) {
        self.max_streams = self.max_streams.max(new_max);
    }

    /// Whether we want to open a stream but the peer's limit forbids it — the
    /// trigger, when there is a stream to open, for a `STREAMS_BLOCKED` frame
    /// (RFC 9000 §19.14).
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        !self.can_open()
    }

    /// The stream-count limit at which we are blocked, if blocked
    /// (RFC 9000 §19.14).
    #[must_use]
    pub fn blocked_at(&self) -> Option<u64> {
        self.is_blocked().then_some(self.max_streams)
    }
}

// ── Receive-side stream-count limit (RFC 9000 §4.6) ─────────────────────────

/// How many streams of one direction we let the peer open, the enforcement of
/// that limit, and its re-advertisement as streams complete (RFC 9000 §4.6).
#[derive(Clone, Debug)]
pub struct RecvStreamLimit {
    /// Which stream-count axis this tracks (RFC 9000 §4.6).
    dir: StreamDir,
    /// The highest stream count the peer has opened of this direction.
    opened: u64,
    /// How many streams of this direction have finished (both halves closed),
    /// the basis for re-advertising the limit.
    closed: u64,
    /// The limit we have advertised (`MAX_STREAMS`).
    max_streams: u64,
}

impl RecvStreamLimit {
    /// Creates the receive-side count advertising `initial_max_streams` (our
    /// `initial_max_streams_bidi` / `_uni` transport parameter, RFC 9000 §18.2).
    #[must_use]
    pub fn new(dir: StreamDir, initial_max_streams: u64) -> Self {
        Self { dir, opened: 0, closed: 0, max_streams: initial_max_streams }
    }

    /// The stream-count axis this tracks (RFC 9000 §4.6).
    #[must_use]
    pub fn dir(&self) -> StreamDir {
        self.dir
    }

    /// The highest stream count of this direction the peer has opened.
    #[must_use]
    pub fn opened(&self) -> u64 {
        self.opened
    }

    /// The number of streams of this direction that have finished.
    #[must_use]
    pub fn closed(&self) -> u64 {
        self.closed
    }

    /// The limit currently advertised to the peer (RFC 9000 §4.6).
    #[must_use]
    pub fn max_streams(&self) -> u64 {
        self.max_streams
    }

    /// Records that the peer opened a stream whose cumulative count is `count`
    /// (from [`stream_count`] on the stream ID). Returns [`ConnError::StreamLimit`]
    /// if that exceeds the advertised limit (RFC 9000 §4.6). Opening an
    /// already-seen or lower-numbered stream is a no-op (the highest count wins).
    pub fn record_open(&mut self, count: u64) -> Result<(), ConnError> {
        if count > self.max_streams {
            return Err(ConnError::StreamLimit { count, limit: self.max_streams, dir: self.dir });
        }
        self.opened = self.opened.max(count);
        Ok(())
    }

    /// Records that `delta` more streams of this direction have finished
    /// (RFC 9000 §4.6). Feeds the re-advertisement in [`Self::window_update`].
    pub fn record_closed(&mut self, delta: u64) {
        self.closed = self.closed.saturating_add(delta);
    }

    /// Re-advertises the limit as `closed + concurrency` and returns the new
    /// limit (RFC 9000 §4.6). The limit only ever grows. The caller sends a
    /// `MAX_STREAMS` frame with the returned value. `concurrency` is the number
    /// of concurrent streams of this direction the peer may keep open.
    pub fn window_update(&mut self, concurrency: u64) -> u64 {
        let candidate = self.closed.saturating_add(concurrency);
        self.max_streams = self.max_streams.max(candidate);
        self.max_streams
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Directionality + counting (RFC 9000 §2.1, §4.6) ─────────────────────

    #[test]
    fn stream_dir_from_id() {
        // 0x0/0x1 bidi, 0x2/0x3 uni.
        assert_eq!(StreamDir::of(0), StreamDir::Bidi);
        assert_eq!(StreamDir::of(1), StreamDir::Bidi);
        assert_eq!(StreamDir::of(2), StreamDir::Uni);
        assert_eq!(StreamDir::of(3), StreamDir::Uni);
        assert_eq!(StreamDir::of(4), StreamDir::Bidi);
    }

    #[test]
    fn stream_count_from_id() {
        // The nth stream of a type (id = n*4 + bits) has count n+1.
        assert_eq!(stream_count(0), 1);
        assert_eq!(stream_count(3), 1);
        assert_eq!(stream_count(4), 2);
        assert_eq!(stream_count(7), 2);
        assert_eq!(stream_count(40), 11);
    }

    // ── Send-side connection flow control (RFC 9000 §4.1) ───────────────────

    #[test]
    fn send_conn_flow_tracks_available() {
        let mut f = SendConnFlow::new(100);
        assert_eq!(f.available(), 100);
        f.on_sent(30);
        assert_eq!(f.sent(), 30);
        assert_eq!(f.available(), 70);
        assert!(!f.is_blocked());
    }

    #[test]
    fn send_conn_flow_allowed_caps_at_available() {
        let mut f = SendConnFlow::new(10);
        assert_eq!(f.allowed(4), 4);
        assert_eq!(f.allowed(100), 10);
        f.on_sent(10);
        assert_eq!(f.allowed(5), 0);
    }

    #[test]
    fn send_conn_flow_blocked_at_limit() {
        let mut f = SendConnFlow::new(8);
        f.on_sent(8);
        assert!(f.is_blocked());
        assert_eq!(f.blocked_at(), Some(8));
        // Peer raises MAX_DATA → unblocked.
        f.update_max_data(16);
        assert!(!f.is_blocked());
        assert_eq!(f.available(), 8);
        assert_eq!(f.blocked_at(), None);
    }

    #[test]
    fn send_conn_flow_max_data_never_shrinks() {
        let mut f = SendConnFlow::new(100);
        f.update_max_data(50);
        assert_eq!(f.max_data(), 100);
    }

    // ── Receive-side connection flow control (RFC 9000 §4.1) ────────────────

    #[test]
    fn recv_conn_flow_within_limit() {
        let mut f = RecvConnFlow::new(100);
        f.record_received(40).unwrap();
        f.record_received(30).unwrap();
        assert_eq!(f.received(), 70);
    }

    #[test]
    fn recv_conn_flow_violation() {
        let mut f = RecvConnFlow::new(50);
        f.record_received(40).unwrap();
        let err = f.record_received(20).unwrap_err();
        assert_eq!(err, ConnError::FlowControl { received: 60, limit: 50 });
        assert_eq!(err.code(), FLOW_CONTROL_ERROR);
    }

    #[test]
    fn recv_conn_flow_at_exact_limit_ok() {
        let mut f = RecvConnFlow::new(50);
        f.record_received(50).unwrap();
        assert_eq!(f.received(), 50);
    }

    #[test]
    fn recv_conn_flow_window_update_grows() {
        let mut f = RecvConnFlow::new(50);
        f.record_received(50).unwrap();
        f.record_read(50);
        // Re-advertise a 50-byte window from the consumed total → limit 100.
        assert_eq!(f.window_update(50), 100);
        assert_eq!(f.limit(), 100);
        f.record_received(50).unwrap();
    }

    #[test]
    fn recv_conn_flow_window_update_never_shrinks() {
        let mut f = RecvConnFlow::new(100);
        // read is 0, window 10 → candidate 10 < current 100.
        assert_eq!(f.window_update(10), 100);
    }

    // ── Send-side stream-count limit (RFC 9000 §4.6) ────────────────────────

    #[test]
    fn send_stream_limit_bidi_ids() {
        let mut l = SendStreamLimit::new(StreamDir::Bidi, 3);
        assert_eq!(l.open(), Some(0));
        assert_eq!(l.open(), Some(4));
        assert_eq!(l.open(), Some(8));
        // Fourth open exceeds the limit of 3.
        assert_eq!(l.open(), None);
        assert!(l.is_blocked());
        assert_eq!(l.blocked_at(), Some(3));
        assert_eq!(l.opened(), 3);
    }

    #[test]
    fn send_stream_limit_uni_ids() {
        let mut l = SendStreamLimit::new(StreamDir::Uni, 2);
        assert_eq!(l.open(), Some(2));
        assert_eq!(l.open(), Some(6));
        assert_eq!(l.open(), None);
    }

    #[test]
    fn send_stream_limit_raise_unblocks() {
        let mut l = SendStreamLimit::new(StreamDir::Bidi, 1);
        assert_eq!(l.open(), Some(0));
        assert!(l.is_blocked());
        l.update_max_streams(2);
        assert!(l.can_open());
        assert_eq!(l.open(), Some(4));
    }

    #[test]
    fn send_stream_limit_never_shrinks() {
        let mut l = SendStreamLimit::new(StreamDir::Uni, 5);
        l.update_max_streams(2);
        assert_eq!(l.max_streams(), 5);
    }

    #[test]
    fn send_stream_limit_zero_blocks_immediately() {
        let mut l = SendStreamLimit::new(StreamDir::Bidi, 0);
        assert!(!l.can_open());
        assert_eq!(l.open(), None);
        assert_eq!(l.blocked_at(), Some(0));
    }

    // ── Receive-side stream-count limit (RFC 9000 §4.6) ─────────────────────

    #[test]
    fn recv_stream_limit_within() {
        let mut l = RecvStreamLimit::new(StreamDir::Bidi, 3);
        // Peer opens streams 1, 5, 9 (server bidi), counts 1, 2, 3.
        l.record_open(stream_count(1)).unwrap();
        l.record_open(stream_count(5)).unwrap();
        l.record_open(stream_count(9)).unwrap();
        assert_eq!(l.opened(), 3);
    }

    #[test]
    fn recv_stream_limit_violation() {
        let mut l = RecvStreamLimit::new(StreamDir::Uni, 2);
        // Third uni stream (count 3) exceeds the limit of 2.
        let err = l.record_open(3).unwrap_err();
        assert_eq!(err, ConnError::StreamLimit { count: 3, limit: 2, dir: StreamDir::Uni });
        assert_eq!(err.code(), STREAM_LIMIT_ERROR);
    }

    #[test]
    fn recv_stream_limit_out_of_order_keeps_highest() {
        let mut l = RecvStreamLimit::new(StreamDir::Bidi, 5);
        l.record_open(4).unwrap();
        // A lower count arriving late does not lower `opened`.
        l.record_open(2).unwrap();
        assert_eq!(l.opened(), 4);
    }

    #[test]
    fn recv_stream_limit_window_update_grows() {
        let mut l = RecvStreamLimit::new(StreamDir::Bidi, 3);
        l.record_open(3).unwrap();
        // Two streams finished; re-advertise concurrency 3 from the closed base.
        l.record_closed(2);
        assert_eq!(l.window_update(3), 5);
        assert_eq!(l.max_streams(), 5);
        l.record_open(5).unwrap();
    }

    #[test]
    fn recv_stream_limit_window_update_never_shrinks() {
        let mut l = RecvStreamLimit::new(StreamDir::Uni, 10);
        // No streams closed, concurrency 3 → candidate 3 < current 10.
        assert_eq!(l.window_update(3), 10);
    }

    #[test]
    fn recv_stream_limit_at_exact_limit_ok() {
        let mut l = RecvStreamLimit::new(StreamDir::Bidi, 2);
        l.record_open(2).unwrap();
        assert_eq!(l.opened(), 2);
    }
}
