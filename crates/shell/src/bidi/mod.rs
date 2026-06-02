//! WebDriver BiDi сервер (Phase 0 stub, §6.11, ADR-006).
//!
//! `lumen --bidi-port N` поднимает WebSocket-сервер W3C WebDriver BiDi на
//! `127.0.0.1:N`. На данном этапе реализован минимальный набор команд для
//! рукопожатия клиентов (Playwright/Selenium 5): `session.new`,
//! `session.status`, `browsingContext.getTree` + эмиссия события
//! `browsingContext.created`. Полная поверхность строится поверх
//! `BrowserSession` (см. `lumen-driver`).

mod protocol;
mod server;

pub use server::spawn;
