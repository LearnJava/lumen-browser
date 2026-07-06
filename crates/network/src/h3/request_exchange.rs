//! HTTP/3 client request exchange (RFC 9114 §4.1, §6.1).
//!
//! This is the composition slice above [`h3_request`](super::h3_request) (which
//! only builds the request `HEADERS` frame and decodes a response head) and
//! [`h3_exchange::ResponseAssembler`](super::h3_exchange::ResponseAssembler)
//! (which only reassembles the response stream): it joins the two halves of a
//! single client request/response exchange into one lifecycle automaton.
//!
//! A client request over HTTP/3 occupies one client-initiated *bidirectional*
//! stream (RFC 9114 §6.1): the client writes the request message — a `HEADERS`
//! frame carrying the request field section, then zero or more `DATA` frames
//! carrying the body — and closes its send half (STREAM FIN); the server writes
//! the response message back on the same stream. [`ClientExchange`] models the
//! client side of that: [`ClientExchange::start`] renders the request-stream
//! bytes to send (the whole request is emitted at once, so the caller sends them
//! with FIN set), and [`ClientExchange::on_recv`] feeds the response-stream bytes
//! back through an internal [`ResponseAssembler`] until the server's STREAM FIN,
//! at which point it yields the finished [`H3Response`].
//!
//! Like every slice below it, it is a pure state machine — no IO, no timers, no
//! stream identifiers, no QUIC. The transport that opens the bidirectional
//! stream, writes the request bytes, and reports received bytes and the peer FIN
//! — the `h3_do_request` dispatch — is the next slice; this automaton is what it
//! drives.

use super::frame::{Frame, FrameError};
use super::h3_exchange::{AssembleError, H3Response, ResponseAssembler};
use super::h3_request::{self, H3Profile, MessageError};

/// A client request to render onto an HTTP/3 request stream (RFC 9114 §4.1).
///
/// The four request pseudo-headers are supplied individually (they are ordered by
/// `profile` per the impersonation fingerprint, RFC 9114 §4.3.1); `headers` are
/// the ordinary request fields (lower-case names, no pseudo- or connection-
/// specific fields — validated per RFC 9114 §4.2); `body` is the optional request
/// body, framed into a single `DATA` frame when non-empty.
#[derive(Clone, Copy, Debug)]
pub struct ClientRequest<'a> {
    /// The impersonation profile selecting the request pseudo-header order.
    pub profile: H3Profile,
    /// The request method (`:method`), e.g. `GET` or `POST`.
    pub method: &'a [u8],
    /// The request scheme (`:scheme`), e.g. `https`.
    pub scheme: &'a [u8],
    /// The request authority (`:authority`), i.e. the host (and optional port).
    pub authority: &'a [u8],
    /// The request target (`:path`), e.g. `/index.html`.
    pub path: &'a [u8],
    /// The ordinary request header fields as `(name, value)` byte slices.
    pub headers: &'a [(&'a [u8], &'a [u8])],
    /// The request body; empty means no body (no `DATA` frame is emitted).
    pub body: &'a [u8],
    /// Whether to Huffman-code the QPACK literals when it does not enlarge them.
    pub use_huffman: bool,
}

/// The lifecycle state of a [`ClientExchange`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExchangeState {
    /// The request has been rendered (send half closed by the caller's FIN) and
    /// the response is being received.
    Receiving,
    /// The response has been fully assembled (the server's STREAM FIN was seen).
    Complete,
    /// The exchange failed — a malformed response or an RFC 9114 §4.1 grammar
    /// violation on the response stream. The stream must be reset.
    Failed,
}

/// An error rendering a request or assembling its response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExchangeError {
    /// Building the request field section / `HEADERS` frame failed — an
    /// `headers` entry violated RFC 9114 §4.2, or the frame failed to encode.
    Request(MessageError),
    /// Encoding the request body `DATA` frame failed (RFC 9114 §7.2.1).
    Body(FrameError),
    /// Assembling the response stream failed: a frame decode error, an RFC 9114
    /// §4.1 grammar violation, or a malformed response message.
    Assemble(AssembleError),
    /// Bytes were delivered after the exchange already finished — its response
    /// was assembled ([`ExchangeState::Complete`]) or it failed
    /// ([`ExchangeState::Failed`]); the automaton accepts no further input.
    NotReceiving,
}

impl core::fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Request(e) => write!(f, "request build: {e}"),
            Self::Body(e) => write!(f, "request body frame: {e}"),
            Self::Assemble(e) => write!(f, "response assembly: {e}"),
            Self::NotReceiving => write!(f, "bytes delivered after the exchange finished"),
        }
    }
}

impl std::error::Error for ExchangeError {}

/// The client side of one HTTP/3 request/response exchange (RFC 9114 §4.1, §6.1):
/// renders the request bytes at [`start`](Self::start), then folds the response
/// stream bytes fed to [`on_recv`](Self::on_recv) into an [`H3Response`].
#[derive(Clone, Debug)]
pub struct ClientExchange {
    /// Reassembles the response request/response stream. Drained on the recv FIN;
    /// left as a fresh (unused) assembler afterwards.
    assembler: ResponseAssembler,
    /// The lifecycle state.
    state: ExchangeState,
}

impl ClientExchange {
    /// Render the request onto request-stream bytes and return a receiving
    /// exchange awaiting the response.
    ///
    /// The returned `Vec<u8>` is the complete request message — the `HEADERS`
    /// frame followed by a single `DATA` frame when `req.body` is non-empty. The
    /// caller writes these bytes to the client-initiated bidirectional stream and
    /// closes the send half (STREAM FIN); no request trailers are emitted.
    ///
    /// # Errors
    ///
    /// - [`ExchangeError::Request`] if a request header violates RFC 9114 §4.2 or
    ///   the `HEADERS` frame fails to encode.
    /// - [`ExchangeError::Body`] if the body `DATA` frame fails to encode.
    pub fn start(req: &ClientRequest) -> Result<(Self, Vec<u8>), ExchangeError> {
        let mut bytes = h3_request::encode_request(
            req.profile,
            req.method,
            req.scheme,
            req.authority,
            req.path,
            req.headers,
            req.use_huffman,
        )
        .map_err(ExchangeError::Request)?;
        if !req.body.is_empty() {
            Frame::Data(req.body.to_vec())
                .encode(&mut bytes)
                .map_err(ExchangeError::Body)?;
        }
        let exchange = Self {
            assembler: ResponseAssembler::new(),
            state: ExchangeState::Receiving,
        };
        Ok((exchange, bytes))
    }

    /// The current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> ExchangeState {
        self.state
    }

    /// Feed the next chunk of response-stream bytes.
    ///
    /// `data` is the bytes the QUIC transport delivered on the request stream
    /// (possibly empty); `fin` marks the server's STREAM FIN — the end of the
    /// response. Returns `Ok(Some(response))` once the response is complete (only
    /// on the call that carries `fin`), and `Ok(None)` while more is expected.
    ///
    /// After a call returns the response or an error the exchange is terminal;
    /// any further call returns [`ExchangeError::NotReceiving`].
    ///
    /// # Errors
    ///
    /// - [`ExchangeError::Assemble`] if the response stream is malformed — a frame
    ///   decode error, an RFC 9114 §4.1 grammar violation, a malformed message, or
    ///   (on `fin`) a stream that ended mid-frame or without a final response head.
    /// - [`ExchangeError::NotReceiving`] if the exchange has already finished.
    pub fn on_recv(
        &mut self,
        data: &[u8],
        fin: bool,
    ) -> Result<Option<H3Response>, ExchangeError> {
        if !matches!(self.state, ExchangeState::Receiving) {
            return Err(ExchangeError::NotReceiving);
        }
        if let Err(e) = self.assembler.push_bytes(data) {
            self.state = ExchangeState::Failed;
            return Err(ExchangeError::Assemble(e));
        }
        if !fin {
            return Ok(None);
        }
        // The recv FIN drains the assembler; leaving a fresh one keeps the field
        // valid without an `Option` (the automaton is terminal past this point).
        let assembler = core::mem::take(&mut self.assembler);
        match assembler.finish() {
            Ok(response) => {
                self.state = ExchangeState::Complete;
                Ok(Some(response))
            }
            Err(e) => {
                self.state = ExchangeState::Failed;
                Err(ExchangeError::Assemble(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::frame::Frame;
    use crate::h3::h3_request::MessageError;
    use crate::h3::qpack::{self, HeaderField};

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

    fn status(code: &[u8]) -> HeaderField {
        HeaderField::new(b":status".to_vec(), code.to_vec())
    }

    #[test]
    fn get_request_bytes_are_a_headers_frame() {
        let (exchange, bytes) = ClientExchange::start(&get(b"/index.html")).unwrap();
        assert_eq!(exchange.state(), ExchangeState::Receiving);
        // A GET carries no body, so the request is exactly one HEADERS frame.
        let (frame, consumed) = Frame::parse(&bytes).unwrap().unwrap();
        assert_eq!(consumed, bytes.len());
        let Frame::Headers(block) = frame else {
            panic!("expected HEADERS frame");
        };
        let fields = qpack::decode_field_section(&block).unwrap();
        let path = fields.iter().find(|f| f.name == b":path").unwrap();
        assert_eq!(path.value, b"/index.html");
    }

    #[test]
    fn post_request_appends_a_body_data_frame() {
        let req = ClientRequest {
            method: b"POST",
            path: b"/submit",
            headers: &[(b"content-type", b"text/plain")],
            body: b"hello body",
            ..get(b"/submit")
        };
        let (_, bytes) = ClientExchange::start(&req).unwrap();
        // First frame: the request HEADERS. Second frame: the body DATA.
        let (first, n1) = Frame::parse(&bytes).unwrap().unwrap();
        assert!(matches!(first, Frame::Headers(_)));
        let (second, n2) = Frame::parse(&bytes[n1..]).unwrap().unwrap();
        assert_eq!(n1 + n2, bytes.len());
        assert_eq!(second, Frame::Data(b"hello body".to_vec()));
    }

    #[test]
    fn full_response_assembled_on_fin() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        let mut stream = headers_frame(&[
            status(b"200"),
            HeaderField::new(b"content-type".to_vec(), b"text/plain".to_vec()),
        ]);
        stream.extend(data_frame(b"hello "));
        stream.extend(data_frame(b"world"));
        let resp = exchange.on_recv(&stream, true).unwrap().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello world");
        assert_eq!(
            resp.headers,
            vec![(b"content-type".to_vec(), b"text/plain".to_vec())]
        );
        assert_eq!(exchange.state(), ExchangeState::Complete);
    }

    #[test]
    fn response_without_fin_yields_nothing_yet() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        let head = headers_frame(&[status(b"200")]);
        assert_eq!(exchange.on_recv(&head, false).unwrap(), None);
        assert_eq!(exchange.state(), ExchangeState::Receiving);
        // The FIN can arrive on a later, empty chunk.
        let resp = exchange.on_recv(&[], true).unwrap().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(exchange.state(), ExchangeState::Complete);
    }

    #[test]
    fn response_across_single_byte_chunks() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        let mut stream = headers_frame(&[status(b"204")]);
        stream.extend(data_frame(b"abc"));
        let last = stream.len() - 1;
        for (i, byte) in stream.iter().enumerate() {
            let out = exchange.on_recv(&[*byte], i == last).unwrap();
            if i == last {
                assert_eq!(out.unwrap().body, b"abc");
            } else {
                assert_eq!(out, None);
            }
        }
    }

    #[test]
    fn interim_responses_precede_final() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        let mut stream = headers_frame(&[status(b"100")]);
        stream.extend(headers_frame(&[status(b"103")]));
        stream.extend(headers_frame(&[status(b"200")]));
        stream.extend(data_frame(b"body"));
        let resp = exchange.on_recv(&stream, true).unwrap().unwrap();
        assert_eq!(resp.informational, vec![100, 103]);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"body");
    }

    #[test]
    fn trailers_delivered_with_response() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        let mut stream = headers_frame(&[status(b"200")]);
        stream.extend(data_frame(b"payload"));
        stream.extend(headers_frame(&[HeaderField::new(
            b"x-checksum".to_vec(),
            b"deadbeef".to_vec(),
        )]));
        let resp = exchange.on_recv(&stream, true).unwrap().unwrap();
        assert_eq!(
            resp.trailers,
            vec![(b"x-checksum".to_vec(), b"deadbeef".to_vec())]
        );
    }

    #[test]
    fn bad_request_header_rejected_at_start() {
        let req = ClientRequest {
            headers: &[(b"Accept", b"x")],
            ..get(b"/")
        };
        let err = ClientExchange::start(&req).unwrap_err();
        assert_eq!(
            err,
            ExchangeError::Request(MessageError::UppercaseName(b"Accept".to_vec()))
        );
    }

    #[test]
    fn malformed_response_fails_and_is_terminal() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        // A HEADERS frame with no :status is a malformed message (RFC 9114 §4.3.2).
        let head = headers_frame(&[HeaderField::new(
            b"content-type".to_vec(),
            b"text/plain".to_vec(),
        )]);
        let err = exchange.on_recv(&head, true).unwrap_err();
        assert_eq!(
            err,
            ExchangeError::Assemble(AssembleError::Message(MessageError::MissingStatus))
        );
        assert_eq!(exchange.state(), ExchangeState::Failed);
        // Terminal: further bytes are rejected without touching the assembler.
        assert_eq!(
            exchange.on_recv(&[], true).unwrap_err(),
            ExchangeError::NotReceiving
        );
    }

    #[test]
    fn grammar_violation_fails() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        // DATA before any HEADERS violates the request-stream grammar (§4.1).
        let err = exchange.on_recv(&data_frame(b"x"), false).unwrap_err();
        assert!(matches!(
            err,
            ExchangeError::Assemble(AssembleError::Stream(_))
        ));
        assert_eq!(exchange.state(), ExchangeState::Failed);
    }

    #[test]
    fn recv_after_complete_is_rejected() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        exchange
            .on_recv(&headers_frame(&[status(b"200")]), true)
            .unwrap()
            .unwrap();
        assert_eq!(exchange.state(), ExchangeState::Complete);
        assert_eq!(
            exchange.on_recv(&headers_frame(&[status(b"200")]), true)
                .unwrap_err(),
            ExchangeError::NotReceiving
        );
    }

    #[test]
    fn fin_mid_frame_is_incomplete() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        let head = headers_frame(&[status(b"200")]);
        // All but the last byte, then FIN: the trailing frame is truncated.
        let err = exchange.on_recv(&head[..head.len() - 1], true).unwrap_err();
        assert_eq!(
            err,
            ExchangeError::Assemble(AssembleError::IncompleteFrame)
        );
        assert_eq!(exchange.state(), ExchangeState::Failed);
    }

    #[test]
    fn fin_with_only_interim_response_has_no_final() {
        let (mut exchange, _) = ClientExchange::start(&get(b"/")).unwrap();
        let err = exchange
            .on_recv(&headers_frame(&[status(b"100")]), true)
            .unwrap_err();
        assert_eq!(
            err,
            ExchangeError::Assemble(AssembleError::NoFinalResponse)
        );
    }
}
