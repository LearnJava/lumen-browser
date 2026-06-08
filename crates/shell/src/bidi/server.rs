//! WebDriver BiDi TCP server — accepts WebSocket connections, one thread per connection.
//!
//! Clients (Playwright, Selenium 5) connect to `ws://127.0.0.1:<port>/session`.
//! Actual WebSocket framing and BiDi command dispatch are handled by `transport`.

use std::net::TcpListener;
use std::thread;

use crate::bidi::transport;

/// Spawn the BiDi server on `127.0.0.1:port`. Non-blocking — runs in a background thread.
///
/// Returns `Err` if the port is unavailable. The server runs until process exit.
pub fn spawn(port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("[bidi] слушает ws://127.0.0.1:{port}");
    thread::Builder::new()
        .name("lumen-bidi".into())
        .spawn(move || accept_loop(listener))?;
    Ok(())
}

/// Accept incoming connections, spawning one thread per connection.
fn accept_loop(listener: TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                thread::spawn(move || transport::handle(stream));
            }
            Err(e) => {
                eprintln!("[bidi] accept error: {e}");
                break;
            }
        }
    }
}
