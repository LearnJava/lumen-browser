//! RFC 6455 WebSocket client.
//!
//! Entry point: [`WebSocket::connect`].

pub(crate) mod frame;
pub(crate) mod mask;
pub(crate) mod upgrade;

use std::sync::Arc;

use lumen_core::error::Result;
use lumen_core::event::{Event, TabId};
use lumen_core::ext::{DnsResolver, EventSink, HstsEnforcement, WebSocketSession, WsMessage};
use lumen_core::url::Url;

use crate::{connect, Error, RawStream};
use frame::Opcode;

// ── Scheme helpers ────────────────────────────────────────────────────────────

/// Parse ws:// or wss:// URL into (host_ascii, port, is_tls, path_and_query).
fn require_ws_scheme(url: &Url) -> Result<(String, u16, bool, String)> {
    let is_tls = match url.scheme() {
        "ws"  => false,
        "wss" => true,
        other => return Err(Error::Network(format!("ws: unsupported scheme: {other}"))),
    };
    let host = url
        .host_ascii()
        .map_err(|e| Error::Network(e.to_string()))?;
    if host.is_empty() {
        return Err(Error::Network(format!(
            "ws: empty host in URL: {}",
            url.as_str()
        )));
    }
    let port = url
        .effective_port()
        .ok_or_else(|| Error::Network(format!("ws: no port for URL: {}", url.as_str())))?;
    let path = url.path_and_query();
    Ok((host, port, is_tls, path))
}

// ── WebSocket ─────────────────────────────────────────────────────────────────

/// Open WebSocket connection. Implements [`WebSocketSession`].
pub(crate) struct WebSocket {
    stream:   RawStream,
    url:      Url,
    tab_id:   TabId,
    sink:     Arc<dyn EventSink>,
    /// Accumulator for fragmented data messages (RFC 6455 §5.4).
    frag_buf: Vec<u8>,
    /// Opcode of the first fragment (Text or Binary).
    frag_op:  Option<Opcode>,
    closed:   bool,
}

impl WebSocket {
    /// Establish a WebSocket connection to `url` (`ws://` or `wss://`).
    pub(crate) fn connect(
        url:      &Url,
        resolver: &dyn DnsResolver,
        hsts:     Option<&dyn HstsEnforcement>,
        sink:     Arc<dyn EventSink>,
        tab_id:   TabId,
    ) -> Result<Self> {
        let (host, port, mut is_tls, path) = require_ws_scheme(url)?;

        // HSTS upgrade: ws → wss when the host has a Strict-Transport-Security entry.
        if !is_tls && let Some(h) = hsts {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if h.is_https_only(&host, now) {
                is_tls = true;
            }
        }

        let conn = connect(&host, port, is_tls, resolver, crate::tls::TlsProfile::Standard)?;
        let mut stream = conn.into_stream();

        let key = upgrade::generate_key();
        upgrade::perform(&mut stream, &host, &path, &key, &[])?;

        sink.emit(&Event::WebSocketConnected {
            tab_id,
            url: url.clone(),
        });

        Ok(Self {
            stream,
            url: url.clone(),
            tab_id,
            sink,
            frag_buf: Vec::new(),
            frag_op: None,
            closed: false,
        })
    }

    /// Generate a 4-byte masking key (pseudo-random, not crypto-grade).
    fn mask_key() -> [u8; 4] {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0xDEAD_BEEFu32);
        seed.wrapping_mul(0x9E37_79B9).to_le_bytes()
    }

    fn send_frame(&mut self, opcode: Opcode, payload: &[u8]) -> Result<()> {
        frame::write_frame(&mut self.stream, true, opcode, payload, Some(Self::mask_key()))
    }

    /// Read frames, handling control frames inline, until we have a full
    /// application message (RFC 6455 §5.4 fragmentation reassembly).
    fn recv_inner(&mut self) -> Result<WsMessage> {
        loop {
            let fr = frame::read_frame(&mut self.stream)?;

            match fr.opcode {
                Opcode::Ping => {
                    // RFC 6455 §5.5.2: respond with Pong immediately.
                    let payload = fr.payload.clone();
                    self.send_frame(Opcode::Pong, &payload)?;
                    return Ok(WsMessage::Ping(fr.payload));
                }
                Opcode::Pong => {
                    return Ok(WsMessage::Pong(fr.payload));
                }
                Opcode::Close => {
                    let (code, reason) = frame::parse_close_payload(&fr.payload);
                    if !self.closed {
                        let echo = frame::make_close_payload(code.unwrap_or(1000), &reason);
                        let _ = self.send_frame(Opcode::Close, &echo);
                        self.closed = true;
                    }
                    self.sink.emit(&Event::WebSocketClosed {
                        tab_id: self.tab_id,
                        url:    self.url.clone(),
                        code,
                        reason: reason.clone(),
                    });
                    return Ok(WsMessage::Close { code, reason });
                }
                Opcode::Text | Opcode::Binary => {
                    if fr.fin {
                        return self.finish_data(fr.opcode, fr.payload);
                    }
                    self.frag_op  = Some(fr.opcode);
                    self.frag_buf = fr.payload;
                }
                Opcode::Continuation => {
                    let Some(op) = self.frag_op else {
                        return Err(Error::Network(
                            "ws: continuation frame without preceding data frame".into(),
                        ));
                    };
                    self.frag_buf.extend_from_slice(&fr.payload);
                    if fr.fin {
                        let buf = std::mem::take(&mut self.frag_buf);
                        self.frag_op = None;
                        return self.finish_data(op, buf);
                    }
                }
            }
        }
    }

    /// Emit the message event and convert raw bytes into a `WsMessage`.
    fn finish_data(&self, opcode: Opcode, payload: Vec<u8>) -> Result<WsMessage> {
        if opcode == Opcode::Text {
            let text = String::from_utf8(payload)
                .map_err(|_| Error::Network("ws: invalid UTF-8 in text message".into()))?;
            self.sink.emit(&Event::WebSocketMessage {
                tab_id:    self.tab_id,
                url:       self.url.clone(),
                is_binary: false,
                len:       text.len(),
            });
            Ok(WsMessage::Text(text))
        } else {
            let len = payload.len();
            self.sink.emit(&Event::WebSocketMessage {
                tab_id:    self.tab_id,
                url:       self.url.clone(),
                is_binary: true,
                len,
            });
            Ok(WsMessage::Binary(payload))
        }
    }
}

impl WebSocketSession for WebSocket {
    fn send_text(&mut self, text: &str) -> Result<()> {
        self.send_frame(Opcode::Text, text.as_bytes())
    }

    fn send_binary(&mut self, data: &[u8]) -> Result<()> {
        self.send_frame(Opcode::Binary, data)
    }

    fn recv(&mut self) -> Result<WsMessage> {
        self.recv_inner()
    }

    fn close(&mut self, code: u16, reason: &str) -> Result<()> {
        if !self.closed {
            let payload = frame::make_close_payload(code, reason);
            self.send_frame(Opcode::Close, &payload)?;
            self.closed = true;
        }
        Ok(())
    }
}
