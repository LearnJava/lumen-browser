//! `lumen-mcp` — Model Context Protocol transport for Lumen automation API.
//!
//! Этот крейт предоставляет MCP-сервер, который обворачивает [`BrowserSession`]
//! трэйт из [`lumen_driver`] и открывает его через Model Context Protocol.
//!
//! # Ресурсы (Resources)
//!
//! MCP ресурсы предоставляют read-only доступ к текущему состоянию браузера:
//! - `screenshot` — PNG-снимок экрана
//! - `a11y_tree` — дерево доступности
//! - `layout` — layout-боксы и box-model
//! - `console` — логи console.log/warn/error
//! - `network` — логи HTTP-запросов
//!
//! # Инструменты (Tools)
//!
//! MCP инструменты предоставляют команды для управления браузером:
//! - `navigate` — загрузить URL
//! - `click` — клик по элементу
//! - `type` — ввод текста
//! - `scroll` — прокрутка
//! - `wait` — ожидание условия
//! - `eval` — выполнить JS
//! - `query` — поиск по селектору

pub mod protocol;
pub mod server;
pub mod transport;

pub use protocol::{McpMessage, McpRequest, McpResponse, McpResource, McpTool};
pub use server::McpServer;
pub use transport::Transport;
