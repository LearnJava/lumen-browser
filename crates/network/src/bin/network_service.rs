//! `lumen-network-service` — сетевой процесс, изолирующий HTTP/TLS/DNS.
//!
//! Запускается шеллом как дочерний процесс:
//! ```text
//! lumen-network-service
//! ```
//! Процесс:
//! 1. Привязывается к случайному порту на 127.0.0.1.
//! 2. Печатает номер порта в stdout одной строкой ("<port>\n").
//! 3. Принимает одно TCP-соединение (от шелла) и обрабатывает запросы в цикле.
//! 4. Завершается при получении `IpcRequest::Shutdown` или разрыве соединения.

use std::io::Write as _;

use lumen_core::ext::NetworkTransport as _;
use lumen_ipc::{FetchErr, FetchOk, IpcRequest, IpcResponse, IpcServer};
use lumen_network::HttpClient;

fn main() {
    // Bind on random loopback port and tell the shell which port we got.
    let (server, port) = IpcServer::bind().unwrap_or_else(|e| {
        eprintln!("lumen-network-service: bind failed: {e}");
        std::process::exit(1);
    });

    // One line to stdout — shell reads this to know where to connect.
    println!("{port}");
    std::io::stdout().flush().ok();

    // Build the HTTP client.  Phase 1: default settings, no filters/auth/cache.
    // Phase 2 will accept a config file path via argv and populate all providers.
    let client = HttpClient::new();

    // Accept exactly one connection (from the shell).  If the shell crashes, the
    // accept() call will unblock because the kernel closes the socket and we exit.
    let mut conn = server.accept().unwrap_or_else(|e| {
        eprintln!("lumen-network-service: accept failed: {e}");
        std::process::exit(1);
    });

    // Request handling loop.
    loop {
        let req = match conn.recv::<IpcRequest>() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("lumen-network-service: recv error (shell disconnected?): {e}");
                break;
            }
        };

        match req {
            IpcRequest::Fetch(fr) => {
                let resp = match lumen_core::url::Url::parse(&fr.url) {
                    Ok(url) => match client.fetch(&url) {
                        Ok(body) => IpcResponse::FetchOk(FetchOk {
                            id: fr.id,
                            status: 200,
                            headers: vec![],
                            body,
                        }),
                        Err(e) => IpcResponse::FetchErr(FetchErr {
                            id: fr.id,
                            error: e.to_string(),
                        }),
                    },
                    Err(e) => IpcResponse::FetchErr(FetchErr {
                        id: fr.id,
                        error: format!("invalid url '{}': {e}", fr.url),
                    }),
                };
                if let Err(e) = conn.send(&resp) {
                    eprintln!("lumen-network-service: send error: {e}");
                    break;
                }
            }
            IpcRequest::Ping => {
                if let Err(e) = conn.send(&IpcResponse::Pong) {
                    eprintln!("lumen-network-service: pong send error: {e}");
                    break;
                }
            }
            IpcRequest::Shutdown => {
                // Best-effort acknowledgement; ignore write errors.
                let _ = conn.send(&IpcResponse::Shutdown);
                break;
            }
        }
    }
}
