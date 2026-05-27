//! Standalone MCP server для Lumen browser automation API.
//!
//! Используется как stdio-сервер для Claude Computer Use и других AI-агентов.
//!
//! # Запуск
//!
//! ```bash
//! cargo run -p lumen-mcp -- <URL>
//! ```

use lumen_driver::{BrowserSession, InProcessSession};
use lumen_mcp::{McpServer, StdioTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Инициализация логирования (опционально).
    // tracing_subscriber::fmt()
    //     .with_max_level(tracing::Level::DEBUG)
    //     .init();

    // Получить URL из аргументов команды (опционально).
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "about:blank".to_string());

    // Создать headless сессию и загрузить страницу.
    let mut session = InProcessSession::new();
    if url != "about:blank" {
        session.navigate(&url)?;
    }

    // Создать MCP сервер поверх сессии.
    let transport = StdioTransport::new();
    let mut server = McpServer::new(session, transport);

    // Запустить основной цикл обработки MCP сообщений.
    server.run().await?;

    Ok(())
}
