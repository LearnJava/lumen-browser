//! WebDriver BiDi сервер (Phase 1, §6.11, ADR-006).
//!
//! `lumen --bidi-port N` поднимает WebSocket-сервер W3C WebDriver BiDi на
//! `127.0.0.1:N`. Реализована protocol-слойная state-машина: `session.*`
//! (status/new/subscribe/unsubscribe/end с реальным хранением подписок) +
//! `browsingContext.*` (create/close/navigate/activate/getTree) с управлением
//! несколькими контекстами и event-gating по подпискам. Фактическая навигация
//! движка и расширенные события (`domContentLoaded`, response body, cookie
//! changes) строятся поверх `BrowserSession` (см. `lumen-driver`) — handoff P3,
//! roadmap 8H.3.

mod protocol;
mod server;

pub use server::spawn;
