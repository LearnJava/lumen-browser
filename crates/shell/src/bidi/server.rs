//! WebDriver BiDi TCP-сервер — принимает WebSocket-подключения, диспетчеризует
//! BiDi-команды (Phase 0 stub, §6.11, ADR-006).
//!
//! Клиенты (Playwright, Selenium 5) подключаются к `ws://127.0.0.1:<port>/session`.
//! Каждое соединение получает изолированное [`BidiState`] и обрабатывается в
//! отдельном потоке. WebSocket-кодек переиспользуется из `lumen-devtools::ws`
//! (RFC 6455 text-фреймы).

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use lumen_devtools::ws::{read_text_frame, upgrade, write_text_frame, WsError};

use crate::bidi::protocol::{dispatch, BidiState};

/// Запустить BiDi-сервер на `127.0.0.1:port`. Не блокирует — поток в фоне.
///
/// Сервер живёт до завершения процесса (поток отсоединён). Возвращает `Err`,
/// если порт занят или недоступен.
pub fn spawn(port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("[bidi] слушает ws://127.0.0.1:{port}");
    thread::Builder::new()
        .name("lumen-bidi".into())
        .spawn(move || accept_loop(listener))?;
    Ok(())
}

/// Принимать подключения, по одному потоку на соединение.
fn accept_loop(listener: TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || handle_connection(stream));
            }
            Err(e) => {
                eprintln!("[bidi] accept error: {e}");
                break;
            }
        }
    }
}

/// Обработать одно соединение: WS-handshake → цикл команд BiDi.
fn handle_connection(mut stream: TcpStream) {
    // 60-секундный read timeout — защита от зависших соединений.
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
