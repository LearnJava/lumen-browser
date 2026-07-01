//! WebDriver BiDi TCP server — accepts WebSocket connections, one thread per connection.
//!
//! Clients (Playwright, Selenium 5) connect to `ws://127.0.0.1:<port>/session`.
//! Actual WebSocket framing and BiDi command dispatch are handled by `transport`.

use std::net::TcpListener;
use std::thread;

use lumen_driver::AutomationHandle;

use crate::transport;

/// Spawn the BiDi server on `127.0.0.1:port`. Non-blocking — runs in a background thread.
///
/// `automation` (SDC-2) is the live shell window's automation handle — each
/// connection gets its own [`lumen_driver::LiveWindowSession`] bound to a
/// clone of it, so `browsingContext.navigate`/`script.evaluate`/
/// `browsingContext.captureScreenshot`/`input.performActions` execute for
/// real. If no window is open, calls through the handle simply time out and
/// those commands fall back to their in-memory stub behavior.
///
/// Returns `Err` if the port is unavailable. The server runs until process exit.
pub fn spawn(port: u16, automation: AutomationHandle) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("[bidi] слушает ws://127.0.0.1:{port}");
    thread::Builder::new()
        .name("lumen-bidi".into())
        .spawn(move || accept_loop(listener, automation))?;
    Ok(())
}

/// Accept incoming connections, spawning one thread per connection.
fn accept_loop(listener: TcpListener, automation: AutomationHandle) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let automation = automation.clone();
                thread::spawn(move || transport::handle(stream, automation));
            }
            Err(e) => {
                eprintln!("[bidi] accept error: {e}");
                break;
            }
        }
    }
}
