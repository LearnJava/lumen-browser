//! HTTP/3 request pump — translating request streams to and from QUIC per-stream
//! frames (RFC 9000 §19.4, §19.5, §19.8, §19.10, §19.13; RFC 9114 §4.1, §6.1).
//!
//! [`request_dispatch`](super::request_dispatch) joined the HTTP/3 request tower
//! and the QUIC stream tower into a [`RequestDispatch`] that speaks in *stream
//! operations*: "write these request bytes to stream N and finish it" and "here are
//! the ordered bytes that arrived on stream N, does a response complete". But a
//! QUIC connection carries *frames*, not stream operations — the send loop transmits
//! [`Frame::Stream`](super::quic_frame::Frame::Stream) frames it assembles from a
//! [`SendStream`](super::stream::SendStream), and the receive loop hands back the
//! per-stream frames [`connection`](super::connection) deferred (STREAM,
//! RESET_STREAM, STOP_SENDING, MAX_STREAM_DATA, STREAM_DATA_BLOCKED). [`RequestPump`]
//! is the translation between the two: it owns a [`RequestDispatch`] and turns its
//! stream operations into outbound frames, and inbound frames into its stream
//! operations.
//!
//! ## Sending — [`RequestPump::poll_transmit`]
//!
//! [`RequestDispatch::send_request`] renders a request onto a fresh client-initiated
//! bidirectional stream, writing the whole message to its
//! [`SendStream`](super::stream::SendStream) with the send half finished (STREAM
//! FIN, RFC 9114 §4.1). [`RequestPump::poll_transmit`] drains those buffered send
//! halves: it walks every stream that has a sending half, polls each for the next
//! flow-controlled chunk ([`SendStream::poll_transmit`](super::stream::SendStream::poll_transmit)),
//! and wraps the chunk into a [`Frame::Stream`](super::quic_frame::Frame::Stream)
//! (RFC 9000 §19.8) for the send loop to packetize. A stream that is fully
//! transmitted, blocked on its flow-control window, or reset yields nothing.
//!
//! ## Receiving — [`RequestPump::on_frame`]
//!
//! The receive loop delivers each per-stream frame the connection deferred, and
//! [`RequestPump::on_frame`] routes it to the half it belongs to:
//!
//! - **STREAM** ([`Frame::Stream`](super::quic_frame::Frame::Stream)) is response
//!   data — fed to [`RequestDispatch::on_stream_frame`], yielding a completed
//!   [`H3Response`] on the frame carrying the server's FIN
//!   ([`PumpEvent::Response`]) or advancing the response otherwise
//!   ([`PumpEvent::Progress`]).
//! - **RESET_STREAM** ([`Frame::ResetStream`](super::quic_frame::Frame::ResetStream),
//!   RFC 9000 §19.4) aborts the response half: the reset's final size is committed
//!   to the connection receive budget (RFC 9000 §4.5) and the request is retired
//!   ([`PumpEvent::Aborted`]).
//! - **STOP_SENDING** ([`Frame::StopSending`](super::quic_frame::Frame::StopSending),
//!   RFC 9000 §19.5) asks us to stop sending the request: our send half is reset and
//!   the [`Frame::ResetStream`](super::quic_frame::Frame::ResetStream) to send in
//!   reply is surfaced ([`PumpEvent::StopSending`]).
//! - **MAX_STREAM_DATA** ([`Frame::MaxStreamData`](super::quic_frame::Frame::MaxStreamData),
//!   RFC 9000 §19.10) raises our send window so more of the request body may flow;
//!   **STREAM_DATA_BLOCKED**
//!   ([`Frame::StreamDataBlocked`](super::quic_frame::Frame::StreamDataBlocked),
//!   RFC 9000 §19.13) is the peer signalling it is blocked on our receive window.
//!   Both are applied and reported as [`PumpEvent::Progress`].
//!
//! A per-stream frame for a stream with no in-flight request (never opened, or
//! already completed, aborted, and retired) is [`PumpEvent::Ignored`] — no state is
//! materialised, keeping the pump's view of "known stream" aligned with the mux's
//! (the same contract [`RequestDispatch::on_stream_frame`] enforces for STREAM). A
//! frame that is not one of these five per-stream frames is [`PumpEvent::Ignored`]
//! too — the connection layer owns every other frame type.
//!
//! Like every slice below it, [`RequestPump`] is a pure state machine: no sockets,
//! no timers, no packet protection. It answers the surrounding `h3_do_request`
//! dispatch's two remaining questions — *what STREAM frames do I have to send* and
//! *this per-stream frame arrived, what does it do to my requests* — while the
//! datagram IO, header protection, and loss recovery stay in the transport layers
//! around it.

use super::h3_exchange::H3Response;
use super::quic_frame::Frame;
use super::request_dispatch::{DispatchError, RequestDispatch, SentRequest};
use super::request_exchange::ClientRequest;
use super::stream_manager::StreamManagerConfig;

/// What routing one inbound per-stream frame through the pump produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PumpEvent {
    /// A STREAM frame completed a response on its stream, which is now retired
    /// (RFC 9114 §4.1).
    Response(H3Response),
    /// The frame advanced a request without completing it: a STREAM frame that did
    /// not carry the response's FIN, or a flow-control frame (MAX_STREAM_DATA /
    /// STREAM_DATA_BLOCKED) that moved a limit but produced no response.
    Progress,
    /// A STOP_SENDING frame reset our sending half of `stream_id` (RFC 9000 §19.5);
    /// transmit `reset` (a RESET_STREAM frame) in reply. The request stays in
    /// flight — its response half is unaffected.
    StopSending {
        /// The request stream the peer asked us to stop sending on.
        stream_id: u64,
        /// The RESET_STREAM frame to send in reply (RFC 9000 §3.5).
        reset: Frame,
    },
    /// A RESET_STREAM frame aborted the response half of `stream_id` (RFC 9000
    /// §19.4): the request is retired without a response, carrying the peer's
    /// application error code.
    Aborted {
        /// The request stream the peer reset.
        stream_id: u64,
        /// The peer's application-protocol error code (RFC 9114 §8.1).
        app_error_code: u64,
    },
    /// The frame targeted a stream with no in-flight request (never opened, or
    /// already completed/aborted and retired), or was not one of the five
    /// per-stream frames the pump routes. No state changed.
    Ignored,
}

/// Translates a [`RequestDispatch`]'s stream operations to and from QUIC per-stream
/// frames (RFC 9000 §19; RFC 9114 §6.1).
///
/// Owns the dispatch and keeps it in step with the frame wire: outbound, it drains
/// the request send streams into [`Frame::Stream`](super::quic_frame::Frame::Stream)
/// frames ([`RequestPump::poll_transmit`]); inbound, it routes each deferred
/// per-stream frame to the dispatch or the stream layer
/// ([`RequestPump::on_frame`]).
#[derive(Debug)]
pub struct RequestPump {
    /// The request dispatch this pump translates for.
    dispatch: RequestDispatch,
}

impl RequestPump {
    /// Builds a pump over a fresh [`RequestDispatch`] advertising `stream_config`'s
    /// receive windows and stream-count limits (RFC 9000 §18.2) and seeding each
    /// request's send half with the peer's `initial_max_stream_data_bidi_remote`
    /// send window (`peer_initial_max_data_bidi_remote`).
    #[must_use]
    pub fn new(
        stream_config: StreamManagerConfig,
        peer_initial_max_data_bidi_remote: u64,
    ) -> Self {
        Self {
            dispatch: RequestDispatch::new(stream_config, peer_initial_max_data_bidi_remote),
        }
    }

    /// Places `req` onto a fresh client-initiated bidirectional stream and finishes
    /// its send half (STREAM FIN, RFC 9114 §4.1) — see
    /// [`RequestDispatch::send_request`]. The rendered bytes wait on the send half
    /// until [`RequestPump::poll_transmit`] drains them into STREAM frames.
    ///
    /// # Errors
    ///
    /// [`DispatchError::Open`] if the request cannot be built (RFC 9114
    /// §4.2/§7.2.1) or all client bidirectional stream identifiers are spent
    /// (RFC 9000 §2.1). No stream is consumed.
    pub fn send_request(&mut self, req: &ClientRequest) -> Result<SentRequest, DispatchError> {
        self.dispatch.send_request(req)
    }

    /// Drains every request send stream into QUIC STREAM frames (RFC 9000 §19.8),
    /// each carrying at most `max_frame_len` bytes of stream data.
    ///
    /// Walks every stream with a sending half and polls it repeatedly, emitting one
    /// [`Frame::Stream`](super::quic_frame::Frame::Stream) per chunk until the send
    /// half yields nothing — because it is fully transmitted, blocked on its
    /// flow-control window (awaiting a MAX_STREAM_DATA), or reset. The final chunk of
    /// a request carries the STREAM FIN. Returns the frames in stream-identifier then
    /// offset order, ready for the send loop to packetize.
    pub fn poll_transmit(&mut self, max_frame_len: usize) -> Vec<Frame> {
        let mut frames = Vec::new();
        // Collect the identifiers first: polling borrows the stream map mutably, so
        // it cannot run while iterating the map's keys.
        for stream_id in self.dispatch.streams().send_stream_ids() {
            while let Some(send) = self.dispatch.streams_mut().send_stream_mut(stream_id) {
                let Some(chunk) = send.poll_transmit(max_frame_len) else {
                    break;
                };
                frames.push(Frame::Stream {
                    stream_id,
                    offset: chunk.offset,
                    fin: chunk.fin,
                    data: chunk.data,
                });
            }
        }
        frames
    }

    /// Routes one inbound per-stream frame the connection deferred to us, returning
    /// the [`PumpEvent`] it produced.
    ///
    /// STREAM frames are response data ([`RequestDispatch::on_stream_frame`]);
    /// RESET_STREAM aborts a request; STOP_SENDING resets our send half and yields a
    /// RESET_STREAM to send; MAX_STREAM_DATA and STREAM_DATA_BLOCKED move a
    /// flow-control limit. A frame for a stream with no in-flight request, or any
    /// other frame type, is [`PumpEvent::Ignored`].
    ///
    /// # Errors
    ///
    /// - [`DispatchError::Stream`] if the frame breaches QUIC flow control, the
    ///   final-size invariants, or the stream-state rules (RFC 9000 §4.1, §4.5,
    ///   §19.5, §19.10).
    /// - [`DispatchError::Mux`] if a STREAM frame targets a stream with no in-flight
    ///   request or completes a malformed response (RFC 9114 §4.1).
    pub fn on_frame(&mut self, frame: &Frame) -> Result<PumpEvent, DispatchError> {
        match frame {
            // Response data: the dispatch owns the "unknown stream" and "malformed
            // response" contracts, so route straight through and let it decide.
            Frame::Stream { stream_id, offset, fin, data } => {
                match self.dispatch.on_stream_frame(*stream_id, *offset, data, *fin)? {
                    Some(response) => Ok(PumpEvent::Response(response)),
                    None => Ok(PumpEvent::Progress),
                }
            }
            // The four stream-control frames only act on an in-flight request; a
            // frame for an unknown or retired stream is ignored without touching the
            // stream layer, keeping the pump's view aligned with the mux.
            Frame::ResetStream { stream_id, app_error_code, final_size } => {
                if !self.dispatch.is_active(*stream_id) {
                    return Ok(PumpEvent::Ignored);
                }
                // Commit the peer's final size to the connection receive budget
                // (RFC 9000 §4.5) before retiring the request.
                self.dispatch
                    .streams_mut()
                    .recv_reset(*stream_id, *final_size, *app_error_code)?;
                self.dispatch.abort(*stream_id);
                Ok(PumpEvent::Aborted {
                    stream_id: *stream_id,
                    app_error_code: *app_error_code,
                })
            }
            Frame::StopSending { stream_id, app_error_code } => {
                if !self.dispatch.is_active(*stream_id) {
                    return Ok(PumpEvent::Ignored);
                }
                match self
                    .dispatch
                    .streams_mut()
                    .recv_stop_sending(*stream_id, *app_error_code)?
                {
                    Some(reset) => Ok(PumpEvent::StopSending { stream_id: *stream_id, reset }),
                    // An in-flight request always has a send half, but stay
                    // defensive: nothing to reset means nothing to reply.
                    None => Ok(PumpEvent::Ignored),
                }
            }
            Frame::MaxStreamData { stream_id, max } => {
                if !self.dispatch.is_active(*stream_id) {
                    return Ok(PumpEvent::Ignored);
                }
                self.dispatch.streams_mut().recv_max_stream_data(*stream_id, *max)?;
                Ok(PumpEvent::Progress)
            }
            Frame::StreamDataBlocked { stream_id, .. } => {
                if !self.dispatch.is_active(*stream_id) {
                    return Ok(PumpEvent::Ignored);
                }
                self.dispatch.streams_mut().recv_stream_data_blocked(*stream_id)?;
                Ok(PumpEvent::Progress)
            }
            // Every other frame type is the connection layer's, not the pump's.
            _ => Ok(PumpEvent::Ignored),
        }
    }

    /// Whether an in-flight request currently owns `stream_id`.
    #[must_use]
    pub fn is_active(&self, stream_id: u64) -> bool {
        self.dispatch.is_active(stream_id)
    }

    /// The number of in-flight requests (sent, response not yet complete or failed).
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.dispatch.active_count()
    }

    /// Whether the request on `stream_id` has emitted its whole message and FIN —
    /// see [`RequestDispatch::request_flushed`].
    #[must_use]
    pub fn request_flushed(&self, stream_id: u64) -> bool {
        self.dispatch.request_flushed(stream_id)
    }

    /// The wrapped request dispatch, for the transport loop to reach the stream
    /// manager (re-advertising receive windows, applying ACKs to send streams).
    #[must_use]
    pub fn dispatch(&self) -> &RequestDispatch {
        &self.dispatch
    }

    /// The wrapped request dispatch, mutably.
    pub fn dispatch_mut(&mut self) -> &mut RequestDispatch {
        &mut self.dispatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::frame::Frame as H3Frame;
    use crate::h3::h3_request::H3Profile;
    use crate::h3::qpack::{self, HeaderField};
    use crate::h3::request_mux::MuxError;
    use crate::h3::stream::SendState;

    /// A permissive stream config: generous windows so flow control never interferes
    /// unless a test tightens it deliberately.
    fn config() -> StreamManagerConfig {
        StreamManagerConfig {
            initial_max_stream_data_bidi_local: 1 << 20,
            initial_max_stream_data_bidi_remote: 1 << 20,
            initial_max_stream_data_uni: 1 << 20,
            initial_max_data: 1 << 20,
            initial_max_streams_bidi: 100,
            initial_max_streams_uni: 100,
        }
    }

    fn pump() -> RequestPump {
        RequestPump::new(config(), 1 << 20)
    }

    /// A pump whose one request's send window is `peer_send_window` bytes.
    fn pump_with_send_window(peer_send_window: u64) -> RequestPump {
        RequestPump::new(config(), peer_send_window)
    }

    fn get(path: &'static [u8]) -> ClientRequest<'static> {
        ClientRequest {
            profile: H3Profile::Chrome,
            method: b"GET",
            scheme: b"https",
            authority: b"example.com",
            path,
            headers: &[],
            body: b"",
            use_huffman: true,
        }
    }

    fn post(path: &'static [u8], body: &'static [u8]) -> ClientRequest<'static> {
        ClientRequest {
            profile: H3Profile::Chrome,
            method: b"POST",
            scheme: b"https",
            authority: b"example.com",
            path,
            headers: &[],
            body,
            use_huffman: true,
        }
    }

    fn status(code: &[u8]) -> HeaderField {
        HeaderField::new(b":status".to_vec(), code.to_vec())
    }

    /// Encode the response-stream bytes for a `code`/`body` response.
    fn response_bytes(code: &[u8], body: &[u8]) -> Vec<u8> {
        let block = qpack::encode_field_section(&[status(code)], true);
        let mut out = Vec::new();
        H3Frame::Headers(block).encode(&mut out).unwrap();
        H3Frame::Data(body.to_vec()).encode(&mut out).unwrap();
        out
    }

    // ---- poll_transmit: outbound STREAM frames --------------------------

    #[test]
    fn poll_transmit_emits_the_request_as_a_finished_stream_frame() {
        let mut p = pump();
        let s = p.send_request(&get(b"/index.html")).unwrap().stream_id;
        let frames = p.poll_transmit(1 << 20);
        assert_eq!(frames.len(), 1);
        let Frame::Stream { stream_id, offset, fin, data } = &frames[0] else {
            panic!("expected a STREAM frame, got {:?}", frames[0]);
        };
        assert_eq!(*stream_id, s);
        assert_eq!(*offset, 0);
        assert!(*fin, "an HTTP/3 request stream is finished with the message");
        // The bytes are exactly one HEADERS frame (a GET has no body).
        let (frame, consumed) = H3Frame::parse(data).unwrap().unwrap();
        assert_eq!(consumed, data.len());
        assert!(matches!(frame, H3Frame::Headers(_)));
    }

    #[test]
    fn poll_transmit_drains_each_send_stream_once() {
        let mut p = pump();
        p.send_request(&get(b"/a")).unwrap();
        // The whole request left in the first drain; the send half is now quiescent.
        assert_eq!(p.poll_transmit(1 << 20).len(), 1);
        assert!(p.poll_transmit(1 << 20).is_empty());
    }

    #[test]
    fn poll_transmit_marks_the_send_half_flushed() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        assert!(!p.request_flushed(s));
        p.poll_transmit(1 << 20);
        assert!(p.request_flushed(s));
        assert_eq!(
            p.dispatch().streams().send_stream(s).unwrap().state(),
            SendState::DataSent
        );
    }

    #[test]
    fn poll_transmit_segments_a_large_request_by_the_frame_budget() {
        let mut p = pump();
        // A POST whose body forces the message past a small per-frame budget.
        let s = p.send_request(&post(b"/upload", &[b'x'; 200])).unwrap().stream_id;
        let frames = p.poll_transmit(64);
        assert!(frames.len() > 1, "a 200-byte body needs several 64-byte frames");
        // Offsets are contiguous and ascending; exactly one frame carries the FIN.
        let mut expected_offset = 0u64;
        let mut fin_count = 0;
        for frame in &frames {
            let Frame::Stream { stream_id, offset, fin, data } = frame else {
                panic!("expected STREAM frames");
            };
            assert_eq!(*stream_id, s);
            assert_eq!(*offset, expected_offset);
            assert!(data.len() <= 64);
            expected_offset += data.len() as u64;
            fin_count += usize::from(*fin);
        }
        assert_eq!(fin_count, 1, "exactly the last frame carries the FIN");
    }

    #[test]
    fn poll_transmit_multiplexes_concurrent_requests() {
        let mut p = pump();
        let a = p.send_request(&get(b"/a")).unwrap().stream_id;
        let b = p.send_request(&get(b"/b")).unwrap().stream_id;
        assert_eq!((a, b), (0, 4));
        let frames = p.poll_transmit(1 << 20);
        assert_eq!(frames.len(), 2);
        // Ascending by stream id: stream 0 before stream 4.
        let ids: Vec<u64> = frames
            .iter()
            .map(|f| match f {
                Frame::Stream { stream_id, .. } => *stream_id,
                other => panic!("expected STREAM frames, got {other:?}"),
            })
            .collect();
        assert_eq!(ids, vec![0, 4]);
    }

    #[test]
    fn poll_transmit_stops_at_the_send_flow_control_window() {
        // The peer grants only 4 bytes of send window, smaller than the request.
        let mut p = pump_with_send_window(4);
        let s = p.send_request(&get(b"/some/longer/path")).unwrap().stream_id;
        let frames = p.poll_transmit(1 << 20);
        // Exactly the 4 windowed bytes go out, without a FIN — the rest is blocked.
        assert_eq!(frames.len(), 1);
        let Frame::Stream { offset, fin, data, .. } = &frames[0] else {
            panic!("expected a STREAM frame");
        };
        assert_eq!(*offset, 0);
        assert_eq!(data.len(), 4);
        assert!(!*fin, "the request is not finished while blocked");
        // A second poll yields nothing until the window opens.
        assert!(p.poll_transmit(1 << 20).is_empty());
        // MAX_STREAM_DATA opens the window; the rest — and the FIN — now flow.
        assert_eq!(
            p.on_frame(&Frame::MaxStreamData { stream_id: s, max: 1 << 20 }).unwrap(),
            PumpEvent::Progress
        );
        let rest = p.poll_transmit(1 << 20);
        assert!(!rest.is_empty());
        assert!(rest.iter().any(|f| matches!(f, Frame::Stream { fin: true, .. })));
    }

    #[test]
    fn poll_transmit_is_empty_with_no_requests() {
        let mut p = pump();
        assert!(p.poll_transmit(1 << 20).is_empty());
    }

    // ---- on_frame: inbound STREAM -> response ---------------------------

    #[test]
    fn on_frame_stream_completes_a_response() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        let bytes = response_bytes(b"200", b"hello");
        let event = p
            .on_frame(&Frame::Stream { stream_id: s, offset: 0, fin: true, data: bytes })
            .unwrap();
        match event {
            PumpEvent::Response(resp) => {
                assert_eq!(resp.status, 200);
                assert_eq!(resp.body, b"hello");
            }
            other => panic!("expected a Response, got {other:?}"),
        }
        assert!(!p.is_active(s), "a completed request is retired");
        assert_eq!(p.active_count(), 0);
    }

    #[test]
    fn on_frame_stream_reports_progress_before_fin() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        let bytes = response_bytes(b"200", b"partial");
        // No FIN: the response is not yet complete.
        let event = p
            .on_frame(&Frame::Stream { stream_id: s, offset: 0, fin: false, data: bytes })
            .unwrap();
        assert_eq!(event, PumpEvent::Progress);
        assert!(p.is_active(s));
    }

    #[test]
    fn on_frame_stream_for_unknown_stream_is_a_mux_error() {
        let mut p = pump();
        let err = p
            .on_frame(&Frame::Stream { stream_id: 0, offset: 0, fin: true, data: Vec::new() })
            .unwrap_err();
        assert_eq!(err, DispatchError::Mux(MuxError::UnknownStream(0)));
    }

    #[test]
    fn on_frame_round_trips_a_request_and_response() {
        let mut p = pump();
        let s = p.send_request(&post(b"/echo", b"ping")).unwrap().stream_id;
        // Send side: the request drains out.
        let out = p.poll_transmit(1 << 20);
        assert!(out.iter().any(|f| matches!(f, Frame::Stream { fin: true, .. })));
        // Receive side: the response arrives on the same stream and completes.
        let resp = match p
            .on_frame(&Frame::Stream {
                stream_id: s,
                offset: 0,
                fin: true,
                data: response_bytes(b"200", b"pong"),
            })
            .unwrap()
        {
            PumpEvent::Response(r) => r,
            other => panic!("expected Response, got {other:?}"),
        };
        assert_eq!(resp.body, b"pong");
        assert_eq!(p.active_count(), 0);
    }

    // ---- on_frame: RESET_STREAM -> abort --------------------------------

    #[test]
    fn on_frame_reset_stream_aborts_the_request() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        let event = p
            .on_frame(&Frame::ResetStream { stream_id: s, app_error_code: 0x102, final_size: 0 })
            .unwrap();
        assert_eq!(event, PumpEvent::Aborted { stream_id: s, app_error_code: 0x102 });
        assert!(!p.is_active(s), "an aborted request is retired");
        assert_eq!(p.active_count(), 0);
    }

    #[test]
    fn on_frame_reset_stream_commits_the_final_size_to_the_budget() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        // Some response bytes arrived first, then a reset at a consistent final size.
        p.on_frame(&Frame::Stream { stream_id: s, offset: 0, fin: false, data: vec![0xAB; 5] })
            .unwrap();
        p.on_frame(&Frame::ResetStream { stream_id: s, app_error_code: 0, final_size: 5 })
            .unwrap();
        assert_eq!(p.dispatch().streams().recv_flow().received(), 5);
    }

    #[test]
    fn on_frame_reset_stream_with_contradictory_final_size_errors() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        // Five bytes received, then a reset claiming a smaller final size.
        p.on_frame(&Frame::Stream { stream_id: s, offset: 0, fin: false, data: vec![0xAB; 5] })
            .unwrap();
        let err = p
            .on_frame(&Frame::ResetStream { stream_id: s, app_error_code: 0, final_size: 2 })
            .unwrap_err();
        assert!(matches!(err, DispatchError::Stream(_)));
        // The request is still active — the erroring frame did not retire it.
        assert!(p.is_active(s));
    }

    // ---- on_frame: STOP_SENDING -> reset reply --------------------------

    #[test]
    fn on_frame_stop_sending_resets_send_half_and_yields_reset() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        // Emit some of the request so the reset's final size is the sent offset.
        p.poll_transmit(1 << 20);
        let sent = p.dispatch().streams().send_stream(s).unwrap().write_offset();
        let event = p.on_frame(&Frame::StopSending { stream_id: s, app_error_code: 0x99 }).unwrap();
        match event {
            PumpEvent::StopSending { stream_id, reset } => {
                assert_eq!(stream_id, s);
                assert_eq!(
                    reset,
                    Frame::ResetStream { stream_id: s, app_error_code: 0x99, final_size: sent }
                );
            }
            other => panic!("expected StopSending, got {other:?}"),
        }
        // The send half is reset; the request stays in flight for its response.
        assert_eq!(
            p.dispatch().streams().send_stream(s).unwrap().state(),
            SendState::ResetSent
        );
        assert!(p.is_active(s));
    }

    // ---- on_frame: flow-control frames ---------------------------------

    #[test]
    fn on_frame_max_stream_data_is_progress() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        assert_eq!(
            p.on_frame(&Frame::MaxStreamData { stream_id: s, max: 1 << 21 }).unwrap(),
            PumpEvent::Progress
        );
    }

    #[test]
    fn on_frame_stream_data_blocked_is_progress() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        assert_eq!(
            p.on_frame(&Frame::StreamDataBlocked { stream_id: s, limit: 100 }).unwrap(),
            PumpEvent::Progress
        );
    }

    // ---- on_frame: unknown streams and other frames --------------------

    #[test]
    fn on_frame_control_frame_for_unknown_stream_is_ignored() {
        let mut p = pump();
        // No request opened: every control frame is a no-op, not an error.
        assert_eq!(
            p.on_frame(&Frame::ResetStream { stream_id: 0, app_error_code: 0, final_size: 0 }).unwrap(),
            PumpEvent::Ignored
        );
        assert_eq!(
            p.on_frame(&Frame::StopSending { stream_id: 0, app_error_code: 0 }).unwrap(),
            PumpEvent::Ignored
        );
        assert_eq!(
            p.on_frame(&Frame::MaxStreamData { stream_id: 0, max: 10 }).unwrap(),
            PumpEvent::Ignored
        );
        assert_eq!(
            p.on_frame(&Frame::StreamDataBlocked { stream_id: 0, limit: 10 }).unwrap(),
            PumpEvent::Ignored
        );
        // And no phantom receive state was materialised for the stray stream.
        assert!(p.dispatch().streams().recv_stream_ref(0).is_none());
    }

    #[test]
    fn on_frame_control_frame_after_completion_is_ignored() {
        let mut p = pump();
        let s = p.send_request(&get(b"/a")).unwrap().stream_id;
        p.on_frame(&Frame::Stream {
            stream_id: s,
            offset: 0,
            fin: true,
            data: response_bytes(b"200", b"x"),
        })
        .unwrap();
        // The stream is retired; a late RESET_STREAM finds no request.
        assert_eq!(
            p.on_frame(&Frame::ResetStream { stream_id: s, app_error_code: 0, final_size: 0 }).unwrap(),
            PumpEvent::Ignored
        );
    }

    #[test]
    fn on_frame_non_stream_frame_is_ignored() {
        let mut p = pump();
        p.send_request(&get(b"/a")).unwrap();
        // A connection-level frame the pump does not own.
        assert_eq!(p.on_frame(&Frame::Ping).unwrap(), PumpEvent::Ignored);
        assert_eq!(p.on_frame(&Frame::MaxData(1 << 20)).unwrap(), PumpEvent::Ignored);
    }
}
