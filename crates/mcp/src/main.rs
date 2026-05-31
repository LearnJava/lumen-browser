//! Standalone MCP server для Lumen browser automation API.
//!
//! Используется как stdio-сервер для Claude Computer Use и других AI-агентов.
//!
//! # Запуск
//!
//! ```bash
//! # stdio (Claude Desktop / MCP клиент):
//! cargo run -p lumen-mcp -- [URL]
//!
//! # TCP сокет (для отладки через netcat):
//! cargo run -p lumen-mcp -- --port 7777 [URL]
//! ```

use std::net::TcpListener;

use lumen_driver::{BrowserSession, InProcessSession};
use lumen_mcp::{McpServer, StdioTransport, TcpTransport};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let (port, url) = parse_args(&raw_args);

    let mut session = InProcessSession::new();
    if let Some(u) = &url {
        let _ = session.navigate(u);
    }

    if let Some(port) = port {
        // TCP mode: принимаем одно соединение и обслуживаем его.
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        eprintln!("MCP listening on 127.0.0.1:{port}");
        let (stream, addr) = listener.accept()?;
        eprintln!("MCP connection from {addr}");
        let transport = TcpTransport::from_stream(stream)?;
        let mut server = McpServer::new(session, transport);
        let _ = server.run();
    } else {
        // Stdio mode.
        let transport = StdioTransport::new();
        let mut server = McpServer::new(session, transport);
        let _ = server.run();
    }

    Ok(())
}

fn parse_args(args: &[String]) -> (Option<u16>, Option<String>) {
    let mut port: Option<u16> = None;
    let mut url: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--port" {
            i += 1;
            if let Some(p) = args.get(i).and_then(|s| s.parse::<u16>().ok()) {
                port = Some(p);
            }
        } else if !args[i].starts_with("--") {
            url = Some(args[i].clone());
        }
        i += 1;
    }
    (port, url)
}
