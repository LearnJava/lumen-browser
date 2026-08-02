//! MCP over TCP against a live shell window (SDC-2).
//!
//! `run_mcp_mode` (in `lumen-shell`) already serves MCP over stdio/TCP for
//! headless AI-agent automation via `InProcessSession`. This module is the
//! live-window counterpart: `lumen --mcp-live-port N <url>` opens a real,
//! visible window and lets each accepted MCP connection drive it through
//! [`lumen_driver::LiveWindowSession`] — so `screenshot`/`eval` (and the rest
//! of the tool/resource surface) execute for real instead of the headless
//! session's `Err`/empty-default fallbacks.

use std::net::TcpListener;
use std::thread;

use lumen_driver::{AutomationHandle, LiveWindowSession};

use crate::{McpServer, TcpTransport};

/// Spawn the live-window MCP server on `127.0.0.1:port`. Non-blocking — runs
/// in a background thread, one connection at a time (matches the existing
/// `--mcp-port` debug-transport model: simple, not meant for concurrent agents).
///
/// Prints a fresh per-run token to stderr next to the port line (ADR-024
/// §Access model, DEVX-15) — every connection must authenticate with it via
/// `initialize` before any other method is served.
///
/// Returns `Err` if the port is unavailable. Runs until process exit.
pub fn spawn(port: u16, automation: AutomationHandle) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let token = lumen_core::auth::generate_token();
    eprintln!("[mcp] слушает 127.0.0.1:{port} (live window)");
    eprintln!("[mcp] token: {token}");
    thread::Builder::new()
        .name("lumen-mcp-live".into())
        .spawn(move || accept_loop(listener, automation, token))?;
    Ok(())
}

/// Accept incoming connections one at a time; each gets its own
/// [`LiveWindowSession`] bound to a clone of the automation handle.
fn accept_loop(listener: TcpListener, automation: AutomationHandle, token: String) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match TcpTransport::from_stream(stream) {
                Ok(transport) => {
                    let session = LiveWindowSession::new(automation.clone());
                    let mut server = McpServer::with_token(session, transport, token.clone());
                    if let Err(e) = server.run() {
                        eprintln!("[mcp] connection error: {e}");
                    }
                }
                Err(e) => eprintln!("[mcp] transport error: {e}"),
            },
            Err(e) => {
                eprintln!("[mcp] accept error: {e}");
                break;
            }
        }
    }
}
