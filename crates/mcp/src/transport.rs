//! MCP transport layer (stdio, sockets).

use std::io::{self, BufRead, BufReader, Write};

use crate::protocol::McpMessage;
use lumen_core::error::Result;

/// Абстракция транспорта для MCP сообщений.
pub trait Transport: Send + Sync {
    /// Прочитать одно сообщение (блокирующе).
    fn read_message(&mut self) -> Result<McpMessage>;

    /// Отправить сообщение (блокирующе).
    fn write_message(&mut self, msg: &McpMessage) -> Result<()>;
}

/// Stdio-транспорт (stdin/stdout).
///
/// Читает JSON-RPC сообщения из stdin (одно на строку),
/// пишет ответы в stdout.
pub struct StdioTransport {
    reader: BufReader<io::Stdin>,
    writer: io::Stdout,
}

impl StdioTransport {
    /// Создать новый stdio-транспорт.
    pub fn new() -> Self {
        Self {
            reader: BufReader::new(io::stdin()),
            writer: io::stdout(),
        }
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for StdioTransport {
    fn read_message(&mut self) -> Result<McpMessage> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Err(lumen_core::error::Error::Other("EOF".into())),
            Ok(_) => Ok(McpMessage::from_json(&line)),
            Err(e) => Err(lumen_core::error::Error::Io(e.to_string())),
        }
    }

    fn write_message(&mut self, msg: &McpMessage) -> Result<()> {
        let json = msg
            .to_json()
            .map_err(|e| lumen_core::error::Error::Other(e.to_string()))?;
        writeln!(self.writer, "{}", json)
            .map_err(|e| lumen_core::error::Error::Io(e.to_string()))?;
        self.writer
            .flush()
            .map_err(|e| lumen_core::error::Error::Io(e.to_string()))?;
        Ok(())
    }
}
