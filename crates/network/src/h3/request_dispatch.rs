//! HTTP/3 client request dispatch over QUIC streams (RFC 9000 §2.1, §3;
//! RFC 9114 §4.1, §6.1).
//!
//! This is the composition slice that finally joins the two towers built below
//! it: the HTTP/3 request tower ([`request_mux`](super::request_mux) over
//! [`request_exchange`](super::request_exchange) over the frame / QPACK codecs)
//! and the QUIC stream tower ([`stream_manager`](super::stream_manager) over
//! [`stream`](super::stream)). A [`RequestMux`] knows *which* client-initiated
//! bidirectional stream a request occupies and *what bytes* an exchange emits and
//! consumes; a [`StreamManager`] knows how those bytes are carried — buffered on a
//! [`SendStream`](super::stream::SendStream) for transmission, reassembled from
//! out-of-order STREAM frames on a [`RecvStream`](super::stream::RecvStream), and
//! flow-controlled on both directions. [`RequestDispatch`] owns both and wires
//! them together.
//!
//! Sending a request ([`RequestDispatch::send_request`]) allocates the stream via
//! the mux, opens the matching [`SendStream`](super::stream::SendStream), writes
//! the rendered request message, and closes the send half (STREAM FIN, RFC 9114
//! §4.1) — an HTTP/3 request is exactly one message, so the request stream is
//! finished the moment it is written. The surrounding send loop later drains that
//! send stream into STREAM frames.
//!
//! Receiving ([`RequestDispatch::on_stream_frame`]) is the mirror: the connection
//! delivers an inbound STREAM frame for a request stream, the dispatch routes it
//! into the [`StreamManager`] reassembly, drains the newly-contiguous ordered
//! prefix, detects the receive-side STREAM FIN (the server's end of the response),
//! and feeds both to the owning exchange through the mux — yielding the assembled
//! [`H3Response`] on the frame that carries the FIN.
//!
//! Like every slice below it, [`RequestDispatch`] is a pure state machine: no
//! sockets, no timers, no packet protection. It answers the two questions the
//! transport loop asks — *I have a request, which stream does it go on and what do
//! I write*, and *a STREAM frame arrived on stream N, does it complete a response*
//! — while the STREAM-frame framing itself (send via
//! [`SendStream::poll_transmit`](super::stream::SendStream::poll_transmit), receive
//! via the connection's frame dispatch) stays in the transport layers around it.

use super::h3_exchange::{BodySink, H3Response};
use super::request_exchange::ClientRequest;
use super::request_mux::{MuxError, OpenError, RequestMux};
use super::stream::SendState;
use super::stream_manager::{
    StreamManager, StreamManagerConfig, StreamManagerError, recv_stream_finished,
};

/// An error dispatching a client request onto, or a response frame off of, a QUIC
/// stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DispatchError {
    /// Opening the request failed: the request could not be built (RFC 9114
    /// §4.2/§7.2.1) or every client-initiated bidirectional stream identifier is
    /// spent (RFC 9000 §2.1). No stream is consumed.
    Open(OpenError),
    /// A received STREAM / RESET_STREAM frame violated the QUIC stream layer — a
    /// per-stream or connection-level flow-control breach, a final-size
    /// contradiction, or a stream-state error (RFC 9000 §4.1, §4.5, §4.6).
    Stream(StreamManagerError),
    /// The response bytes on the addressed stream were rejected by the HTTP/3
    /// layer — a malformed message or an RFC 9114 §4.1 grammar violation — or no
    /// in-flight request owns the stream (never opened, or already completed and
    /// retired). The mux has retired the stream on a grammar failure.
    Mux(MuxError),
}

impl core::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Open(e) => write!(f, "request dispatch open: {e}"),
            Self::Stream(e) => write!(f, "request dispatch stream: {e}"),
            Self::Mux(e) => write!(f, "request dispatch route: {e}"),
        }
    }
}

impl std::error::Error for DispatchError {}

impl From<OpenError> for DispatchError {
    fn from(e: OpenError) -> Self {
        Self::Open(e)
    }
}

impl From<StreamManagerError> for DispatchError {
    fn from(e: StreamManagerError) -> Self {
        Self::Stream(e)
    }
}

impl From<MuxError> for DispatchError {
    fn from(e: MuxError) -> Self {
        Self::Mux(e)
    }
}

/// A request placed onto a QUIC stream: the client-initiated bidirectional stream
/// it occupies (RFC 9000 §2.1). The request message has already been written to
/// that stream's [`SendStream`](super::stream::SendStream) with the send half
/// closed (STREAM FIN); the transport loop drains it into STREAM frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SentRequest {
    /// The client-initiated bidirectional stream carrying the request/response.
    pub stream_id: u64,
}

/// Dispatches HTTP/3 client requests over QUIC streams: the join of the request
/// multiplexer ([`RequestMux`]) and the QUIC stream manager ([`StreamManager`]).
///
/// Owns one of each and keeps them in lockstep — a request opened in the mux gets
/// its send stream opened, written, and finished in the manager; a STREAM frame
/// routed through the manager's reassembly gets its ordered bytes fed back to the
/// mux.
#[derive(Debug)]
pub struct RequestDispatch {
    /// The HTTP/3 request multiplexer: stream-id allocation and response routing.
    mux: RequestMux,
    /// The QUIC stream manager: per-stream send/receive halves and flow control.
    streams: StreamManager,
    /// The peer's `initial_max_stream_data_bidi_remote` (RFC 9000 §18.2): the
    /// send-side flow-control window for a bidirectional stream *we* initiate —
    /// "remote" from the peer's point of view. Seeds each request's send stream.
    peer_initial_max_stream_data_bidi_remote: u64,
}

impl RequestDispatch {
    /// Builds a dispatch advertising the receive windows and stream-count limits in
    /// `stream_config` (RFC 9000 §18.2), and seeding each request's send half with
    /// the peer's `initial_max_stream_data_bidi_remote` send window
    /// (`peer_initial_max_stream_data_bidi_remote`).
    #[must_use]
    pub fn new(
        stream_config: StreamManagerConfig,
        peer_initial_max_stream_data_bidi_remote: u64,
    ) -> Self {
        Self {
            mux: RequestMux::new(),
            streams: StreamManager::new(stream_config),
            peer_initial_max_stream_data_bidi_remote,
        }
    }

    /// Places `req` onto a fresh client-initiated bidirectional stream: allocate the
    /// stream in the mux, render the request message, open the send half, write the
    /// message, and close the send half (STREAM FIN, RFC 9114 §4.1). An HTTP/3
    /// request is a single message, so the request stream is finished as soon as it
    /// is written; the transport loop drains it via
    /// [`SendStream::poll_transmit`](super::stream::SendStream::poll_transmit).
    ///
    /// Returns the [`SentRequest`] naming the stream. The stream identifier is
    /// consumed only on success — a request that cannot be built burns no id.
    ///
    /// # Errors
    ///
    /// [`DispatchError::Open`] if the request cannot be built (RFC 9114
    /// §4.2/§7.2.1) or all client bidirectional stream identifiers are spent
    /// (RFC 9000 §2.1). The stream manager is left untouched.
    pub fn send_request(&mut self, req: &ClientRequest) -> Result<SentRequest, DispatchError> {
        let opened = self.mux.open(req)?;
        let send = self
            .streams
            .open_send_stream(opened.stream_id, self.peer_initial_max_stream_data_bidi_remote);
        send.write(&opened.bytes);
        send.finish();
        Ok(SentRequest { stream_id: opened.stream_id })
    }

    /// Routes an inbound STREAM frame for a request stream: `offset`/`data` is the
    /// carried byte range (possibly empty) and `fin` marks the server's STREAM FIN.
    ///
    /// Reassembles the frame in the QUIC stream layer, drains the newly-contiguous
    /// ordered prefix, and feeds it — with the receive-side FIN detected once the
    /// whole response has been consumed — to the owning exchange. Returns
    /// `Ok(Some(response))` on the frame that completes the response (retiring the
    /// stream from the mux) and `Ok(None)` while more is expected.
    ///
    /// Equivalent to [`on_stream_frame_with_sink`](Self::on_stream_frame_with_sink)
    /// with `sink = None`.
    ///
    /// # Errors
    ///
    /// - [`DispatchError::Stream`] if the frame breaches QUIC flow control or the
    ///   final-size invariants (RFC 9000 §4.1, §4.5).
    /// - [`DispatchError::Mux`] with [`MuxError::UnknownStream`] if no in-flight
    ///   request owns `stream_id` (never opened, or already completed and retired),
    ///   or [`MuxError::Exchange`] if the response stream is malformed (RFC 9114
    ///   §4.1) — the failed stream is retired.
    pub fn on_stream_frame(
        &mut self,
        stream_id: u64,
        offset: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<Option<H3Response>, DispatchError> {
        self.on_stream_frame_with_sink(stream_id, offset, data, fin, None)
    }

    /// Routes an inbound STREAM frame for a request stream, forwarding `DATA`
    /// frame payloads to `sink` as they arrive.
    ///
    /// Reassembles the frame in the QUIC stream layer, drains the newly-contiguous
    /// ordered prefix, and feeds it — with the receive-side FIN detected once the
    /// whole response has been consumed — to the owning exchange. Returns
    /// `Ok(Some(response))` on the frame that completes the response (retiring the
    /// stream from the mux) and `Ok(None)` while more is expected.
    ///
    /// When `sink` is `Some`, each `DATA` frame payload is forwarded to it as the
    /// QUIC stream layer delivers ordered bytes. Passing `None` is equivalent to
    /// [`on_stream_frame`](Self::on_stream_frame).
    ///
    /// # Errors
    ///
    /// - [`DispatchError::Stream`] if the frame breaches QUIC flow control or the
    ///   final-size invariants (RFC 9000 §4.1, §4.5).
    /// - [`DispatchError::Mux`] with [`MuxError::UnknownStream`] if no in-flight
    ///   request owns `stream_id` (never opened, or already completed and retired),
    ///   or [`MuxError::Exchange`] if the response stream is malformed (RFC 9114
    ///   §4.1) — the failed stream is retired.
    pub fn on_stream_frame_with_sink(
        &mut self,
        stream_id: u64,
        offset: u64,
        data: &[u8],
        fin: bool,
        sink: Option<BodySink<'_>>,
    ) -> Result<Option<H3Response>, DispatchError> {
        // Reject bytes for a stream with no in-flight request before touching the
        // reassembly, mirroring the mux's own contract (never opened, or completed
        // and retired). This keeps the two layers' views of "known stream" aligned
        // and avoids materialising phantom receive state for a stray stream.
        if !self.mux.is_active(stream_id) {
            return Err(DispatchError::Mux(MuxError::UnknownStream(stream_id)));
        }
        self.streams.recv_stream(stream_id, offset, data, fin)?;
        self.pump_recv_with_sink(stream_id, sink)
    }

    /// Drains every ordered byte the last frame made readable on `stream_id` into
    /// the owning exchange, detecting the receive STREAM FIN once the whole stream
    /// is consumed. Returns the response on completion.
    fn pump_recv_with_sink(
        &mut self,
        stream_id: u64,
        mut sink: Option<BodySink<'_>>,
    ) -> Result<Option<H3Response>, DispatchError> {
        loop {
            let chunk = self.streams.read(stream_id);
            // The receive half reaches `DataRead` the moment the read cursor passes
            // the FIN's final size — which happens either here on the read that pops
            // the last byte, or already in `recv` when a bare FIN arrives after the
            // body was drained. Detecting it off the stream state covers both.
            let finished = self
                .streams
                .recv_stream_ref(stream_id)
                .is_some_and(recv_stream_finished);
            // Nothing new to deliver and the stream is not finished: a gap precedes
            // the next buffered bytes, or this frame added none. Wait for more.
            if chunk.is_empty() && !finished {
                return Ok(None);
            }
            let sink_ref = sink.as_mut().map(|f| &mut **f as &mut dyn FnMut(&[u8]));
            match self.mux.on_recv_with_sink(stream_id, &chunk, finished, sink_ref)? {
                Some(response) => return Ok(Some(response)),
                None => {
                    if finished {
                        // The QUIC stream ended, yet the mux did not complete the
                        // response. An HTTP/3 response always finishes on FIN, so
                        // `on_recv_with_sink` with `fin` would have errored on a
                        // truncated message rather than returning `None`; there is
                        // nothing more to feed, so stop.
                        return Ok(None);
                    }
                    if chunk.is_empty() {
                        return Ok(None);
                    }
                    // A non-empty chunk before the FIN: loop to drain any further
                    // contiguous prefix a reorder may have made readable at once.
                }
            }
        }
    }

    /// Retires the in-flight request on `stream_id` without a response — the
    /// transport tore the stream down (the peer reset the response half with
    /// RESET_STREAM, RFC 9000 §19.4). The mux identifier no longer routes; the
    /// stream layer's receive state is left as the reset put it. Returns whether an
    /// in-flight request was actually retired.
    pub fn abort(&mut self, stream_id: u64) -> bool {
        self.mux.abort(stream_id)
    }

    /// Whether an in-flight request currently owns `stream_id`.
    #[must_use]
    pub fn is_active(&self, stream_id: u64) -> bool {
        self.mux.is_active(stream_id)
    }

    /// The number of in-flight requests (sent, response not yet complete or failed).
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.mux.active_count()
    }

    /// The QUIC stream manager, for the transport loop to drain send streams into
    /// STREAM frames and re-advertise receive windows.
    #[must_use]
    pub fn streams(&self) -> &StreamManager {
        &self.streams
    }

    /// The QUIC stream manager, mutably — the transport loop's handle to poll send
    /// streams for transmission and apply peer flow-control frames.
    pub fn streams_mut(&mut self) -> &mut StreamManager {
        &mut self.streams
    }

    /// Whether the request stream `stream_id` has emitted its whole message and FIN
    /// (its send half is at [`SendState::DataSent`] or later) — the request is fully
    /// on the wire pending acknowledgement.
    #[must_use]
    pub fn request_flushed(&self, stream_id: u64) -> bool {
        self.streams
            .send_stream(stream_id)
            .is_some_and(|s| matches!(s.state(), SendState::DataSent | SendState::DataRecvd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::frame::Frame;
    use crate::h3::h3_request::{H3Profile, MessageError};
    use crate::h3::qpack::{self, HeaderField};
    use crate::h3::request_exchange::ExchangeError;
    use crate::h3::stream::{SendState, is_bidirectional, is_client_initiated};

    /// A permissive stream config: generous windows so the flow-control layer never
    /// interferes with the routing tests.
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

    fn dispatch() -> RequestDispatch {
        RequestDispatch::new(config(), 1 << 20)
    }

    /// A minimal GET request with no extra headers and no body.
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

    /// A POST request carrying `body`.
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

    fn headers_frame(fields: &[HeaderField]) -> Vec<u8> {
        let block = qpack::encode_field_section(fields, true);
        let mut out = Vec::new();
        Frame::Headers(block).encode(&mut out).unwrap();
        out
    }

    fn data_frame(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        Frame::Data(payload.to_vec()).encode(&mut out).unwrap();
        out
    }

    #[test]
    fn send_request_opens_and_finishes_the_send_stream() {
        let mut d = dispatch();
        let sent = d.send_request(&get(b"/index.html")).unwrap();
        assert_eq!(sent.stream_id, 0);
        assert!(is_client_initiated(sent.stream_id) && is_bidirectional(sent.stream_id));
        assert!(d.is_active(0));
        assert_eq!(d.active_count(), 1);

        // The send half exists, holds the whole request, and has requested FIN.
        let send = d.streams().send_stream(0).unwrap();
        assert_eq!(send.state(), SendState::Send);
        assert!(send.write_offset() > 0);
    }

    #[test]
    fn send_stream_carries_exactly_the_request_message() {
        let mut d = dispatch();
        d.send_request(&get(b"/p")).unwrap();
        // Drain the whole send stream (one poll suffices for this small request).
        let chunk = d.streams_mut().send_stream_mut(0).unwrap().poll_transmit(1 << 20).unwrap();
        assert!(chunk.fin, "the request stream is finished with FIN");
        // The bytes are exactly one HEADERS frame (a GET carries no body).
        let (frame, consumed) = Frame::parse(&chunk.data).unwrap().unwrap();
        assert_eq!(consumed, chunk.data.len());
        let Frame::Headers(block) = frame else {
            panic!("expected HEADERS frame");
        };
        let fields = qpack::decode_field_section(&block).unwrap();
        let path = fields.iter().find(|f| f.name == b":path").unwrap();
        assert_eq!(path.value, b"/p");
    }

    #[test]
    fn post_body_rides_the_send_stream_after_headers() {
        let mut d = dispatch();
        d.send_request(&post(b"/submit", b"hello")).unwrap();
        let chunk = d.streams_mut().send_stream_mut(0).unwrap().poll_transmit(1 << 20).unwrap();
        assert!(chunk.fin);
        // HEADERS then DATA(hello).
        let (h, n) = Frame::parse(&chunk.data).unwrap().unwrap();
        assert!(matches!(h, Frame::Headers(_)));
        let (body, _) = Frame::parse(&chunk.data[n..]).unwrap().unwrap();
        assert_eq!(body, Frame::Data(b"hello".to_vec()));
    }

    #[test]
    fn response_in_one_frame_completes_and_retires() {
        let mut d = dispatch();
        let s = d.send_request(&get(b"/a")).unwrap().stream_id;
        let mut bytes = headers_frame(&[status(b"200")]);
        bytes.extend(data_frame(b"body"));
        let resp = d.on_stream_frame(s, 0, &bytes, true).unwrap().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"body");
        // Retired from the mux; the QUIC receive half has fully drained.
        assert!(!d.is_active(s));
        assert_eq!(d.active_count(), 0);
    }

    #[test]
    fn response_across_frames_reassembles_before_completing() {
        let mut d = dispatch();
        let s = d.send_request(&get(b"/a")).unwrap().stream_id;
        let head = headers_frame(&[status(b"200")]);
        let body = data_frame(b"chunked");
        // Head first, no FIN.
        assert_eq!(d.on_stream_frame(s, 0, &head, false).unwrap(), None);
        assert!(d.is_active(s));
        // Body with FIN completes it.
        let resp = d
            .on_stream_frame(s, head.len() as u64, &body, true)
            .unwrap()
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"chunked");
        assert!(!d.is_active(s));
    }

    #[test]
    fn out_of_order_frames_are_reordered_by_the_stream_layer() {
        let mut d = dispatch();
        let s = d.send_request(&get(b"/a")).unwrap().stream_id;
        let head = headers_frame(&[status(b"204")]);
        let body = data_frame(b"z");
        // Deliver the tail (with FIN) before the head: the reassembly holds it, no
        // response yet because the prefix has a gap.
        assert_eq!(
            d.on_stream_frame(s, head.len() as u64, &body, true).unwrap(),
            None
        );
        assert!(d.is_active(s));
        // The head fills the gap; the whole stream is now contiguous and finished.
        let resp = d.on_stream_frame(s, 0, &head, false).unwrap().unwrap();
        assert_eq!(resp.status, 204);
        assert_eq!(resp.body, b"z");
        assert!(!d.is_active(s));
    }

    #[test]
    fn bare_fin_after_body_completes_the_response() {
        let mut d = dispatch();
        let s = d.send_request(&get(b"/a")).unwrap().stream_id;
        let mut bytes = headers_frame(&[status(b"200")]);
        bytes.extend(data_frame(b"done"));
        // All the message bytes, but no FIN yet.
        assert_eq!(d.on_stream_frame(s, 0, &bytes, false).unwrap(), None);
        assert!(d.is_active(s));
        // A trailing empty frame carrying only the FIN completes it — the receive
        // half transitions to DataRead in `recv`, not on a read.
        let resp = d
            .on_stream_frame(s, bytes.len() as u64, &[], true)
            .unwrap()
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"done");
        assert!(!d.is_active(s));
    }

    #[test]
    fn concurrent_requests_route_independently() {
        let mut d = dispatch();
        let a = d.send_request(&get(b"/a")).unwrap().stream_id;
        let b = d.send_request(&get(b"/b")).unwrap().stream_id;
        assert_eq!((a, b), (0, 4));
        assert_eq!(d.active_count(), 2);

        // Interleave: head of a, all of b, tail of a.
        assert_eq!(
            d.on_stream_frame(a, 0, &headers_frame(&[status(b"201")]), false)
                .unwrap(),
            None
        );
        let mut b_bytes = headers_frame(&[status(b"202")]);
        b_bytes.extend(data_frame(b"bb"));
        let rb = d.on_stream_frame(b, 0, &b_bytes, true).unwrap().unwrap();
        assert_eq!((rb.status, rb.body.as_slice()), (202, b"bb".as_slice()));
        assert!(!d.is_active(b) && d.is_active(a));

        let head_len = headers_frame(&[status(b"201")]).len() as u64;
        let ra = d
            .on_stream_frame(a, head_len, &data_frame(b"aa"), true)
            .unwrap()
            .unwrap();
        assert_eq!((ra.status, ra.body.as_slice()), (201, b"aa".as_slice()));
        assert_eq!(d.active_count(), 0);
    }

    #[test]
    fn frame_for_never_opened_stream_is_rejected() {
        let mut d = dispatch();
        let err = d.on_stream_frame(0, 0, &[], true).unwrap_err();
        assert_eq!(err, DispatchError::Mux(MuxError::UnknownStream(0)));
    }

    #[test]
    fn frame_after_completion_is_rejected() {
        let mut d = dispatch();
        let s = d.send_request(&get(b"/a")).unwrap().stream_id;
        d.on_stream_frame(s, 0, &headers_frame(&[status(b"200")]), true)
            .unwrap()
            .unwrap();
        // A late frame for the retired stream finds no in-flight request.
        let err = d.on_stream_frame(s, 0, &[], true).unwrap_err();
        assert_eq!(err, DispatchError::Mux(MuxError::UnknownStream(s)));
    }

    #[test]
    fn malformed_response_retires_the_stream_with_a_mux_error() {
        let mut d = dispatch();
        let s = d.send_request(&get(b"/a")).unwrap().stream_id;
        // A HEADERS frame with no :status is a malformed message (RFC 9114 §4.3.2).
        let head = headers_frame(&[HeaderField::new(
            b"content-type".to_vec(),
            b"text/plain".to_vec(),
        )]);
        let err = d.on_stream_frame(s, 0, &head, true).unwrap_err();
        assert!(matches!(
            err,
            DispatchError::Mux(MuxError::Exchange {
                stream_id,
                error: ExchangeError::Assemble(_),
            }) if stream_id == s
        ));
        assert!(!d.is_active(s));
    }

    #[test]
    fn bad_request_burns_no_stream_and_no_send_state() {
        let mut d = dispatch();
        // An uppercase header name is rejected at request build (RFC 9114 §4.2).
        let bad = ClientRequest {
            headers: &[(b"Accept", b"x")],
            ..get(b"/")
        };
        let err = d.send_request(&bad).unwrap_err();
        assert_eq!(
            err,
            DispatchError::Open(OpenError::Request(ExchangeError::Request(
                MessageError::UppercaseName(b"Accept".to_vec())
            )))
        );
        assert_eq!(d.active_count(), 0);
        assert!(d.streams().send_stream(0).is_none());
        // The next good request still gets stream 0 — the failed open burned no id.
        assert_eq!(d.send_request(&get(b"/")).unwrap().stream_id, 0);
    }

    #[test]
    fn interim_response_keeps_the_stream_active() {
        let mut d = dispatch();
        let s = d.send_request(&get(b"/a")).unwrap().stream_id;
        // A 100-continue head without FIN: not complete, stream stays.
        let interim = headers_frame(&[status(b"100")]);
        assert_eq!(d.on_stream_frame(s, 0, &interim, false).unwrap(), None);
        assert!(d.is_active(s));
        // The final head + body with FIN completes it, carrying the interim status.
        let mut rest = headers_frame(&[status(b"200")]);
        rest.extend(data_frame(b"ok"));
        let resp = d
            .on_stream_frame(s, interim.len() as u64, &rest, true)
            .unwrap()
            .unwrap();
        assert_eq!(resp.informational, vec![100]);
        assert_eq!(resp.status, 200);
        assert!(!d.is_active(s));
    }

    #[test]
    fn request_flushed_tracks_the_send_half() {
        let mut d = dispatch();
        let s = d.send_request(&get(b"/a")).unwrap().stream_id;
        // Not flushed until the send half emits its bytes and FIN.
        assert!(!d.request_flushed(s));
        let _ = d.streams_mut().send_stream_mut(s).unwrap().poll_transmit(1 << 20).unwrap();
        assert!(d.request_flushed(s));
    }
}
