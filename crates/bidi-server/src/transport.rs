//! WebDriver BiDi WebSocket transport — RFC 6455 framing for one BiDi connection.
//!
//! Isolates the I/O layer from the protocol state machine. The TCP listener lives
//! in `server.rs`; this module handles per-connection WebSocket upgrade + read/write
//! loop, delegating each message to `protocol::dispatch`.

use std::net::{Shutdown, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use lumen_devtools::ws::{read_frame, upgrade, write_pong, write_text_frame, WsError, WsFrame};
use lumen_driver::{AutomationHandle, LiveWindowSession};

use crate::protocol::{dispatch, BidiState};

/// Handle one accepted TCP stream: WS upgrade → BiDi command loop.
///
/// Blocks until the connection is closed (by `session.end`, read timeout, or error).
/// The `stream` is fully consumed; the caller must not use it afterwards.
/// `automation` binds this connection's `BidiState` to a live window (SDC-2).
/// `required_token` — see [`BidiState::with_live_session`] (ADR-024
/// §Access model, DEVX-15); `None` disables the `session.new` token check.
pub fn handle(mut stream: TcpStream, automation: AutomationHandle, required_token: Option<String>) {
    // 60-second read timeout — guards against stalled connections.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));

    if let Err(e) = upgrade(&mut stream) {
        eprintln!("[bidi] handshake failed: {e}");
        return;
    }

    let mut state = BidiState::with_live_session(LiveWindowSession::new(automation), required_token);

    // Читающий поток и запись разведены: `dispatch()` может блокировать
    // основной поток надолго (навигация с `wait: "complete"`, синхронная
    // работа страницы), а клиент тем временем шлёт WebSocket Ping раз в
    // ping_interval и рвёт соединение, если не дождался Pong за
    // ping_timeout (BUG-981) — обе величины дефолтно 20с в клиенте
    // wptrunner. Reader-поток продолжает читать сокет и отвечает на Ping
    // немедленно, пока основной поток занят dispatch(); все записи в
    // сокет (Pong и ответ dispatch()) идут через общий `write_half`,
    // чтобы не перемежать байты двух фреймов на одном TCP-соединении.
    let write_half = match stream.try_clone() {
        Ok(s) => Arc::new(Mutex::new(s)),
        Err(e) => {
            eprintln!("[bidi] stream clone failed: {e}");
            return;
        }
    };
    let mut reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[bidi] stream clone failed: {e}");
            return;
        }
    };

    let (tx, rx) = mpsc::channel::<String>();
    let pong_write = Arc::clone(&write_half);
    let reader = thread::spawn(move || loop {
        match read_frame(&mut reader_stream) {
            Ok(WsFrame::Text(msg)) => {
                if tx.send(msg).is_err() {
                    break;
                }
            }
            Ok(WsFrame::Ping(payload)) => {
                let Ok(mut w) = pong_write.lock() else { break };
                if write_pong(&mut *w, &payload).is_err() {
                    break;
                }
            }
            Ok(WsFrame::Close) | Err(WsError::Closed) => break,
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
    });

    for msg in rx {
        let result = dispatch(&msg, &mut state);
        let mut write_failed = false;
        {
            let Ok(mut w) = write_half.lock() else { break };
            for frame in &result.frames {
                if let Err(e) = write_text_frame(&mut *w, frame) {
                    eprintln!("[bidi] write error: {e}");
                    write_failed = true;
                    break;
                }
            }
        }
        if write_failed || result.close {
            break;
        }
    }

    // Основной цикл вышел (close/ошибка записи/канал закрылся вместе с
    // reader-потоком) — оборвать сокет, чтобы блокирующий read() в
    // reader-потоке вернул ошибку, и дождаться его выхода.
    let _ = stream.shutdown(Shutdown::Both);
    let _ = reader.join();
}
