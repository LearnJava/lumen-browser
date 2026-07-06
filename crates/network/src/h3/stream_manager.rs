//! QUIC stream manager — per-stream frame dispatch (RFC 9000 §2, §3, §4, §19).
//!
//! Slice 48 ([`super::connection`]) built the connection-level receive dispatch:
//! it owns the connection-wide state machines and routes each decrypted frame to
//! the machine that owns it, but it explicitly *defers* the per-stream frames
//! (STREAM, RESET_STREAM, STOP_SENDING, MAX_STREAM_DATA, STREAM_DATA_BLOCKED) to
//! "the stream manager (a later slice)" via [`super::connection::PacketEffects::
//! deferred`]. This slice is that stream manager.
//!
//! [`StreamManager`] is the single place that owns every live stream's state — the
//! per-stream receive reassembly and send buffers ([`super::stream::RecvStream`] /
//! [`super::stream::SendStream`]), the connection-level *receive* flow-control
//! budget ([`super::conn_flow::RecvConnFlow`]), and the *receive* stream-count
//! limits ([`super::conn_flow::RecvStreamLimit`], one per direction) — and routes
//! the deferred per-stream frames to the owning half:
//!
//! - **STREAM** ([`StreamManager::recv_stream`]) delivers application data to the
//!   receiving half, lazily creating the [`super::stream::RecvStream`] on first
//!   sight with the receive window this endpoint advertises for the stream's
//!   type (the `initial_max_stream_data_*` transport parameters, RFC 9000 §18.2),
//!   enforcing the receive stream-count limit for a peer-initiated stream
//!   (RFC 9000 §4.6) and the connection-wide receive flow-control limit across all
//!   streams (RFC 9000 §4.1).
//! - **RESET_STREAM** ([`StreamManager::recv_reset`]) aborts the receiving half at
//!   the peer's final size, accounting that final size against the connection
//!   receive budget (RFC 9000 §4.5, §19.4).
//! - **STOP_SENDING** ([`StreamManager::recv_stop_sending`]) is the peer asking us
//!   to stop sending on a stream; we reset our sending half and surface the
//!   `RESET_STREAM` frame to send in response (RFC 9000 §3.5, §19.5).
//! - **MAX_STREAM_DATA** ([`StreamManager::recv_max_stream_data`]) raises our
//!   sending half's per-stream flow-control limit (RFC 9000 §19.10).
//! - **STREAM_DATA_BLOCKED** ([`StreamManager::recv_stream_data_blocked`]) is the
//!   peer signalling it is blocked on our receive limit; like the connection-level
//!   DATA_BLOCKED it carries no state to move — the window re-advertisement is
//!   driven as the application consumes data (RFC 9000 §19.13).
//!
//! A stream identifier's two low bits fix which half a frame may touch
//! (RFC 9000 §2.1): a receive-only stream (a unidirectional stream we initiated)
//! cannot carry a STREAM to us, and a send-only stream (a unidirectional stream
//! the peer initiated) cannot carry a STOP_SENDING / MAX_STREAM_DATA to our send
//! half — either is a [`StreamManagerError::StreamState`] (`STREAM_STATE_ERROR`,
//! RFC 9000 §19.5, §19.10).
//!
//! Like every slice so far this is a pure state machine — no IO, no packet
//! protection, no timers. The caller feeds it the frames [`super::connection`]
//! deferred and reads back ordered application bytes plus the `RESET_STREAM`
//! frames to transmit.
//!
//! ## Out of scope (later slices)
//!
//! - The send path proper: opening a stream, packetizing the STREAM frames a
//!   [`super::stream::SendStream`] produces, and the send-side stream-count limit
//!   ([`super::conn_flow::SendStreamLimit`], owned by [`super::connection`]). This
//!   slice creates a send stream on demand via [`StreamManager::open_send_stream`]
//!   so the deferred STOP_SENDING / MAX_STREAM_DATA frames have a target, but the
//!   policy that decides *when* to open one is the send engine's job.
//! - Mapping ACK frames onto each stream's [`super::stream::SendStream::on_ack`]
//!   (the loss layer's role) and NEW_TOKEN handling (connection-level token
//!   storage).
//! - Any IO, header protection, AEAD, or TLS.

use std::collections::BTreeMap;

use super::conn_flow::{ConnError, RecvConnFlow, RecvStreamLimit, StreamDir, stream_count};
use super::quic_frame::Frame;
use super::stream::{
    RecvState, RecvStream, SendStream, StreamError, is_bidirectional, is_client_initiated,
    is_server_initiated,
};

/// `STREAM_STATE_ERROR` — a frame referenced a stream in a state it is not
/// permitted for, e.g. a STREAM on a receive-only stream or a STOP_SENDING on a
/// send-only stream (RFC 9000 §20.1, §19.5, §19.10).
pub const STREAM_STATE_ERROR: u64 = 0x05;

/// A stream-manager protocol violation surfaced while dispatching a per-stream
/// frame. Each variant maps to a single RFC 9000 §20.1 connection-error code via
/// [`StreamManagerError::code`], preserving the owning machine's error for
/// diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamManagerError {
    /// A per-stream invariant was violated (flow control or final size, from
    /// [`super::stream`]).
    Stream(StreamError),
    /// A connection-wide limit was violated (receive flow control or the receive
    /// stream count, from [`super::conn_flow`]).
    Conn(ConnError),
    /// A frame referenced a stream half it may not touch given the stream's
    /// directionality (RFC 9000 §2.1). Maps to [`STREAM_STATE_ERROR`].
    StreamState {
        /// The offending stream ID.
        stream_id: u64,
    },
}

impl StreamManagerError {
    /// The RFC 9000 §20.1 connection-error code this violation maps to.
    #[must_use]
    pub const fn code(&self) -> u64 {
        match self {
            Self::Stream(e) => e.code(),
            Self::Conn(e) => e.code(),
            Self::StreamState { .. } => STREAM_STATE_ERROR,
        }
    }
}

impl core::fmt::Display for StreamManagerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Stream(e) => write!(f, "{e}"),
            Self::Conn(e) => write!(f, "{e}"),
            Self::StreamState { stream_id } => {
                write!(f, "QUIC stream {stream_id}: frame not permitted in stream state")
            }
        }
    }
}

impl std::error::Error for StreamManagerError {}

impl From<StreamError> for StreamManagerError {
    fn from(e: StreamError) -> Self {
        Self::Stream(e)
    }
}

impl From<ConnError> for StreamManagerError {
    fn from(e: ConnError) -> Self {
        Self::Conn(e)
    }
}

/// The receive-window and stream-count transport parameters a [`StreamManager`]
/// needs to admit peer streams (RFC 9000 §18.2). These are *our* advertised
/// values — the limits the peer must respect when sending to us.
#[derive(Clone, Copy, Debug)]
pub struct StreamManagerConfig {
    /// Our `initial_max_stream_data_bidi_local`: the receive window for the
    /// response half of a bidirectional stream *we* initiated (RFC 9000 §18.2).
    pub initial_max_stream_data_bidi_local: u64,
    /// Our `initial_max_stream_data_bidi_remote`: the receive window for a
    /// bidirectional stream the *peer* initiated (RFC 9000 §18.2).
    pub initial_max_stream_data_bidi_remote: u64,
    /// Our `initial_max_stream_data_uni`: the receive window for a unidirectional
    /// stream the peer initiated (RFC 9000 §18.2).
    pub initial_max_stream_data_uni: u64,
    /// Our `initial_max_data`: the connection-wide receive flow-control budget
    /// shared across all streams (RFC 9000 §18.2, §4.1).
    pub initial_max_data: u64,
    /// Our `initial_max_streams_bidi`: how many bidirectional streams the peer
    /// may open (RFC 9000 §18.2, §4.6).
    pub initial_max_streams_bidi: u64,
    /// Our `initial_max_streams_uni`: how many unidirectional streams the peer
    /// may open (RFC 9000 §18.2, §4.6).
    pub initial_max_streams_uni: u64,
}

/// The connection-wide owner of every live stream's state, routing the per-stream
/// frames [`super::connection`] defers (RFC 9000 §2, §3, §4, §19).
///
/// Holds the per-stream receive and send halves, the connection-level receive
/// flow-control budget, and the two receive stream-count limits. Frames are fed
/// through the `recv_*` methods; ordered application bytes are read back with
/// [`StreamManager::read`].
#[derive(Debug)]
pub struct StreamManager {
    /// Our advertised receive windows and stream-count limits.
    config: StreamManagerConfig,
    /// The receiving half of each stream we accept data on, keyed by stream ID.
    recv: BTreeMap<u64, RecvStream>,
    /// The sending half of each stream we transmit on, keyed by stream ID.
    send: BTreeMap<u64, SendStream>,
    /// The highest byte offset of each receive stream already counted against the
    /// connection receive budget, so a retransmit is not double-counted.
    recv_counted: BTreeMap<u64, u64>,
    /// The connection-wide receive flow-control budget (RFC 9000 §4.1).
    recv_flow: RecvConnFlow,
    /// The receive-side bidirectional stream-count limit (RFC 9000 §4.6).
    recv_bidi_limit: RecvStreamLimit,
    /// The receive-side unidirectional stream-count limit (RFC 9000 §4.6).
    recv_uni_limit: RecvStreamLimit,
}

impl StreamManager {
    /// Builds a stream manager advertising the receive windows and stream-count
    /// limits in `config` (RFC 9000 §18.2).
    #[must_use]
    pub fn new(config: StreamManagerConfig) -> Self {
        Self {
            recv: BTreeMap::new(),
            send: BTreeMap::new(),
            recv_counted: BTreeMap::new(),
            recv_flow: RecvConnFlow::new(config.initial_max_data),
            recv_bidi_limit: RecvStreamLimit::new(StreamDir::Bidi, config.initial_max_streams_bidi),
            recv_uni_limit: RecvStreamLimit::new(StreamDir::Uni, config.initial_max_streams_uni),
            config,
        }
    }

    /// Registers a sending half for `stream_id` bounded by `peer_initial_max_data`
    /// (the peer's `initial_max_stream_data_*` transport parameter for this stream
    /// type, RFC 9000 §18.2), so the deferred STOP_SENDING / MAX_STREAM_DATA frames
    /// have a target. Returns a mutable reference to the send stream (created if
    /// absent). The send-path policy that decides when to open a stream is a later
    /// slice; this only wires the state into place.
    pub fn open_send_stream(&mut self, stream_id: u64, peer_initial_max_data: u64) -> &mut SendStream {
        self.send
            .entry(stream_id)
            .or_insert_with(|| SendStream::new(peer_initial_max_data))
    }

    /// The receiving half of `stream_id`, if one exists.
    #[must_use]
    pub fn recv_stream_ref(&self, stream_id: u64) -> Option<&RecvStream> {
        self.recv.get(&stream_id)
    }

    /// The sending half of `stream_id`, if one exists.
    #[must_use]
    pub fn send_stream(&self, stream_id: u64) -> Option<&SendStream> {
        self.send.get(&stream_id)
    }

    /// The sending half of `stream_id`, mutably, if one exists.
    pub fn send_stream_mut(&mut self, stream_id: u64) -> Option<&mut SendStream> {
        self.send.get_mut(&stream_id)
    }

    /// The identifiers of every stream with a sending half, ascending. The send
    /// path iterates these to drain each [`SendStream`] holding buffered data into
    /// STREAM frames (RFC 9000 §19.8); a stream whose send half is quiescent (fully
    /// transmitted, blocked, or reset) simply yields nothing when polled.
    #[must_use]
    pub fn send_stream_ids(&self) -> Vec<u64> {
        self.send.keys().copied().collect()
    }

    /// The connection-wide receive flow-control budget (RFC 9000 §4.1).
    #[must_use]
    pub fn recv_flow(&self) -> &RecvConnFlow {
        &self.recv_flow
    }

    /// The receive-side stream-count limit for `dir` (RFC 9000 §4.6).
    #[must_use]
    pub fn recv_stream_limit(&self, dir: StreamDir) -> &RecvStreamLimit {
        match dir {
            StreamDir::Bidi => &self.recv_bidi_limit,
            StreamDir::Uni => &self.recv_uni_limit,
        }
    }

    /// Dispatches a received STREAM frame (RFC 9000 §19.8), delivering `data` at
    /// `offset` (with `fin` marking the final frame) to `stream_id`'s receiving
    /// half. Lazily creates the receiving half on first sight, enforcing the
    /// receive stream-count limit for a peer-initiated stream (RFC 9000 §4.6), and
    /// accounts the newly-received bytes against the connection receive budget
    /// (RFC 9000 §4.1).
    ///
    /// # Errors
    ///
    /// [`StreamManagerError::StreamState`] if `stream_id` is receive-forbidden (a
    /// unidirectional stream we initiated); [`StreamManagerError::Stream`] on a
    /// per-stream flow-control or final-size violation;
    /// [`StreamManagerError::Conn`] on a connection receive flow-control or
    /// stream-count violation.
    pub fn recv_stream(
        &mut self,
        stream_id: u64,
        offset: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<(), StreamManagerError> {
        let end = offset.saturating_add(data.len() as u64);
        self.ensure_recv_stream(stream_id)?;
        // Account the frame against the connection receive budget before the
        // per-stream delivery: the aggregate limit is checked on the highest
        // offset ever reached, so a retransmit that does not advance it costs
        // nothing (RFC 9000 §4.1).
        self.count_connection_receive(stream_id, end)?;
        let recv = self
            .recv
            .get_mut(&stream_id)
            .expect("ensure_recv_stream created it");
        recv.recv(offset, data, fin)?;
        Ok(())
    }

    /// Dispatches a received RESET_STREAM frame (RFC 9000 §19.4): the peer aborts
    /// the sending half of `stream_id` at `final_size` with `app_error_code`.
    /// Lazily creates the receiving half, enforces the receive stream-count limit,
    /// and accounts `final_size` against the connection receive budget
    /// (RFC 9000 §4.5).
    ///
    /// # Errors
    ///
    /// As [`StreamManager::recv_stream`], plus [`StreamManagerError::Stream`] with
    /// [`StreamError::FinalSize`] if the reset's final size contradicts data
    /// already received.
    pub fn recv_reset(
        &mut self,
        stream_id: u64,
        final_size: u64,
        app_error_code: u64,
    ) -> Result<(), StreamManagerError> {
        self.ensure_recv_stream(stream_id)?;
        self.count_connection_receive(stream_id, final_size)?;
        let recv = self
            .recv
            .get_mut(&stream_id)
            .expect("ensure_recv_stream created it");
        recv.recv_reset(final_size, app_error_code)?;
        Ok(())
    }

    /// Dispatches a received STOP_SENDING frame (RFC 9000 §19.5): the peer requests
    /// we stop sending on `stream_id`. Resets our sending half with
    /// `app_error_code` and returns the `RESET_STREAM` frame to transmit in
    /// response (RFC 9000 §3.5). Returns `None` if we have no sending half for the
    /// stream (already closed or never opened) — there is nothing to reset.
    ///
    /// # Errors
    ///
    /// [`StreamManagerError::StreamState`] if `stream_id` is a stream we cannot
    /// send on (a unidirectional stream the peer initiated).
    pub fn recv_stop_sending(
        &mut self,
        stream_id: u64,
        app_error_code: u64,
    ) -> Result<Option<Frame>, StreamManagerError> {
        if !self.can_send_on(stream_id) {
            return Err(StreamManagerError::StreamState { stream_id });
        }
        let Some(send) = self.send.get_mut(&stream_id) else {
            return Ok(None);
        };
        send.reset(app_error_code);
        Ok(Some(Frame::ResetStream {
            stream_id,
            app_error_code,
            final_size: send.write_offset(),
        }))
    }

    /// Dispatches a received MAX_STREAM_DATA frame (RFC 9000 §19.10): raises the
    /// per-stream flow-control limit of our sending half for `stream_id` to `max`.
    /// A frame for a stream we have no sending half for is ignored (the limit will
    /// seed the stream when it opens).
    ///
    /// # Errors
    ///
    /// [`StreamManagerError::StreamState`] if `stream_id` is a stream we cannot
    /// send on (a unidirectional stream the peer initiated).
    pub fn recv_max_stream_data(
        &mut self,
        stream_id: u64,
        max: u64,
    ) -> Result<(), StreamManagerError> {
        if !self.can_send_on(stream_id) {
            return Err(StreamManagerError::StreamState { stream_id });
        }
        if let Some(send) = self.send.get_mut(&stream_id) {
            send.update_max_data(max);
        }
        Ok(())
    }

    /// Dispatches a received STREAM_DATA_BLOCKED frame (RFC 9000 §19.13): the peer
    /// signals it is blocked on our receive limit for `stream_id`. Like the
    /// connection-level DATA_BLOCKED it carries no state to move here — the receive
    /// window re-advertisement (a MAX_STREAM_DATA frame) is driven as the
    /// application consumes data via [`StreamManager::read`].
    ///
    /// # Errors
    ///
    /// [`StreamManagerError::StreamState`] if `stream_id` is a stream we cannot
    /// receive on (a unidirectional stream we initiated).
    pub fn recv_stream_data_blocked(&mut self, stream_id: u64) -> Result<(), StreamManagerError> {
        if !self.can_receive_on(stream_id) {
            return Err(StreamManagerError::StreamState { stream_id });
        }
        Ok(())
    }

    /// Pops and returns the contiguous readable prefix of `stream_id`, advancing
    /// the read cursor and accounting the consumed bytes against the connection
    /// receive budget so it can be re-advertised (RFC 9000 §4.1). Returns an empty
    /// vector when no contiguous data is available or the stream does not exist.
    pub fn read(&mut self, stream_id: u64) -> Vec<u8> {
        let Some(recv) = self.recv.get_mut(&stream_id) else {
            return Vec::new();
        };
        let chunk = recv.read();
        if !chunk.is_empty() {
            self.recv_flow.record_read(chunk.len() as u64);
        }
        chunk
    }

    /// Re-advertises the connection-wide receive limit as `read + window` and
    /// returns the new limit (RFC 9000 §4.1). The caller sends a MAX_DATA frame
    /// with the returned value.
    pub fn connection_window_update(&mut self, window: u64) -> u64 {
        self.recv_flow.window_update(window)
    }

    /// Ensures a receiving half exists for `stream_id`, creating it with the
    /// receive window this endpoint advertises for the stream's type and enforcing
    /// the receive stream-count limit for a peer-initiated stream (RFC 9000 §4.6).
    fn ensure_recv_stream(&mut self, stream_id: u64) -> Result<(), StreamManagerError> {
        if self.recv.contains_key(&stream_id) {
            return Ok(());
        }
        let window = self
            .recv_window_for(stream_id)
            .ok_or(StreamManagerError::StreamState { stream_id })?;
        // A stream the peer initiated counts against our receive stream-count
        // limit; a stream we initiated counts against the peer's send-side limit,
        // not ours (RFC 9000 §4.6).
        if is_server_initiated(stream_id) {
            let dir = StreamDir::of(stream_id);
            self.recv_limit_mut(dir).record_open(stream_count(stream_id))?;
        }
        self.recv.insert(stream_id, RecvStream::new(window));
        self.recv_counted.insert(stream_id, 0);
        Ok(())
    }

    /// Advances the connection receive budget by the growth of `stream_id`'s
    /// highest received offset to `end`, rejecting an aggregate past the advertised
    /// MAX_DATA (RFC 9000 §4.1). Idempotent for a non-advancing `end`.
    fn count_connection_receive(
        &mut self,
        stream_id: u64,
        end: u64,
    ) -> Result<(), StreamManagerError> {
        let counted = self.recv_counted.entry(stream_id).or_insert(0);
        if end <= *counted {
            return Ok(());
        }
        let delta = end - *counted;
        *counted = end;
        self.recv_flow.record_received(delta)?;
        Ok(())
    }

    /// The receive window this endpoint advertises for `stream_id`, or `None` if
    /// the stream is receive-forbidden for us (a unidirectional stream we
    /// initiated, RFC 9000 §2.1).
    fn recv_window_for(&self, stream_id: u64) -> Option<u64> {
        if is_bidirectional(stream_id) {
            if is_client_initiated(stream_id) {
                Some(self.config.initial_max_stream_data_bidi_local)
            } else {
                Some(self.config.initial_max_stream_data_bidi_remote)
            }
        } else if is_server_initiated(stream_id) {
            Some(self.config.initial_max_stream_data_uni)
        } else {
            // A unidirectional stream we initiated is send-only.
            None
        }
    }

    /// Whether this (client) endpoint may receive on `stream_id`: any stream but a
    /// unidirectional stream we initiated (RFC 9000 §2.1).
    fn can_receive_on(&self, stream_id: u64) -> bool {
        is_bidirectional(stream_id) || is_server_initiated(stream_id)
    }

    /// Whether this (client) endpoint may send on `stream_id`: any stream but a
    /// unidirectional stream the peer initiated (RFC 9000 §2.1).
    fn can_send_on(&self, stream_id: u64) -> bool {
        is_bidirectional(stream_id) || is_client_initiated(stream_id)
    }

    /// The receive stream-count limit for `dir`, mutably (RFC 9000 §4.6).
    fn recv_limit_mut(&mut self, dir: StreamDir) -> &mut RecvStreamLimit {
        match dir {
            StreamDir::Bidi => &mut self.recv_bidi_limit,
            StreamDir::Uni => &mut self.recv_uni_limit,
        }
    }
}

/// The number of a stream's halves that have fully closed, for the caller to feed
/// the receive stream-count re-advertisement ([`RecvStreamLimit::record_closed`]).
/// A bidirectional stream is finished only when both halves are done; a
/// unidirectional stream, when its single half is.
#[must_use]
pub fn recv_stream_finished(recv: &RecvStream) -> bool {
    matches!(recv.state(), RecvState::DataRead | RecvState::ResetRead)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::stream::{RecvState, SendState};

    fn config() -> StreamManagerConfig {
        StreamManagerConfig {
            initial_max_stream_data_bidi_local: 100,
            initial_max_stream_data_bidi_remote: 100,
            initial_max_stream_data_uni: 100,
            initial_max_data: 1_000,
            initial_max_streams_bidi: 3,
            initial_max_streams_uni: 3,
        }
    }

    fn mgr() -> StreamManager {
        StreamManager::new(config())
    }

    #[test]
    fn stream_frame_delivers_and_is_readable() {
        let mut m = mgr();
        // Client-initiated bidi stream 0 (a request we opened, receiving its
        // response).
        m.recv_stream(0, 0, b"hello", false).unwrap();
        assert_eq!(m.read(0), b"hello");
        assert_eq!(m.recv_flow().received(), 5);
        assert_eq!(m.recv_flow().read(), 5);
    }

    #[test]
    fn stream_frame_reassembles_out_of_order() {
        let mut m = mgr();
        m.recv_stream(0, 6, b"world", false).unwrap();
        // Gap [0,6) open — nothing readable yet.
        assert_eq!(m.read(0), b"");
        m.recv_stream(0, 0, b"hello ", false).unwrap();
        assert_eq!(m.read(0), b"hello world");
    }

    #[test]
    fn retransmit_is_not_double_counted_in_connection_budget() {
        let mut m = mgr();
        m.recv_stream(0, 0, b"abcdef", false).unwrap();
        assert_eq!(m.recv_flow().received(), 6);
        // A pure retransmit of the same bytes does not advance the highest offset.
        m.recv_stream(0, 0, b"abcdef", false).unwrap();
        assert_eq!(m.recv_flow().received(), 6);
        // A partially-overlapping frame advances only by its new tail.
        m.recv_stream(0, 4, b"efgh", false).unwrap();
        assert_eq!(m.recv_flow().received(), 8);
    }

    #[test]
    fn connection_flow_control_violation_across_streams() {
        let mut cfg = config();
        cfg.initial_max_data = 10;
        cfg.initial_max_stream_data_bidi_local = 100;
        let mut m = StreamManager::new(cfg);
        m.recv_stream(0, 0, b"aaaaaa", false).unwrap(); // 6 bytes
        // Stream 4's 6 bytes push the connection total to 12 > 10.
        let err = m.recv_stream(4, 0, b"bbbbbb", false).unwrap_err();
        assert_eq!(err.code(), crate::h3::stream::FLOW_CONTROL_ERROR);
        assert!(matches!(err, StreamManagerError::Conn(_)));
    }

    #[test]
    fn per_stream_flow_control_violation() {
        let mut cfg = config();
        cfg.initial_max_stream_data_bidi_local = 4;
        let mut m = StreamManager::new(cfg);
        let err = m.recv_stream(0, 0, b"abcde", false).unwrap_err();
        assert!(matches!(err, StreamManagerError::Stream(StreamError::FlowControl { .. })));
    }

    #[test]
    fn receive_stream_count_limit_enforced_for_peer_streams() {
        let mut cfg = config();
        cfg.initial_max_streams_uni = 2;
        let mut m = StreamManager::new(cfg);
        // Server uni streams 3, 7 (counts 1, 2) are within the limit.
        m.recv_stream(3, 0, b"a", false).unwrap();
        m.recv_stream(7, 0, b"b", false).unwrap();
        // Server uni stream 11 (count 3) exceeds the advertised limit of 2.
        let err = m.recv_stream(11, 0, b"c", false).unwrap_err();
        assert_eq!(err.code(), crate::h3::conn_flow::STREAM_LIMIT_ERROR);
        assert!(matches!(err, StreamManagerError::Conn(ConnError::StreamLimit { .. })));
    }

    #[test]
    fn client_uni_stream_cannot_receive() {
        let mut m = mgr();
        // Stream 2 is a client-initiated unidirectional stream — send-only.
        let err = m.recv_stream(2, 0, b"x", false).unwrap_err();
        assert!(matches!(err, StreamManagerError::StreamState { stream_id: 2 }));
        assert_eq!(err.code(), STREAM_STATE_ERROR);
    }

    #[test]
    fn reset_stream_aborts_receive_half() {
        let mut m = mgr();
        m.recv_stream(0, 0, b"partial", false).unwrap();
        m.recv_reset(0, 7, 0x101).unwrap();
        let s = m.recv_stream_ref(0).unwrap();
        assert_eq!(s.state(), RecvState::ResetRecvd);
        assert_eq!(s.reset_error(), Some(0x101));
        // The final size counts against the connection budget.
        assert_eq!(m.recv_flow().received(), 7);
    }

    #[test]
    fn reset_final_size_below_received_is_an_error() {
        let mut m = mgr();
        m.recv_stream(0, 0, b"abcdef", false).unwrap();
        let err = m.recv_reset(0, 3, 0).unwrap_err();
        assert!(matches!(err, StreamManagerError::Stream(StreamError::FinalSize { .. })));
    }

    #[test]
    fn stop_sending_resets_send_half_and_emits_reset() {
        let mut m = mgr();
        let send = m.open_send_stream(0, 1_000);
        send.write(b"partial body");
        // Emit four bytes so the send offset advances; the reset's final size is
        // the largest offset actually sent, not the buffered-but-unsent total
        // (which the reset discards, RFC 9000 §4.5).
        m.send_stream_mut(0).unwrap().poll_transmit(4);
        let frame = m.recv_stop_sending(0, 0x99).unwrap().expect("reset frame");
        match frame {
            Frame::ResetStream { stream_id, app_error_code, final_size } => {
                assert_eq!(stream_id, 0);
                assert_eq!(app_error_code, 0x99);
                assert_eq!(final_size, 4);
            }
            other => panic!("expected RESET_STREAM, got {other:?}"),
        }
        assert_eq!(m.send_stream(0).unwrap().state(), SendState::ResetSent);
    }

    #[test]
    fn stop_sending_unknown_stream_is_noop() {
        let mut m = mgr();
        // No send half for stream 0 → nothing to reset.
        assert!(m.recv_stop_sending(0, 0).unwrap().is_none());
    }

    #[test]
    fn stop_sending_on_server_uni_is_stream_state_error() {
        let mut m = mgr();
        // Stream 3 is server-initiated uni — we cannot send on it.
        let err = m.recv_stop_sending(3, 0).unwrap_err();
        assert!(matches!(err, StreamManagerError::StreamState { stream_id: 3 }));
    }

    #[test]
    fn max_stream_data_raises_send_limit() {
        let mut m = mgr();
        m.open_send_stream(0, 4);
        m.open_send_stream(0, 4).write(b"abcdefgh");
        // Only 4 bytes may be sent under the initial limit.
        let chunk = m.send_stream_mut(0).unwrap().poll_transmit(100).unwrap();
        assert_eq!(chunk.data, b"abcd");
        assert!(m.send_stream(0).unwrap().is_blocked());
        // MAX_STREAM_DATA raises the limit → the rest flows.
        m.recv_max_stream_data(0, 8).unwrap();
        let chunk = m.send_stream_mut(0).unwrap().poll_transmit(100).unwrap();
        assert_eq!(chunk.data, b"efgh");
    }

    #[test]
    fn max_stream_data_unknown_stream_is_ignored() {
        let mut m = mgr();
        // No send half yet — the frame is a no-op, not an error.
        m.recv_max_stream_data(0, 500).unwrap();
    }

    #[test]
    fn max_stream_data_on_server_uni_is_stream_state_error() {
        let mut m = mgr();
        let err = m.recv_max_stream_data(3, 100).unwrap_err();
        assert!(matches!(err, StreamManagerError::StreamState { stream_id: 3 }));
    }

    #[test]
    fn stream_data_blocked_is_accepted_for_receivable_stream() {
        let mut m = mgr();
        // A bidi stream we can receive on — accepted, no state to move.
        m.recv_stream_data_blocked(0).unwrap();
        // A client uni stream is send-only — a blocked signal there is nonsensical.
        let err = m.recv_stream_data_blocked(2).unwrap_err();
        assert!(matches!(err, StreamManagerError::StreamState { stream_id: 2 }));
    }

    #[test]
    fn read_feeds_connection_window_readvertisement() {
        let mut cfg = config();
        cfg.initial_max_data = 6;
        let mut m = StreamManager::new(cfg);
        m.recv_stream(0, 0, b"abcdef", false).unwrap();
        assert_eq!(m.read(0), b"abcdef");
        assert_eq!(m.recv_flow().read(), 6);
        // Re-advertise a 6-byte window from the consumed total → limit 12.
        assert_eq!(m.connection_window_update(6), 12);
        m.recv_stream(0, 6, b"ghijkl", false).unwrap();
        assert_eq!(m.read(0), b"ghijkl");
    }

    #[test]
    fn recv_stream_finished_tracks_completion() {
        let mut m = mgr();
        m.recv_stream(0, 0, b"hi", true).unwrap();
        assert!(!recv_stream_finished(m.recv_stream_ref(0).unwrap()));
        assert_eq!(m.read(0), b"hi");
        assert!(recv_stream_finished(m.recv_stream_ref(0).unwrap()));
    }

    #[test]
    fn server_bidi_stream_uses_remote_window_and_counts() {
        let mut cfg = config();
        cfg.initial_max_stream_data_bidi_remote = 50;
        cfg.initial_max_streams_bidi = 1;
        let mut m = StreamManager::new(cfg);
        // Server-initiated bidi stream 1 (count 1) is within the bidi limit.
        m.recv_stream(1, 0, b"x", false).unwrap();
        assert_eq!(m.recv_stream_ref(1).unwrap().max_data(), 50);
        // Server-initiated bidi stream 5 (count 2) exceeds the limit of 1.
        let err = m.recv_stream(5, 0, b"y", false).unwrap_err();
        assert!(matches!(err, StreamManagerError::Conn(ConnError::StreamLimit { dir: StreamDir::Bidi, .. })));
    }
}
