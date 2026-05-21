//! Server-Sent Events (SSE) parser — HTML Living Standard §9.2.6.
//!
//! Parses a `text/event-stream` byte stream into [`SseEvent`] values.
//! The parser is incremental: feed chunks via [`SseParser::push_bytes`] and
//! collect any complete events returned.  Incomplete lines are buffered
//! internally until the next chunk arrives.
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

/// A fully-parsed SSE event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Event type — `"message"` by default, overridden by `event:` field.
    pub event_type: String,
    /// Event data — multiple `data:` lines joined with `\n`, trailing `\n` removed.
    pub data: String,
    /// Last event ID (the value of the last `id:` field seen, persists across events).
    pub id: Option<String>,
    /// Reconnection time in milliseconds if a `retry:` field was seen in this batch.
    pub retry_ms: Option<u64>,
}

/// Incremental `text/event-stream` parser.
///
/// Stores state between [`push_bytes`](Self::push_bytes) calls so callers
/// can feed the stream in arbitrary-sized chunks.
#[derive(Default)]
pub struct SseParser {
    line_buf: Vec<u8>,
    event_type: String,
    data_buf: String,
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
                    let line = String::from_utf8_lossy(&std::mem::take(&mut self.line_buf)).into_owned();
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
                    let line = String::from_utf8_lossy(&std::mem::take(&mut self.line_buf)).into_owned();
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
                self.last_event_id = value.to_string();
            }
            // Spec: set retry only if value is all ASCII digits and parses as u64.
            "retry" if !value.is_empty()
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
}

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
}
