//! HTTP/3 client response assembler (RFC 9114 §4.1, §7.1).
//!
//! This is the composition slice that turns a request/response *stream* of bytes
//! into a complete HTTP response — the piece that joins the three lower slices
//! that were deliberately left apart:
//!
//! - [`frame`](super::frame) parses one HTTP/3 frame off a byte buffer, reporting
//!   `Ok(None)` while a frame is only partially buffered — the incremental codec.
//! - [`h3_stream::RequestStream`](super::h3_stream::RequestStream) is the frame
//!   *grammar* sequencer: it decides whether the order of frames on a
//!   request/response stream is legal (header section → body → optional trailer
//!   section, RFC 9114 §4.1) and, if not, which RFC 9114 §8.1 error to close with.
//! - [`h3_request`](super::h3_request) translates a `HEADERS` frame's QPACK field
//!   block into an [`H3ResponseHead`](super::h3_request::H3ResponseHead) (or, for
//!   the trailer section, an ordinary field list).
//!
//! [`ResponseAssembler`] owns one client-side request/response stream. The caller
//! feeds it the stream bytes as QUIC delivers them ([`ResponseAssembler::push_bytes`])
//! and, once the peer signals the stream FIN, drains the finished response
//! ([`ResponseAssembler::finish`]). It:
//!
//! - parses each complete frame off the growing buffer, keeping the trailing
//!   partial frame for the next chunk (RFC 9114 §7.1: a frame may span QUIC STREAM
//!   frames);
//! - runs every frame through the [`RequestStream`](super::h3_stream::RequestStream)
//!   grammar and surfaces a violation as the RFC 9114 §8.1 error the transport
//!   must reset the stream with;
//! - classifies each `HEADERS` frame by the grammar phase it arrives in — a
//!   response header section (interim `1xx` or the one final head) before the body,
//!   or the trailer section after it (RFC 9114 §4.1) — and decodes it accordingly;
//! - accumulates the `DATA` frame payloads into the response body.
//!
//! Like every slice below it, it is a pure state machine — no IO, no timers, no
//! packet protection. The QUIC transport that actually carries the bytes and
//! reports the stream FIN, and the `h3_do_request` dispatch that drives this
//! assembler against a real connection, remain later slices.

use super::frame::{Frame, FrameError};
use super::h3_request::{self, H3ResponseHead, MessageError};
use super::h3_stream::{RequestState, RequestStream, StreamLayerError};

/// A fully assembled HTTP/3 response: the final `:status`, its header fields, the
/// body, any interim (`1xx`) responses that preceded it, and any trailer section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct H3Response {
    /// The final response `:status` code (RFC 9114 §4.3.2), `200`–`599` (an
    /// interim `1xx` code never lands here — those are collected in
    /// [`H3Response::informational`]).
    pub status: u16,
    /// The final response's ordinary header fields, lower-case names, in received
    /// order (pseudo-headers stripped).
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    /// The response body: the concatenated payloads of every `DATA` frame.
    pub body: Vec<u8>,
    /// The `:status` codes of the interim (`1xx`) informational responses that
    /// preceded the final response, in order (RFC 9114 §4.1).
    pub informational: Vec<u16>,
    /// The trailer section's fields, if the response carried one (RFC 9114 §4.1).
    pub trailers: Vec<(Vec<u8>, Vec<u8>)>,
}

/// An error assembling a response from a request/response stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssembleError {
    /// A frame appeared where the RFC 9114 §4.1 message grammar forbids it; the
    /// wrapped error names the RFC 9114 §8.1 code the stream must be reset with.
    Stream(StreamLayerError),
    /// A frame failed to decode off the stream (RFC 9114 §7.1).
    Frame(FrameError),
    /// A `HEADERS` frame's field section was a malformed HTTP message (RFC 9114
    /// §4.1.2) or trailer section (§4.1).
    Message(MessageError),
    /// A second response header section arrived after the final response head — an
    /// interim (`1xx`) or a duplicate final head where only the body or the
    /// trailer section may follow (RFC 9114 §4.1).
    UnexpectedResponseHeaders,
    /// The stream ended without a final (non-`1xx`) response header section
    /// (RFC 9114 §4.1: a response must carry one).
    NoFinalResponse,
    /// The stream ended mid-frame — trailing bytes did not form a complete frame
    /// (RFC 9114 §7.1: a truncated frame is a malformed stream).
    IncompleteFrame,
    /// Bytes were pushed after the stream was already finished.
    AfterFin,
}

impl core::fmt::Display for AssembleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Stream(e) => write!(f, "stream grammar: {e}"),
            Self::Frame(e) => write!(f, "frame decode: {e}"),
            Self::Message(e) => write!(f, "message: {e}"),
            Self::UnexpectedResponseHeaders => {
                write!(f, "response header section after the final response head")
            }
            Self::NoFinalResponse => {
                write!(f, "stream ended with no final response header section")
            }
            Self::IncompleteFrame => write!(f, "stream ended mid-frame"),
            Self::AfterFin => write!(f, "bytes pushed after stream FIN"),
        }
    }
}

impl std::error::Error for AssembleError {}

/// Assembles a single client-side HTTP/3 request/response stream (RFC 9114 §4.1)
/// into an [`H3Response`], feeding on the stream bytes as the QUIC transport
/// delivers them.
#[derive(Clone, Debug)]
pub struct ResponseAssembler {
    /// The RFC 9114 §4.1 frame-grammar sequencer.
    seq: RequestStream,
    /// Buffered stream bytes not yet consumed as a complete frame.
    buf: Vec<u8>,
    /// The final (non-`1xx`) response head, once its `HEADERS` frame arrives.
    head: Option<H3ResponseHead>,
    /// The `:status` codes of interim (`1xx`) responses seen before the final head.
    informational: Vec<u16>,
    /// The accumulated `DATA` payloads.
    body: Vec<u8>,
    /// The trailer section's fields, if any.
    trailers: Vec<(Vec<u8>, Vec<u8>)>,
    /// Set once [`finish`](Self::finish) has consumed the assembler.
    finished: bool,
}

impl Default for ResponseAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseAssembler {
    /// A fresh assembler for a request/response stream awaiting its first frame.
    #[must_use]
    pub fn new() -> Self {
        Self {
            seq: RequestStream::new(),
            buf: Vec::new(),
            head: None,
            informational: Vec::new(),
            body: Vec::new(),
            trailers: Vec::new(),
            finished: false,
        }
    }

    /// Feed the next chunk of request/response-stream bytes, processing every
    /// complete frame it completes and keeping any trailing partial frame.
    ///
    /// # Errors
    ///
    /// [`AssembleError`] if a frame fails to decode, violates the RFC 9114 §4.1
    /// message grammar, or carries a malformed HTTP message.
    pub fn push_bytes(&mut self, data: &[u8]) -> Result<(), AssembleError> {
        if self.finished {
            return Err(AssembleError::AfterFin);
        }
        self.buf.extend_from_slice(data);
        let mut consumed_total = 0;
        while let Some((frame, consumed)) =
            Frame::parse(&self.buf[consumed_total..]).map_err(AssembleError::Frame)?
        {
            consumed_total += consumed;
            self.process_frame(frame)?;
        }
        if consumed_total > 0 {
            self.buf.drain(..consumed_total);
        }
        Ok(())
    }

    /// Run one decoded frame through the grammar and fold its contents into the
    /// response.
    fn process_frame(&mut self, frame: Frame) -> Result<(), AssembleError> {
        // The grammar phase *before* accepting classifies a HEADERS frame: a
        // header section (Init/Headers) versus the trailer section (Data).
        let phase = self.seq.state();
        self.seq.accept(&frame).map_err(AssembleError::Stream)?;

        match frame {
            Frame::Headers(block) => {
                // A HEADERS accepted from the Data phase is the trailer section;
                // otherwise it is a response header section (the Trailers phase is
                // impossible here — `accept` above would have rejected it).
                if matches!(phase, RequestState::Data) {
                    self.trailers =
                        h3_request::decode_trailers(&block).map_err(AssembleError::Message)?;
                } else {
                    if self.head.is_some() {
                        return Err(AssembleError::UnexpectedResponseHeaders);
                    }
                    let head =
                        h3_request::decode_response(&block).map_err(AssembleError::Message)?;
                    // Interim 1xx responses precede the final head (RFC 9114 §4.1).
                    if head.status < 200 {
                        self.informational.push(head.status);
                    } else {
                        self.head = Some(head);
                    }
                }
            }
            Frame::Data(mut payload) => self.body.append(&mut payload),
            // PUSH_PROMISE and reserved/greased frames carry nothing for the
            // response head or body; the grammar has already validated their
            // ordering.
            _ => {}
        }
        Ok(())
    }

    /// Whether a final (non-`1xx`) response header section has been received.
    #[must_use]
    pub const fn has_final_response(&self) -> bool {
        self.head.is_some()
    }

    /// Consume the assembler at the stream FIN and return the finished response.
    ///
    /// # Errors
    ///
    /// - [`AssembleError::IncompleteFrame`] if the stream ended mid-frame.
    /// - [`AssembleError::NoFinalResponse`] if no final response head arrived.
    pub fn finish(self) -> Result<H3Response, AssembleError> {
        if !self.buf.is_empty() {
            return Err(AssembleError::IncompleteFrame);
        }
        let head = self.head.ok_or(AssembleError::NoFinalResponse)?;
        Ok(H3Response {
            status: head.status,
            headers: head.headers,
            body: self.body,
            informational: self.informational,
            trailers: self.trailers,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::h3::qpack::{self, HeaderField};

    /// Encode a `HEADERS` frame carrying the given fields.
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

    fn status_fields(code: &[u8]) -> Vec<HeaderField> {
        vec![HeaderField::new(b":status".to_vec(), code.to_vec())]
    }

    #[test]
    fn assemble_headers_and_body() {
        let mut a = ResponseAssembler::new();
        let mut stream = headers_frame(&[
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b"content-type".to_vec(), b"text/plain".to_vec()),
        ]);
        stream.extend(data_frame(b"hello "));
        stream.extend(data_frame(b"world"));
        a.push_bytes(&stream).unwrap();
        assert!(a.has_final_response());
        let resp = a.finish().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(
            resp.headers,
            vec![(b"content-type".to_vec(), b"text/plain".to_vec())]
        );
        assert_eq!(resp.body, b"hello world");
        assert!(resp.informational.is_empty());
        assert!(resp.trailers.is_empty());
    }

    #[test]
    fn assemble_across_partial_chunks() {
        // Feed the stream one byte at a time: the incremental frame codec must
        // reassemble it identically (RFC 9114 §7.1 — a frame may span deliveries).
        let mut a = ResponseAssembler::new();
        let mut stream = headers_frame(&status_fields(b"204"));
        stream.extend(data_frame(b"abc"));
        for byte in &stream {
            a.push_bytes(&[*byte]).unwrap();
        }
        let resp = a.finish().unwrap();
        assert_eq!(resp.status, 204);
        assert_eq!(resp.body, b"abc");
    }

    #[test]
    fn header_only_response_has_empty_body() {
        let mut a = ResponseAssembler::new();
        a.push_bytes(&headers_frame(&status_fields(b"304"))).unwrap();
        let resp = a.finish().unwrap();
        assert_eq!(resp.status, 304);
        assert!(resp.body.is_empty());
    }

    #[test]
    fn interim_responses_collected_before_final() {
        let mut a = ResponseAssembler::new();
        let mut stream = headers_frame(&status_fields(b"100"));
        stream.extend(headers_frame(&status_fields(b"103")));
        stream.extend(headers_frame(&[
            HeaderField::new(b":status".to_vec(), b"200".to_vec()),
            HeaderField::new(b"x-final".to_vec(), b"1".to_vec()),
        ]));
        stream.extend(data_frame(b"body"));
        a.push_bytes(&stream).unwrap();
        let resp = a.finish().unwrap();
        assert_eq!(resp.informational, vec![100, 103]);
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"body");
    }

    #[test]
    fn trailers_after_body() {
        let mut a = ResponseAssembler::new();
        let mut stream = headers_frame(&status_fields(b"200"));
        stream.extend(data_frame(b"payload"));
        stream.extend(headers_frame(&[HeaderField::new(
            b"x-checksum".to_vec(),
            b"deadbeef".to_vec(),
        )]));
        a.push_bytes(&stream).unwrap();
        let resp = a.finish().unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"payload");
        assert_eq!(
            resp.trailers,
            vec![(b"x-checksum".to_vec(), b"deadbeef".to_vec())]
        );
    }

    #[test]
    fn data_before_headers_is_grammar_error() {
        let mut a = ResponseAssembler::new();
        let err = a.push_bytes(&data_frame(b"x")).unwrap_err();
        assert!(matches!(err, AssembleError::Stream(_)));
    }

    #[test]
    fn malformed_head_missing_status_is_message_error() {
        let mut a = ResponseAssembler::new();
        let frame = headers_frame(&[HeaderField::new(
            b"content-type".to_vec(),
            b"text/plain".to_vec(),
        )]);
        let err = a.push_bytes(&frame).unwrap_err();
        assert_eq!(err, AssembleError::Message(MessageError::MissingStatus));
    }

    #[test]
    fn second_final_head_before_body_is_rejected() {
        let mut a = ResponseAssembler::new();
        let mut stream = headers_frame(&status_fields(b"200"));
        stream.extend(headers_frame(&status_fields(b"200")));
        let err = a.push_bytes(&stream).unwrap_err();
        assert_eq!(err, AssembleError::UnexpectedResponseHeaders);
    }

    #[test]
    fn trailer_with_pseudo_header_is_rejected() {
        let mut a = ResponseAssembler::new();
        let mut stream = headers_frame(&status_fields(b"200"));
        stream.extend(data_frame(b"x"));
        stream.extend(headers_frame(&status_fields(b"200"))); // pseudo in trailers
        let err = a.push_bytes(&stream).unwrap_err();
        assert_eq!(
            err,
            AssembleError::Message(MessageError::PseudoInTrailer(b":status".to_vec()))
        );
    }

    #[test]
    fn finish_without_final_head_is_error() {
        // Only an interim response arrived, then FIN.
        let mut a = ResponseAssembler::new();
        a.push_bytes(&headers_frame(&status_fields(b"100"))).unwrap();
        assert!(!a.has_final_response());
        assert_eq!(a.finish().unwrap_err(), AssembleError::NoFinalResponse);
    }

    #[test]
    fn finish_mid_frame_is_incomplete() {
        let mut a = ResponseAssembler::new();
        let frame = headers_frame(&status_fields(b"200"));
        // Feed all but the last byte, then FIN.
        a.push_bytes(&frame[..frame.len() - 1]).unwrap();
        assert_eq!(a.finish().unwrap_err(), AssembleError::IncompleteFrame);
    }

    #[test]
    fn push_after_finish_paths_are_independent() {
        // A fresh assembler after a completed one shares no state.
        let mut a = ResponseAssembler::new();
        a.push_bytes(&headers_frame(&status_fields(b"200"))).unwrap();
        let first = a.finish().unwrap();
        assert_eq!(first.status, 200);

        let mut b = ResponseAssembler::new();
        b.push_bytes(&headers_frame(&status_fields(b"404"))).unwrap();
        assert_eq!(b.finish().unwrap().status, 404);
    }
}
