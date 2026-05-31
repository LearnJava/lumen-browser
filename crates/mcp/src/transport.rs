//! MCP transport layer (stdio, TCP socket, in-memory for tests).

use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;

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

/// TCP-транспорт для `--mcp-port N` режима.
///
/// Обслуживает одно соединение на заданном сокете (line-delimited JSON-RPC).
pub struct TcpTransport {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl TcpTransport {
    /// Создать транспорт поверх уже принятого `TcpStream`.
    pub fn from_stream(stream: TcpStream) -> io::Result<Self> {
        let writer = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
        })
    }
}

impl Transport for TcpTransport {
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

/// In-memory транспорт для unit-тестов.
///
/// Входящие сообщения кладутся через `push_incoming`, исходящие читаются через `take_outgoing`.
#[cfg(test)]
#[derive(Default)]
pub struct VecTransport {
    incoming: std::collections::VecDeque<String>,
    /// Все отправленные сервером сообщения в сериализованном виде.
    pub outgoing: Vec<String>,
}

#[cfg(test)]
impl VecTransport {
    /// Создать пустой транспорт.
    pub fn new() -> Self {
        Self::default()
    }

    /// Поставить в очередь входящее JSON сообщение.
    pub fn push_incoming(&mut self, json: &str) {
        self.incoming.push_back(json.to_string());
    }

    /// Забрать все исходящие сообщения (очищает буфер).
    pub fn take_outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outgoing)
    }
}

#[cfg(test)]
impl Transport for VecTransport {
    fn read_message(&mut self) -> Result<McpMessage> {
        match self.incoming.pop_front() {
            Some(line) => Ok(McpMessage::from_json(&line)),
            None => Err(lumen_core::error::Error::Other("no more messages".into())),
        }
    }

    fn write_message(&mut self, msg: &McpMessage) -> Result<()> {
        let json = msg
            .to_json()
            .map_err(|e| lumen_core::error::Error::Other(e.to_string()))?;
        self.outgoing.push(json);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{McpRequest, McpMessage};
    use serde_json::json;

    #[test]
    fn vec_transport_push_pop() {
        let mut t = VecTransport::new();
        let req = McpRequest::new("initialize", json!({})).with_id(42);
        let json = serde_json::to_string(&req).unwrap();
        t.push_incoming(&json);

        let msg = t.read_message().unwrap();
        match msg {
            McpMessage::Request(r) => {
                assert_eq!(r.method, "initialize");
                assert_eq!(r.id, Some(json!(42)));
            }
            _ => panic!("expected Request"),
        }
    }

    #[test]
    fn vec_transport_empty_yields_error() {
        let mut t = VecTransport::new();
        assert!(t.read_message().is_err());
    }

    #[test]
    fn vec_transport_write_message_captured() {
        use crate::protocol::McpResponse;
        let mut t = VecTransport::new();
        let resp = McpResponse::ok(json!(1), json!({ "ok": true }));
        t.write_message(&McpMessage::Response(resp)).unwrap();
        let out = t.take_outgoing();
        assert_eq!(out.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(&out[0]).unwrap();
        assert_eq!(parsed["result"]["ok"], true);
    }
}
