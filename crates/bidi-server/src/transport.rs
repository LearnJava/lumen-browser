//! WebDriver BiDi WebSocket transport — RFC 6455 framing for one BiDi connection.
//!
//! Isolates the I/O layer from the protocol state machine. The TCP listener lives
//! in `server.rs`; this module handles per-connection WebSocket upgrade + read/write
//! loop, delegating each message to `protocol::dispatch`.

use std::net::TcpStream;
use std::time::Duration;

use lumen_devtools::ws::{read_text_frame, upgrade, write_text_frame, WsError};

use crate::protocol::{dispatch, BidiState};

/// Handle one accepted TCP stream: WS upgrade → BiDi command loop.
///
/// Blocks until the connection is closed (by `session.end`, read timeout, or error).
/// The `stream` is fully consumed; the caller must not use it afterwards.
pub fn handle(mut stream: TcpStream) {
    // 60-second read timeout — guards against stalled connections.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));

    if let Err(e) = upgrade(&mut stream) {
        eprintln!("[bidi] handshake failed: {e}");
        return;
    }

    let mut state = BidiState::new();
    loop {
        match read_text_frame(&mut stream) {
            Ok(msg) => {
                let result = dispatch(&msg, &mut state);
                let mut write_failed = false;
                for frame in &result.frames {
                    if let Err(e) = write_text_frame(&mut stream, frame) {
                        eprintln!("[bidi] write error: {e}");
                        write_failed = true;
                        break;
                    }
                }
                if write_failed || result.close {
                    break;
                }
            }
            Err(WsError::Closed) => break,
            Err(WsError::Io(e))
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                break;
            }
            Err(e) => {
                eprintln!("[bidi] frame error: {e}");
                break;
            }
        }
    }
}
