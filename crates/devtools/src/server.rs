//! DevTools TCP сервер — принимает WebSocket подключения, диспетчеризует CDP.

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crate::cdp::dispatch;
use crate::ws::{upgrade, read_text_frame, write_text_frame, WsError};

/// Фоновый DevTools сервер. Живёт пока не дропнется (join handle отсоединён).
pub struct DevToolsServer {
    port: u16,
}

impl DevToolsServer {
    /// Запустить сервер на `127.0.0.1:port`. Не блокирует — поток в фоне.
    ///
    /// Возвращает `Err` если порт занят или недоступен.
    pub fn spawn(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        eprintln!("[devtools] слушает ws://127.0.0.1:{port}");
        thread::Builder::new()
            .name("lumen-devtools".into())
            .spawn(move || accept_loop(listener))?;
        Ok(Self { port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

fn accept_loop(listener: TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || handle_connection(stream));
            }
            Err(e) => {
                eprintln!("[devtools] accept error: {e}");
                break;
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream) {
    // 30-секундный read timeout — защита от зависших соединений.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));

    if let Err(e) = upgrade(&mut stream) {
        eprintln!("[devtools] handshake failed: {e}");
        return;
    }

    loop {
        match read_text_frame(&mut stream) {
            Ok(msg) => {
                let response = dispatch(&msg);
                if let Err(e) = write_text_frame(&mut stream, &response) {
                    eprintln!("[devtools] write error: {e}");
                    break;
                }
            }
            Err(WsError::Closed) => break,
            Err(WsError::Io(e))
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                // read timeout — закрываем соединение
                break;
            }
            Err(e) => {
                eprintln!("[devtools] frame error: {e}");
                break;
            }
        }
    }
}
