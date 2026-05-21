//! Lumen DevTools — WebSocket сервер с минимальным набором CDP (5C).
//!
//! Запускается в фоновом потоке командой [`DevToolsServer::spawn`].
//! Клиенты подключаются через Chrome DevTools Protocol: `ws://127.0.0.1:<port>`.
//!
//! Поддерживаемые CDP-методы (Phase 0):
//! - `Browser.getVersion`
//! - `DOM.getDocument` (stub — пустой документ)
//! - `Network.enable` / `CSS.enable` / `Page.enable` (ACK)
//!
//! Всё остальное возвращает JSON-RPC error -32601 "Method not found".

mod cdp;
mod server;
pub mod ws;

pub use server::DevToolsServer;
