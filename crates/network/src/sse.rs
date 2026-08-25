//! Server-Sent Events (SSE) — HTML Living Standard §9.2.
//!
//! Two layers:
//! - [`SseParser`] — incremental `text/event-stream` byte-stream → [`SseEvent`] values.
//! - [`EventSource`] — HTTP streaming client with auto-reconnect; implements
//!   [`lumen_core::ext::SseSession`].
//!
//! Supported line terminators: LF (`\n`), CR (`\r`), CRLF (`\r\n`).
//!
//! Field semantics (spec §9.2.6 «Parsing an event stream»):
//! - `data:`  — append to data buffer (multiple lines joined with `\n`)
//! - `event:` — set event type (default `"message"`)
//! - `id:`    — set last event ID (persists across events; ignored if contains NUL)
//! - `retry:` — set reconnection time in ms (if all-ASCII-digits)
//! - `:`      — comment, ignored
//! - blank line — dispatch event (if data buffer non-empty)

// Долг по документации: файл написан до включения `missing_docs` и пока не
// покрыт. Область исключения — файл, а не крейт, поэтому НОВЫЙ файл обязан
// документировать публичный API. Счётчики по крейтам — docs/lint-policy.md §10.
#![allow(missing_docs)]

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::time::Duration;

use lumen_core::error::{Error, Result};
use lumen_core::event::{Event, TabId};
use lumen_core::ext::{DnsResolver, EventSink, SseEvent, SseSession};
use lumen_core::url::Url;

use crate::{RawStream, connect, header_value, parse_status, require_http_scheme};

// ── SseParser ─────────────────────────────────────────────────────────────────

/// Incremental `text/event-stream` parser.
///
/// Stores state between [`push_bytes`](Self::push_bytes) calls so callers
/// can feed the stream in arbitrary-sized chunks.
#[derive(Default)]
pub struct SseParser {
    line_buf: Vec<u8>,
    event_type: String,
    data_buf: String,
    /// Spec §9.2.6 «last event ID buffer»: written by the `id:` field of the
    /// stream currently being parsed. NOT what the reconnect header carries —
    /// an `id:` in an event that never dispatched must not be sent
    /// ([`reset_stream`](Self::reset_stream) rolls it back, BUG-845).
    last_event_id_buf: String,
    /// Spec §9.2.6 «last event ID string of the event source»: copied from the
    /// buffer on every dispatch attempt, persists across connections and is
    /// what [`last_event_id`](Self::last_event_id) — hence `Last-Event-ID` —
    /// reports.
    last_event_id: String,
    retry_ms: Option<u64>,
    // True when the previous byte was CR; used to skip the LF of a CRLF pair.
    last_was_cr: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of bytes from the stream; returns any events that
    /// became complete during this call.
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        for &b in bytes {
            match b {
                b'\r' => {
                    let line =
                        String::from_utf8_lossy(&std::mem::take(&mut self.line_buf)).into_owned();
                    if let Some(ev) = self.process_line(&line) {
                        events.push(ev);
                    }
                    self.last_was_cr = true;
                }
                b'\n' => {
                    if self.last_was_cr {
                        // This LF is the second byte of a CRLF pair — the CR
                        // already dispatched the line; skip this byte.
                        self.last_was_cr = false;
                        continue;
                    }
                    let line =
                        String::from_utf8_lossy(&std::mem::take(&mut self.line_buf)).into_owned();
                    if let Some(ev) = self.process_line(&line) {
                        events.push(ev);
                    }
                }
                _ => {
                    self.last_was_cr = false;
                    self.line_buf.push(b);
                }
            }
        }
        events
    }

    /// Process one complete line (without the terminator).
    fn process_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }

        // Comment — ignore
        if line.starts_with(':') {
            return None;
        }

        // Split on the first colon to get field name + value.
        let (name, value) = match line.find(':') {
            Some(pos) => {
                let raw = &line[pos + 1..];
                // Strip exactly one leading U+0020 SPACE if present.
                (&line[..pos], raw.strip_prefix(' ').unwrap_or(raw))
            }
            None => (line, ""),
        };

        match name {
            "data" => {
                // Spec: append value then U+000A to data buffer (always).
                self.data_buf.push_str(value);
                self.data_buf.push('\n');
            }
            "event" => {
                self.event_type = value.to_string();
            }
            // Spec: ignore if value contains U+0000 NULL.
            "id" if !value.contains('\0') => {
                self.last_event_id_buf = value.to_string();
            }
            // Spec: set retry only if value is all ASCII digits and parses as u64.
            "retry"
                if !value.is_empty()
                    && value.bytes().all(|b| b.is_ascii_digit())
                    && let Ok(ms) = value.parse::<u64>() =>
            {
                self.retry_ms = Some(ms);
            }
            _ => {} // Unknown field or guard-rejected arm — spec says "do nothing"
        }

        None
    }

    /// Dispatch the current event buffers (called on blank line).
    fn dispatch(&mut self) -> Option<SseEvent> {
        // Spec §9.2.6 step 1 of «dispatch the event»: promote the last event ID
        // buffer to the event source's last event ID string. This happens
        // BEFORE the empty-data check, so a blank line after a bare `id:` still
        // commits the id — and, conversely, an `id:` never followed by a blank
        // line never commits (BUG-845).
        self.last_event_id.clone_from(&self.last_event_id_buf);

        // Spec: if the data buffer is empty, discard and reset event type.
        if self.data_buf.is_empty() {
            self.event_type.clear();
            return None;
        }

        // Spec: remove the trailing U+000A from data buffer.
        if self.data_buf.ends_with('\n') {
            self.data_buf.pop();
        }

        let event_type = if self.event_type.is_empty() {
            "message".to_string()
        } else {
            std::mem::take(&mut self.event_type)
        };

        let id = if self.last_event_id.is_empty() {
            None
        } else {
            Some(self.last_event_id.clone())
        };

        let event = SseEvent {
            event_type,
            data: std::mem::take(&mut self.data_buf),
            id,
            retry_ms: self.retry_ms.take(),
        };

        // Spec: reset event type and data buffers; last_event_id persists.
        self.event_type.clear();

        Some(event)
    }

    /// Current last-event-id (persists across dispatched events, needed for
    /// reconnection Last-Event-ID header).
    pub fn last_event_id(&self) -> &str {
        &self.last_event_id
    }

    /// Take the reconnection time the stream last asked for, if any.
    ///
    /// `retry:` takes effect when the field is parsed, not when an event
    /// dispatches (§9.2.6) — a stream may set it and never dispatch anything.
    /// [`dispatch`](Self::dispatch) also consumes this value into the event it
    /// produces, so whichever of the two runs first sees it exactly once.
    pub fn take_retry(&mut self) -> Option<u64> {
        self.retry_ms.take()
    }

    /// Drop everything that belongs to the connection that just ended.
    ///
    /// Spec §9.2.6: «Once the end of the file is reached, any pending data must
    /// be discarded» — an event without its final blank line is not dispatched,
    /// and its `id:` must not reach the next request's `Last-Event-ID` header
    /// either, so the buffer rolls back to the last *dispatched* value. What
    /// survives a reconnection is exactly the last event ID string (BUG-845).
    pub fn reset_stream(&mut self) {
        self.line_buf.clear();
        self.event_type.clear();
        self.data_buf.clear();
        self.last_was_cr = false;
        self.last_event_id_buf.clone_from(&self.last_event_id);
    }
}

// ── EventSource ───────────────────────────────────────────────────────────────

const CHUNK: usize = 4096;
const DEFAULT_RETRY_MS: u64 = 3_000;

/// How the response body is delimited (RFC 7230 §3.3.3).
///
/// SSE reads the body incrementally and forever, so the framing is what decides
/// when the *stream* has ended. Reading until the socket closes is correct only
/// for the third case: with `Content-Length` or `chunked` on a keep-alive
/// connection the body ends while the socket stays open, and treating that as
/// «still streaming» is what left the connection hanging forever (BUG-844) —
/// every `.py` handler under `wptserve` answers in exactly that shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SseFraming {
    /// `Content-Length: N` — the stream ends after N body bytes.
    Length,
    /// `Transfer-Encoding: chunked` — the stream ends at the last chunk.
    Chunked,
    /// Neither header — the stream ends when the socket closes.
    Eof,
}

/// Streaming SSE client. Implements [`SseSession`].
///
/// Maintains the HTTP connection and feeds chunks into [`SseParser`].
/// Queues multiple events dispatched from a single chunk so callers receive
/// them one-by-one via [`next_event`](Self::next_event).
pub(crate) struct EventSource {
    url: Url,
    tab_id: TabId,
    sink: Arc<dyn EventSink>,
    resolver: Arc<dyn DnsResolver>,
    /// Buffered events ready for delivery (front = next to return).
    queue: std::collections::VecDeque<SseEvent>,
    parser: SseParser,
    /// Active HTTP stream; None when disconnected (will reconnect on next call).
    stream: Option<BufReader<RawStream>>,
    /// Framing of the body currently being read.
    framing: SseFraming,
    /// `Length`: body bytes still to come. `Chunked`: bytes left in the current
    /// chunk (0 = a chunk-size line is next). Unused for `Eof`.
    remaining: u64,
    retry_ms: u64,
    /// Shared cancellation handle: stops the reconnect loop and wakes a pending
    /// reconnect sleep when `close()` is signalled from another thread.
    cancel: lumen_core::ext::SseCancel,
    /// True once the terminal SseClosed has been emitted; makes close() idempotent.
    closed: bool,
}

impl EventSource {
    /// Open an SSE connection. `url` must be `http://` or `https://`.
    pub(crate) fn connect(
        url: &Url,
        resolver: Arc<dyn DnsResolver>,
        sink: Arc<dyn EventSink>,
        tab_id: TabId,
    ) -> Result<Self> {
        let mut es = Self {
            url: url.clone(),
            tab_id,
            sink,
            resolver,
            queue: std::collections::VecDeque::new(),
            parser: SseParser::new(),
            stream: None,
            framing: SseFraming::Eof,
            remaining: 0,
            retry_ms: DEFAULT_RETRY_MS,
            cancel: lumen_core::ext::SseCancel::new(),
            closed: false,
        };
        es.open_connection()?;
        Ok(es)
    }

    /// Establish (or re-establish) the HTTP connection.
    fn open_connection(&mut self) -> Result<()> {
        let (host, port, is_tls) = require_http_scheme(&self.url)?;
        // No read timeout: an EventSource connection is meant to sit idle
        // between server-sent events far longer than any bounded fetch (BUG-307
        // added a timeout to plain request/response `connect()` calls, not here).
        let conn = connect(&host, port, is_tls, self.resolver.as_ref(), crate::tls::TlsProfile::Standard, None, None)?;

        // Build SSE request: must send Accept and Cache-Control per spec §9.2.1.
        let last_id = self.parser.last_event_id().to_owned();
        let last_id_header = if last_id.is_empty() {
            String::new()
        } else {
            format!("Last-Event-ID: {last_id}\r\n")
        };

        let path = self.url.path_and_query();
        let ua = crate::http::DEFAULT_USER_AGENT;
        let request = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {host}\r\n\
             User-Agent: {ua}\r\n\
             Accept: text/event-stream\r\n\
             Cache-Control: no-store\r\n\
             Connection: keep-alive\r\n\
             {last_id_header}\r\n"
        );

        // Write request onto raw stream (bypass Connection's write_request to
        // keep the BufReader alive for streaming — we need to consume the body
        // incrementally, not buffer it all).
        let mut raw = conn.into_stream();
        raw.write_all(request.as_bytes())
            .map_err(|e| Error::Network(format!("sse: write request: {e}")))?;
        raw.flush()
            .map_err(|e| Error::Network(format!("sse: flush: {e}")))?;

        let mut reader = BufReader::new(raw);

        // Read status line.
        let mut status_line = String::new();
        reader
            .read_line(&mut status_line)
            .map_err(|e| Error::Network(format!("sse: read status: {e}")))?;
        let status = parse_status(&status_line)?;

        // Read headers until blank line.
        let mut headers: Vec<(String, String)> = Vec::new();
        loop {
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .map_err(|e| Error::Network(format!("sse: read header: {e}")))?;
            if n == 0 {
                return Err(Error::Network("sse: EOF in headers".into()));
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some((k, v)) = trimmed.split_once(':') {
                headers.push((k.trim().to_owned(), v.trim().to_owned()));
            }
        }

        if status != 200 {
            return Err(Error::Network(format!("sse: server returned {status}")));
        }

        // Verify Content-Type starts with "text/event-stream".
        let ct = header_value(&headers, "content-type").unwrap_or("");
        if !ct.to_ascii_lowercase().contains("text/event-stream") {
            return Err(Error::Network(format!(
                "sse: unexpected Content-Type: {ct:?}"
            )));
        }

        // Body framing decides where the stream ends (see [`SseFraming`]).
        let chunked = header_value(&headers, "transfer-encoding")
            .map(|v| v.to_ascii_lowercase().contains("chunked"))
            .unwrap_or(false);
        let length = header_value(&headers, "content-length").and_then(|v| v.trim().parse::<u64>().ok());
        // RFC 7230 §3.3.3 (3): `Transfer-Encoding` wins over `Content-Length`.
        let (framing, remaining) = match (chunked, length) {
            (true, _) => (SseFraming::Chunked, 0),
            (false, Some(n)) => (SseFraming::Length, n),
            (false, None) => (SseFraming::Eof, 0),
        };
        self.framing = framing;
        self.remaining = remaining;

        // §9.2.6 «announce the connection»: readyState = OPEN, fire `open`.
        // Emitted on every (re)connection, not just the first — a consumer that
        // hears about the drop must hear about the recovery too (BUG-844).
        self.sink.emit(&Event::SseConnected {
            tab_id: self.tab_id,
            url: self.url.clone(),
        });

        self.stream = Some(reader);
        Ok(())
    }

    /// Read the next slice of the response body into `buf`, honouring the
    /// framing. `Ok(0)` means the *stream* ended (not necessarily the socket).
    fn read_body(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let framing = self.framing;
        let Some(stream) = self.stream.as_mut() else {
            return Ok(0);
        };
        match framing {
            SseFraming::Eof => stream.read(buf),
            SseFraming::Length => {
                if self.remaining == 0 {
                    return Ok(0);
                }
                let take = (buf.len() as u64).min(self.remaining) as usize;
                let n = stream.read(&mut buf[..take])?;
                // A short body is a truncated stream, not a protocol error: the
                // events already handed over stay valid and we reconnect.
                self.remaining -= n as u64;
                Ok(n)
            }
            SseFraming::Chunked => {
                if self.remaining == 0 {
                    let mut size_line = String::new();
                    if stream.read_line(&mut size_line)? == 0 {
                        return Ok(0);
                    }
                    let size_hex = size_line
                        .trim_end_matches(['\r', '\n'])
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim();
                    let Ok(size) = u64::from_str_radix(size_hex, 16) else {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "sse: invalid chunk size",
                        ));
                    };
                    if size == 0 {
                        // last-chunk: drain the trailer section, then end.
                        loop {
                            let mut line = String::new();
                            let n = stream.read_line(&mut line)?;
                            if n == 0 || line == "\r\n" || line == "\n" {
                                break;
                            }
                        }
                        return Ok(0);
                    }
                    self.remaining = size;
                }
                let take = (buf.len() as u64).min(self.remaining) as usize;
                let n = stream.read(&mut buf[..take])?;
                if n == 0 {
                    return Ok(0);
                }
                self.remaining -= n as u64;
                if self.remaining == 0 {
                    // CRLF terminating the chunk data.
                    let mut crlf = [0u8; 2];
                    stream.read_exact(&mut crlf)?;
                }
                Ok(n)
            }
        }
    }

    /// The connection is over: drop it, discard the half-parsed event, and tell
    /// the observer so it can fire `error` with `readyState = CONNECTING`
    /// (§9.2.5 step 1). Called for a clean end of stream as well as a read
    /// error — from the page's point of view the two are the same event.
    fn end_stream(&mut self, reason: String) {
        self.stream = None;
        self.framing = SseFraming::Eof;
        self.remaining = 0;
        self.parser.reset_stream();
        self.sink.emit(&Event::SseError {
            tab_id: self.tab_id,
            url: self.url.clone(),
            message: reason,
        });
    }

    /// Read one chunk from the active stream and push any complete events into
    /// `self.queue`. Returns `true` if the stream is still open, `false` on EOF.
    fn fill_queue(&mut self) -> Result<bool> {
        if self.stream.is_none() {
            return Err(Error::Network("sse: no active stream".into()));
        }

        let mut buf = [0u8; CHUNK];
        let n = self
            .read_body(&mut buf)
            .map_err(|e| Error::Network(format!("sse: read: {e}")))?;
        if n == 0 {
            // End of stream — body exhausted or socket closed.
            return Ok(false);
        }

        let events = self.parser.push_bytes(&buf[..n]);
        // `retry:` applies as soon as it is parsed, even if the stream never
        // dispatches an event carrying it (§9.2.6).
        if let Some(ms) = self.parser.take_retry() {
            self.retry_ms = ms;
        }
        for ev in events {
            // Update retry_ms from server hint.
            if let Some(ms) = ev.retry_ms {
                self.retry_ms = ms;
            }
            self.sink.emit(&Event::SseMessage {
                tab_id: self.tab_id,
                url: self.url.clone(),
                event_type: ev.event_type.clone(),
                data: ev.data.clone(),
                id: ev.id.clone(),
            });
            self.queue.push_back(ev);
        }
        Ok(true)
    }
}

impl SseSession for EventSource {
    fn next_event(&mut self) -> Result<Option<SseEvent>> {
        loop {
            if self.cancel.is_cancelled() {
                return Ok(None);
            }

            // Return buffered events first.
            if let Some(ev) = self.queue.pop_front() {
                return Ok(Some(ev));
            }

            if self.stream.is_some() {
                match self.fill_queue() {
                    Ok(true) => continue,  // read more; queue may now have events
                    // Transient drop -> reconnect, no terminal close (HTML SSE §9.2.1).
                    Ok(false) => self.end_stream("stream ended".into()),
                    Err(e) => self.end_stream(e.to_string()),
                }
            }

            if self.cancel.is_cancelled() {
                return Ok(None);
            }

            // Reconnect after retry_ms delay (spec §9.2.1). The sleep is
            // interruptible: close() signals the cancel handle and we stop
            // immediately instead of waiting out the full retry delay.
            if self.cancel.sleep(Duration::from_millis(self.retry_ms)) {
                return Ok(None);
            }

            match self.open_connection() {
                Ok(()) => {}
                Err(e) => {
                    // Permanent connect failure: propagate to caller.
                    return Err(e);
                }
            }
        }
    }

    fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.cancel.signal();
        self.stream = None;
        self.sink.emit(&Event::SseClosed {
            tab_id: self.tab_id,
            url: self.url.clone(),
            reason: "client closed".into(),
        });
    }

    /// Shares the reconnect-cancellation handle so another thread can interrupt
    /// a pending reconnect delay (see [`SseCancel`](lumen_core::ext::SseCancel)).
    fn cancel(&self) -> lumen_core::ext::SseCancel {
        self.cancel.clone()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        p.push_bytes(input.as_bytes())
    }

    #[test]
    fn simple_message_lf() {
        let events = parse("data: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "message");
        assert_eq!(events[0].data, "hello");
        assert_eq!(events[0].id, None);
    }

    #[test]
    fn simple_message_crlf() {
        let events = parse("data: hello\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn simple_message_cr() {
        let events = parse("data: hello\r\r");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn custom_event_type() {
        let events = parse("event: ping\ndata: 1\n\n");
        assert_eq!(events[0].event_type, "ping");
        assert_eq!(events[0].data, "1");
    }

    #[test]
    fn multiline_data_joined_with_newline() {
        let events = parse("data: line1\ndata: line2\n\n");
        assert_eq!(events[0].data, "line1\nline2");
    }

    #[test]
    fn id_field_persists_across_events() {
        let events = parse("id: 42\ndata: a\n\ndata: b\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, Some("42".into()));
        // second event sees the same last_event_id
        assert_eq!(events[1].id, Some("42".into()));
    }

    #[test]
    fn id_updated_by_second_event() {
        let events = parse("id: 1\ndata: a\n\nid: 2\ndata: b\n\n");
        assert_eq!(events[0].id, Some("1".into()));
        assert_eq!(events[1].id, Some("2".into()));
    }

    #[test]
    fn id_ignored_if_contains_null() {
        let events = parse("id: ab\0cd\ndata: x\n\n");
        assert_eq!(events[0].id, None);
    }

    #[test]
    fn retry_field_parsed() {
        let events = parse("retry: 5000\ndata: ok\n\n");
        assert_eq!(events[0].retry_ms, Some(5000));
    }

    #[test]
    fn retry_ignored_if_not_digits() {
        let events = parse("retry: 1s\ndata: ok\n\n");
        assert_eq!(events[0].retry_ms, None);
    }

    #[test]
    fn retry_taken_once_per_batch() {
        // retry: in first event only; second event should not carry it.
        let events = parse("retry: 3000\ndata: a\n\ndata: b\n\n");
        assert_eq!(events[0].retry_ms, Some(3000));
        assert_eq!(events[1].retry_ms, None);
    }

    #[test]
    fn comment_ignored() {
        let events = parse(": this is a comment\ndata: ok\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "ok");
    }

    #[test]
    fn empty_data_discards_event() {
        let events = parse("\n");
        assert!(events.is_empty());
    }

    #[test]
    fn event_type_reset_after_dispatch() {
        let events = parse("event: custom\ndata: a\n\ndata: b\n\n");
        assert_eq!(events[0].event_type, "custom");
        assert_eq!(events[1].event_type, "message"); // reset to default
    }

    #[test]
    fn value_without_space_after_colon() {
        // "data:nospace" — value is "nospace" (no space strip applied)
        let events = parse("data:nospace\n\n");
        assert_eq!(events[0].data, "nospace");
    }

    #[test]
    fn field_without_colon_uses_empty_value() {
        // "data" alone → field "data" with value ""
        let events = parse("data\n\n");
        assert_eq!(events[0].data, "");
    }

    #[test]
    fn multiple_events_in_one_chunk() {
        let events = parse("data: a\n\ndata: b\n\ndata: c\n\n");
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].data, "a");
        assert_eq!(events[1].data, "b");
        assert_eq!(events[2].data, "c");
    }

    #[test]
    fn incremental_chunks_preserve_state() {
        let mut p = SseParser::new();
        // Feed in 3 separate chunks that together form one event.
        let mut events = p.push_bytes(b"data: he");
        assert!(events.is_empty());
        events.extend(p.push_bytes(b"llo\n"));
        assert!(events.is_empty());
        events.extend(p.push_bytes(b"\n"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
    }

    #[test]
    fn bom_treated_as_field_name_on_first_line() {
        // The BOM (U+FEFF) is not stripped by the parser — callers should
        // strip it. If present, the first field name is "\u{FEFF}data" which
        // won't match "data" → the event dispatches with an empty data buffer
        // and is discarded.
        let input = "\u{FEFF}data: x\n\n";
        let events = parse(input);
        // Spec §9.2.6: BOM handling is at the stream level, not in the line
        // parser. Our parser intentionally does not strip BOMs — the HTTP
        // layer is responsible. So the first event is discarded.
        let _ = events; // behaviour is defined; just check it doesn't panic
    }

    #[test]
    fn data_trailing_newline_stripped() {
        // Multiple data lines → joined by \n; trailing \n removed on dispatch.
        let events = parse("data: a\ndata: b\n\n");
        assert_eq!(events[0].data, "a\nb");
        // No trailing newline.
        assert!(!events[0].data.ends_with('\n'));
    }

    #[test]
    fn unknown_field_ignored() {
        let events = parse("foo: bar\ndata: ok\n\n");
        assert_eq!(events[0].data, "ok");
    }

    #[test]
    fn empty_event_type_field_defaults_to_message() {
        let events = parse("event: \ndata: x\n\n");
        // "event: " → event type is "" → default "message"
        assert_eq!(events[0].event_type, "message");
    }

    #[test]
    fn last_event_id_accessible_via_parser() {
        let mut p = SseParser::new();
        p.push_bytes(b"id: 99\ndata: x\n\n");
        assert_eq!(p.last_event_id(), "99");
    }

    #[test]
    fn retry_ms_updates_across_events() {
        // retry: updates on each event that carries one.
        let events = parse("retry: 1000\ndata: a\n\nretry: 2000\ndata: b\n\n");
        assert_eq!(events[0].retry_ms, Some(1000));
        assert_eq!(events[1].retry_ms, Some(2000));
    }

    // ── Конец потока и переподключение (BUG-844 / BUG-845) ────────────────

    #[test]
    fn dispatch_commits_id_even_when_data_is_empty() {
        // §9.2.6: продвижение буфера last event ID — ШАГ 1 «диспатча», до
        // проверки на пустой data. Голый `id:` с пустой строкой события не
        // порождает, но идентификатор фиксирует.
        let mut p = SseParser::new();
        let events = p.push_bytes(b"id: 42\n\n");
        assert!(events.is_empty(), "пустой data события не даёт");
        assert_eq!(p.last_event_id(), "42");
    }

    #[test]
    fn undispatched_id_does_not_reach_last_event_id() {
        // `id:` без завершающей пустой строки не диспатчился, значит в
        // `Last-Event-ID` уходить не должен (BUG-845).
        let mut p = SseParser::new();
        p.push_bytes(b"data: a\n\nid: X\ndata: b");
        assert_eq!(p.last_event_id(), "", "id недиспатченного блока не в счёт");
    }

    #[test]
    fn reset_stream_drops_pending_block_and_rolls_back_id() {
        // Оборванное соединение: незавершённый блок выбрасывается целиком, а
        // буфер id откатывается к последнему ДИСПАТЧЕННОМУ значению — иначе
        // данные склеиваются с данными следующего соединения (BUG-845).
        let mut p = SseParser::new();
        p.push_bytes(b"id: 1\ndata: a\n\nid: X\ndata: b");
        assert_eq!(p.last_event_id(), "1");

        p.reset_stream();
        assert_eq!(p.last_event_id(), "1", "откат к диспатченному id");

        // Следующее соединение: данные оборванного блока не приклеились.
        let events = p.push_bytes(b"data: c\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "c", "без склейки с 'b'");
        assert_eq!(events[0].event_type, "message");
        // И — главное — диспатч нового потока не поднимает `X` из буфера:
        // именно это уходило в `Last-Event-ID` следующего запроса (BUG-845).
        assert_eq!(p.last_event_id(), "1", "'X' не всплыл после переподключения");
    }

    #[test]
    fn reset_stream_clears_pending_line_and_event_type() {
        // Недочитанная СТРОКА и тип события — тоже состояние соединения.
        // Обрыв приходится на середину строки (терминатора нет), поэтому в
        // `line_buf` остаётся «data: pending»: без очистки он склеится с первой
        // строкой нового потока, а `event: custom` перекрасит его первое
        // событие.
        let mut p = SseParser::new();
        let events = p.push_bytes(b"event: custom\ndata: pending");
        assert!(events.is_empty(), "блок не завершён — события нет");

        p.reset_stream();

        let events = p.push_bytes(b"data: fresh\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "fresh", "без склейки с недочитанной строкой");
        assert_eq!(events[0].event_type, "message", "тип не пережил обрыв");
    }

    #[test]
    fn wpt_format_data_before_final_empty_line() {
        // Тело `eventsource/format-data-before-final-empty-line.any.html`
        // дословно: поток обрывается на середине второго блока. По спеке
        // диспатчится ровно один `message` с data === "test1", а блок
        // `id:test`/`data:test2` уходит вместе с соединением (BUG-845).
        let mut p = SseParser::new();
        let events = p.push_bytes(b"retry:400\ndata:test1\n\nid:test\ndata:test2");
        assert_eq!(events.len(), 1, "второй блок не завершён: {events:?}");
        assert_eq!(events[0].data, "test1");
        assert_eq!(p.take_retry(), None, "retry забран в событие");

        p.reset_stream();
        assert_eq!(
            p.last_event_id(),
            "",
            "id недиспатченного блока не уходит в Last-Event-ID"
        );

        // Следующее соединение отдаёт то же тело: склейки быть не должно.
        let events = p.push_bytes(b"retry:400\ndata:test1\n\nid:test\ndata:test2");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "test1", "не 'test2\\ntest1'");
    }

    #[test]
    fn take_retry_reports_value_parsed_without_any_dispatch() {
        // §9.2.6: `retry:` действует с момента разбора поля, а не с диспатча
        // события — поток может задать задержку и не отдать ни одного события.
        let mut p = SseParser::new();
        let events = p.push_bytes(b"retry: 700\n");
        assert!(events.is_empty());
        assert_eq!(p.take_retry(), Some(700));
        assert_eq!(p.take_retry(), None, "значение забирается один раз");
    }

    #[test]
    fn dispatch_and_take_retry_do_not_double_report() {
        // Диспатч тоже забирает `retry_ms` в событие; кто из двух успел первым,
        // тот и видит значение — но ровно один раз.
        let mut p = SseParser::new();
        let events = p.push_bytes(b"retry: 800\ndata: a\n\n");
        assert_eq!(events[0].retry_ms, Some(800));
        assert_eq!(p.take_retry(), None);
    }

    // ── Обрамление тела ответа (BUG-844) ──────────────────────────────────

    /// Резолвер «любое имя — это loopback»: тест ходит на свой же слушатель.
    struct LoopbackDns;
    impl DnsResolver for LoopbackDns {
        fn resolve(&self, _hostname: &str, port: u16) -> Result<Vec<std::net::SocketAddr>> {
            Ok(vec![std::net::SocketAddr::from(([127, 0, 0, 1], port))])
        }
    }

    /// Слушатель на эфемерном порту, не попадающий в список «плохих» портов
    /// Fetch §3.9 — иначе клиент откажется соединяться и тест замигает
    /// (форма BUG-911).
    fn good_listener() -> std::net::TcpListener {
        loop {
            let Ok(l) = std::net::TcpListener::bind("127.0.0.1:0") else {
                continue;
            };
            let Ok(addr) = l.local_addr() else { continue };
            if !crate::bad_port::is_bad_port(addr.port()) {
                return l;
            }
        }
    }

    /// Поднимает сервер, отвечающий `head` + `body` на каждое соединение и
    /// ДЕРЖАЩИЙ сокет открытым 10 с, и читает события клиентом в отдельном
    /// потоке. Возвращает канал событий и счётчик соединений.
    ///
    /// Сокет держится открытым намеренно: и `Content-Length`, и `chunked`
    /// заканчивают поток, не закрывая соединение, — ровно тот случай, который
    /// раньше концом потока не считался (BUG-844). Держать его нужно на
    /// отдельном потоке, иначе последовательный `accept` не примет
    /// переподключение и «одно соединение» прочтётся как его отсутствие.
    fn serve_holding(
        head: &'static str,
        body: &'static [u8],
    ) -> (
        std::sync::mpsc::Receiver<String>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::io::Write as _;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = good_listener();
        let Ok(addr) = listener.local_addr() else {
            panic!("local_addr")
        };
        let conns = Arc::new(AtomicUsize::new(0));
        let conns_srv = Arc::clone(&conns);

        std::thread::spawn(move || {
            for sock in listener.incoming() {
                let Ok(mut sock) = sock else { break };
                conns_srv.fetch_add(1, Ordering::SeqCst);
                // Заголовки запроса до пустой строки.
                let mut byte = [0u8; 1];
                let mut seen = Vec::new();
                while std::io::Read::read(&mut sock, &mut byte).unwrap_or(0) == 1 {
                    seen.push(byte[0]);
                    if seen.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.write_all(body);
                let _ = sock.flush();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    drop(sock);
                });
            }
        });

        let Ok(url) = Url::parse(&format!("http://127.0.0.1:{}/sse", addr.port())) else {
            panic!("url")
        };
        let Ok(mut es) = EventSource::connect(
            &url,
            Arc::new(LoopbackDns),
            Arc::new(lumen_core::ext::NoopEventSink),
            TabId(0),
        ) else {
            panic!("connect")
        };

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(Some(ev)) = es.next_event() {
                if tx.send(ev.data).is_err() {
                    break;
                }
            }
        });
        (rx, conns)
    }

    /// Три события подряд от сервера, отдающего по одному на соединение, —
    /// значит переподключение состоялось дважды. Ждём с ДЕДЛАЙНОМ: без него
    /// тест зелёный и при сломанном обрамлении, потому что чтение до закрытия
    /// сокета тоже когда-нибудь кончится — просто через 10 с (проверено
    /// мутацией `framing = Eof`: тест держался 20 с и всё равно проходил).
    /// Отличает исправное поведение только время.
    fn expect_three_reconnects(
        rx: &std::sync::mpsc::Receiver<String>,
        conns: &Arc<std::sync::atomic::AtomicUsize>,
        what: &str,
    ) {
        use std::sync::atomic::Ordering;
        let budget = std::time::Duration::from_secs(3);
        for i in 0..3 {
            match rx.recv_timeout(budget) {
                Ok(data) => assert_eq!(data, "a", "{what}: событие {i}"),
                Err(e) => panic!(
                    "{what}: событие {i} не пришло за {budget:?} ({e:?}) — поток \
                     не считается законченным; соединений на сервере {}",
                    conns.load(Ordering::SeqCst)
                ),
            }
        }
        assert!(
            conns.load(Ordering::SeqCst) >= 3,
            "{what}: каждое событие — своё соединение, получено {}",
            conns.load(Ordering::SeqCst)
        );
    }

    /// Тело, ограниченное `Content-Length` на keep-alive сокете — форма любого
    /// `.py`-обработчика `wptserve` (BUG-844).
    #[test]
    fn lengthed_body_on_open_socket_ends_stream_and_reconnects() {
        const BODY: &[u8] = b"retry: 50\ndata: a\n\n";
        let (rx, conns) = serve_holding(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
             Content-Length: 19\r\nConnection: keep-alive\r\n\r\n",
            BODY,
        );
        assert_eq!(BODY.len(), 19, "Content-Length должен совпасть с телом");
        expect_three_reconnects(&rx, &conns, "content-length");
    }

    /// `Transfer-Encoding: chunked` на том же открытом сокете: поток кончается
    /// последним чанком нулевого размера, а не закрытием соединения. Разбор
    /// размеров, CRLF после данных чанка и секции трейлеров — отдельная ветка
    /// `read_body`, которую случай с `Content-Length` не задевает.
    #[test]
    fn chunked_body_on_open_socket_ends_stream_at_last_chunk() {
        // Размеры в hex ровно по длине: "retry: 50\n" — 10 (a), "data: a\n\n" — 9.
        // Ошибка на единицу съела бы CRLF чанка и сломала разбор.
        const BODY: &[u8] = b"a\r\nretry: 50\n\r\n9\r\ndata: a\n\n\r\n0\r\n\r\n";
        let (rx, conns) = serve_holding(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
             Transfer-Encoding: chunked\r\n\r\n",
            BODY,
        );
        expect_three_reconnects(&rx, &conns, "chunked");
    }

    /// RFC 7230 §3.3.3(3): при обоих заголовках выигрывает `Transfer-Encoding`.
    /// Если бы победил `Content-Length`, клиент прочёл бы ровно N байт сырого
    /// чанкового кадра вместе с его служебными строками.
    #[test]
    fn chunked_wins_over_content_length() {
        const BODY: &[u8] = b"a\r\nretry: 50\n\r\n9\r\ndata: a\n\n\r\n0\r\n\r\n";
        let (rx, conns) = serve_holding(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
             Content-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n",
            BODY,
        );
        expect_three_reconnects(&rx, &conns, "chunked+content-length");
    }
}
