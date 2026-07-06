//! HTTP/3 client request multiplexer (RFC 9000 §2.1, RFC 9114 §6.1).
//!
//! This is the composition slice above [`request_exchange`](super::request_exchange):
//! a single [`ClientExchange`] models the client side of *one* request/response
//! exchange on *one* stream, but an HTTP/3 connection carries many concurrent
//! requests, each on its own client-initiated bidirectional stream (RFC 9114
//! §6.1). [`RequestMux`] owns that fan-out: it allocates the stream identifier
//! for each new request (RFC 9000 §2.1), tracks the in-flight [`ClientExchange`]
//! keyed by that identifier, and routes inbound response-stream bytes back to the
//! exchange the transport delivered them for.
//!
//! Client-initiated bidirectional stream identifiers are `0, 4, 8, …` — the two
//! low bits are `0b00` (bit 0 clear = client-initiated, bit 1 clear =
//! bidirectional, RFC 9000 §2.1), so the *n*-th such stream has identifier
//! `4 * n`. The mux hands them out in ascending order, which is also the order
//! HTTP/3 requires request streams to be opened relative to each other on a
//! connection.
//!
//! Like every slice below it, [`RequestMux`] is a pure state machine — no IO, no
//! timers, no QUIC, no flow control (the peer's `MAX_STREAMS` limit and the
//! stream-level byte accounting live in [`stream_manager`](super::stream_manager)).
//! It answers two questions for the surrounding `h3_do_request` dispatch: *which
//! stream does this new request go on, and what bytes do I write to it*, and
//! *which pending request owns the bytes that just arrived on stream N*.

use std::collections::BTreeMap;

use super::h3_exchange::H3Response;
use super::request_exchange::{ClientExchange, ClientRequest, ExchangeError};

/// The identifier of the first client-initiated bidirectional stream and the
/// stride between successive ones (RFC 9000 §2.1): the two low bits are `0b00`,
/// so identifiers are `0, 4, 8, …`.
const CLIENT_BIDI_STRIDE: u64 = 4;

/// The number of distinct stream identifiers of one type. Stream identifiers are
/// 62-bit values (RFC 9000 §16), and the low two bits select the type, leaving
/// `2^60` identifiers per type — the mux is exhausted once it has handed them all
/// out (a purely theoretical bound; a real connection is limited far earlier by
/// the peer's `MAX_STREAMS`).
const MAX_CLIENT_BIDI_STREAMS: u64 = 1 << 60;

/// An error routing response-stream bytes through the multiplexer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MuxError {
    /// Bytes were delivered for a stream identifier the mux has no in-flight
    /// request for — it was never opened, or it already completed or failed and
    /// was retired. The dispatch delivered to the wrong stream, or the peer sent
    /// data on a stream we never used.
    UnknownStream(u64),
    /// The exchange for the addressed stream rejected the bytes — a malformed
    /// response or an RFC 9114 §4.1 grammar violation. The failed stream is
    /// retired from the mux; its identifier's `RESET_STREAM`/`STOP_SENDING` is the
    /// dispatch's responsibility.
    Exchange {
        /// The stream the failing exchange was bound to.
        stream_id: u64,
        /// The underlying exchange error.
        error: ExchangeError,
    },
}

impl core::fmt::Display for MuxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownStream(id) => {
                write!(f, "no in-flight request on stream {id}")
            }
            Self::Exchange { stream_id, error } => {
                write!(f, "request on stream {stream_id} failed: {error}")
            }
        }
    }
}

impl std::error::Error for MuxError {}

/// An error allocating a stream for a new request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// Rendering the request onto its stream failed (RFC 9114 §4.2/§7.2.1) — the
    /// underlying [`ClientExchange::start`] error. No stream identifier is
    /// consumed when the request cannot be built.
    Request(ExchangeError),
    /// Every client-initiated bidirectional stream identifier has been handed out
    /// (RFC 9000 §2.1). Unreachable on any real connection.
    StreamsExhausted,
}

impl core::fmt::Display for OpenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Request(e) => write!(f, "request build: {e}"),
            Self::StreamsExhausted => {
                write!(f, "client bidirectional stream identifiers exhausted")
            }
        }
    }
}

impl std::error::Error for OpenError {}

/// A newly opened request: the client-initiated bidirectional stream it occupies
/// and the request-stream bytes to write to that stream with the send half closed
/// (STREAM FIN), per [`ClientExchange::start`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedRequest {
    /// The client-initiated bidirectional stream identifier (RFC 9000 §2.1).
    pub stream_id: u64,
    /// The complete request message to send with FIN (RFC 9114 §4.1).
    pub bytes: Vec<u8>,
}

/// Multiplexes many concurrent HTTP/3 client requests over one connection
/// (RFC 9114 §6.1): allocates each request's client-initiated bidirectional
/// stream (RFC 9000 §2.1) and routes inbound response bytes to the right
/// in-flight [`ClientExchange`].
#[derive(Clone, Debug, Default)]
pub struct RequestMux {
    /// The next client-initiated bidirectional stream number to allocate; the
    /// identifier handed out is `next_stream_number * CLIENT_BIDI_STRIDE`.
    next_stream_number: u64,
    /// The in-flight exchanges, keyed by their stream identifier. An exchange is
    /// removed the moment it completes (its [`H3Response`] is yielded) or fails.
    exchanges: BTreeMap<u64, ClientExchange>,
}

impl RequestMux {
    /// A fresh multiplexer with no in-flight requests, ready to hand out stream
    /// `0` to the first request.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a new request: allocate the next client-initiated bidirectional
    /// stream, render the request bytes, and track the exchange awaiting its
    /// response.
    ///
    /// Returns the [`OpenedRequest`] whose `bytes` the caller writes to
    /// `stream_id` with the send half closed (STREAM FIN, RFC 9114 §4.1). The
    /// stream number is advanced only on success, so a rejected request does not
    /// burn an identifier.
    ///
    /// # Errors
    ///
    /// - [`OpenError::Request`] if the request cannot be built (RFC 9114
    ///   §4.2/§7.2.1).
    /// - [`OpenError::StreamsExhausted`] if all `2^60` client bidirectional stream
    ///   identifiers have been allocated (RFC 9000 §2.1).
    pub fn open(&mut self, req: &ClientRequest) -> Result<OpenedRequest, OpenError> {
        if self.next_stream_number >= MAX_CLIENT_BIDI_STREAMS {
            return Err(OpenError::StreamsExhausted);
        }
        let (exchange, bytes) = ClientExchange::start(req).map_err(OpenError::Request)?;
        let stream_id = self.next_stream_number * CLIENT_BIDI_STRIDE;
        self.next_stream_number += 1;
        self.exchanges.insert(stream_id, exchange);
        Ok(OpenedRequest { stream_id, bytes })
    }

    /// Feed response-stream bytes the transport delivered on `stream_id`.
    ///
    /// `data` is the received bytes (possibly empty), `fin` marks the server's
    /// STREAM FIN. Returns `Ok(Some(response))` once that stream's response is
    /// fully assembled (only on the call carrying its `fin`) — at which point the
    /// exchange is retired from the mux — and `Ok(None)` while more is expected.
    ///
    /// A failing exchange is retired too: its error is surfaced and its identifier
    /// no longer routes.
    ///
    /// # Errors
    ///
    /// - [`MuxError::UnknownStream`] if no in-flight request owns `stream_id`.
    /// - [`MuxError::Exchange`] if the addressed exchange rejects the bytes
    ///   (RFC 9114 §4.1); the failed stream is retired.
    pub fn on_recv(
        &mut self,
        stream_id: u64,
        data: &[u8],
        fin: bool,
    ) -> Result<Option<H3Response>, MuxError> {
        let exchange = self
            .exchanges
            .get_mut(&stream_id)
            .ok_or(MuxError::UnknownStream(stream_id))?;
        match exchange.on_recv(data, fin) {
            Ok(Some(response)) => {
                // The exchange is complete; retire it so the identifier no longer
                // routes and the response is yielded exactly once.
                self.exchanges.remove(&stream_id);
                Ok(Some(response))
            }
            Ok(None) => Ok(None),
            Err(error) => {
                // A failed exchange is terminal; retire it so a stale identifier
                // does not linger, and surface the error to the dispatch.
                self.exchanges.remove(&stream_id);
                Err(MuxError::Exchange { stream_id, error })
            }
        }
    }

    /// Whether an in-flight request currently owns `stream_id`.
    #[must_use]
    pub fn is_active(&self, stream_id: u64) -> bool {
        self.exchanges.contains_key(&stream_id)
    }

    /// The number of in-flight requests (opened, not yet completed or failed).
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.exchanges.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::frame::Frame;
    use crate::h3::h3_request::{H3Profile, MessageError};
    use crate::h3::qpack::{self, HeaderField};
    use crate::h3::request_exchange::ExchangeError;
    use crate::h3::stream::{is_bidirectional, is_client_initiated};

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

    fn status(code: &[u8]) -> HeaderField {
        HeaderField::new(b":status".to_vec(), code.to_vec())
    }

    /// Encode a `HEADERS` frame carrying `fields`.
    fn headers_frame(fields: &[HeaderField]) -> Vec<u8> {
        let block = qpack::encode_field_section(fields, true);
        let mut out = Vec::new();
        Frame::Headers(block).encode(&mut out).unwrap();
        out
    }

    /// Encode a `DATA` frame carrying `payload`.
    fn data_frame(payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        Frame::Data(payload.to_vec()).encode(&mut out).unwrap();
        out
    }

    #[test]
    fn first_request_uses_stream_zero() {
        let mut mux = RequestMux::new();
        let opened = mux.open(&get(b"/")).unwrap();
        assert_eq!(opened.stream_id, 0);
        assert!(is_client_initiated(opened.stream_id) && is_bidirectional(opened.stream_id));
        assert!(mux.is_active(0));
        assert_eq!(mux.active_count(), 1);
    }

    #[test]
    fn stream_ids_ascend_by_four() {
        let mut mux = RequestMux::new();
        for expected in [0u64, 4, 8, 12, 16] {
            let opened = mux.open(&get(b"/")).unwrap();
            assert_eq!(opened.stream_id, expected);
            assert!(is_client_initiated(opened.stream_id) && is_bidirectional(opened.stream_id));
        }
        assert_eq!(mux.active_count(), 5);
    }

    #[test]
    fn opened_bytes_are_the_exchange_request() {
        let mut mux = RequestMux::new();
        let opened = mux.open(&get(b"/index.html")).unwrap();
        // The bytes are exactly one HEADERS frame (a GET carries no body).
        let (frame, consumed) = Frame::parse(&opened.bytes).unwrap().unwrap();
        assert_eq!(consumed, opened.bytes.len());
        let Frame::Headers(block) = frame else {
            panic!("expected HEADERS frame");
        };
        let fields = qpack::decode_field_section(&block).unwrap();
        let path = fields.iter().find(|f| f.name == b":path").unwrap();
        assert_eq!(path.value, b"/index.html");
    }

    #[test]
    fn response_routes_to_its_stream_and_retires_it() {
        let mut mux = RequestMux::new();
        let a = mux.open(&get(b"/a")).unwrap().stream_id;
        let b = mux.open(&get(b"/b")).unwrap().stream_id;
        assert_eq!((a, b), (0, 4));

        let mut stream = headers_frame(&[status(b"200")]);
        stream.extend(data_frame(b"body-a"));
        let resp = mux.on_recv(a, &stream, true).unwrap().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"body-a");

        // Stream a is retired; stream b is untouched and still in flight.
        assert!(!mux.is_active(a));
        assert!(mux.is_active(b));
        assert_eq!(mux.active_count(), 1);
    }

    #[test]
    fn concurrent_responses_interleave_across_chunks() {
        let mut mux = RequestMux::new();
        let a = mux.open(&get(b"/a")).unwrap().stream_id;
        let b = mux.open(&get(b"/b")).unwrap().stream_id;

        // Head of a, then all of b, then tail of a — the mux keeps each exchange's
        // partial state separate.
        assert_eq!(mux.on_recv(a, &headers_frame(&[status(b"201")]), false).unwrap(), None);

        let mut b_stream = headers_frame(&[status(b"202")]);
        b_stream.extend(data_frame(b"bbb"));
        let resp_b = mux.on_recv(b, &b_stream, true).unwrap().unwrap();
        assert_eq!(resp_b.status, 202);
        assert_eq!(resp_b.body, b"bbb");
        assert!(!mux.is_active(b));

        let resp_a = mux.on_recv(a, &data_frame(b"aaa"), true).unwrap().unwrap();
        assert_eq!(resp_a.status, 201);
        assert_eq!(resp_a.body, b"aaa");
        assert!(!mux.is_active(a));
        assert_eq!(mux.active_count(), 0);
    }

    #[test]
    fn bytes_for_unknown_stream_are_rejected() {
        let mut mux = RequestMux::new();
        // Nothing opened yet.
        assert_eq!(mux.on_recv(0, &[], true).unwrap_err(), MuxError::UnknownStream(0));
        // Open stream 0, complete it, then a late chunk finds it retired.
        let s = mux.open(&get(b"/")).unwrap().stream_id;
        mux.on_recv(s, &headers_frame(&[status(b"200")]), true)
            .unwrap()
            .unwrap();
        assert_eq!(mux.on_recv(s, &[], true).unwrap_err(), MuxError::UnknownStream(s));
    }

    #[test]
    fn failing_exchange_is_retired_and_reported() {
        let mut mux = RequestMux::new();
        let s = mux.open(&get(b"/")).unwrap().stream_id;
        // A HEADERS frame with no :status is a malformed message (RFC 9114 §4.3.2).
        let head = headers_frame(&[HeaderField::new(
            b"content-type".to_vec(),
            b"text/plain".to_vec(),
        )]);
        let err = mux.on_recv(s, &head, true).unwrap_err();
        assert!(matches!(
            err,
            MuxError::Exchange {
                stream_id,
                error: ExchangeError::Assemble(_),
            } if stream_id == s
        ));
        // The failed stream is retired; further bytes report UnknownStream.
        assert!(!mux.is_active(s));
        assert_eq!(mux.on_recv(s, &[], true).unwrap_err(), MuxError::UnknownStream(s));
    }

    #[test]
    fn bad_request_does_not_consume_a_stream_id() {
        let mut mux = RequestMux::new();
        // An uppercase header name is rejected at start (RFC 9114 §4.2).
        let bad = ClientRequest {
            headers: &[(b"Accept", b"x")],
            ..get(b"/")
        };
        let err = mux.open(&bad).unwrap_err();
        assert_eq!(
            err,
            OpenError::Request(ExchangeError::Request(MessageError::UppercaseName(
                b"Accept".to_vec()
            )))
        );
        assert_eq!(mux.active_count(), 0);
        // The next good request still gets stream 0 — the failed open burned no id.
        let opened = mux.open(&get(b"/")).unwrap();
        assert_eq!(opened.stream_id, 0);
    }

    #[test]
    fn interim_response_keeps_stream_active() {
        let mut mux = RequestMux::new();
        let s = mux.open(&get(b"/")).unwrap().stream_id;
        // A 100-continue head without FIN: response not complete, stream stays.
        assert_eq!(mux.on_recv(s, &headers_frame(&[status(b"100")]), false).unwrap(), None);
        assert!(mux.is_active(s));
        let mut rest = headers_frame(&[status(b"200")]);
        rest.extend(data_frame(b"ok"));
        let resp = mux.on_recv(s, &rest, true).unwrap().unwrap();
        assert_eq!(resp.informational, vec![100]);
        assert_eq!(resp.status, 200);
        assert!(!mux.is_active(s));
    }
}
