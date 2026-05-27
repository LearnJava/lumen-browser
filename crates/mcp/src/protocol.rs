//! MCP protocol types (JSON-RPC 2.0).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// MCP resource describing a read-only data snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    /// Уникальный URI ресурса (e.g. "resource://screenshot", "resource://a11y_tree").
    pub uri: String,
    /// Человекочитаемое имя ресурса.
    pub name: String,
    /// Описание ресурса.
    pub description: String,
    /// MIME-тип контента (e.g. "image/png", "application/json").
    pub mime_type: String,
}

/// MCP tool describing a callable action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// Имя инструмента (e.g. "navigate", "click").
    pub name: String,
    /// Описание действия инструмента.
    pub description: String,
    /// JSON-schema аргументов инструмента.
    pub input_schema: Value,
}

/// MCP JSON-RPC запрос.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    /// JSON-RPC версия (всегда "2.0").
    pub jsonrpc: String,
    /// Уникальный ID запроса (может быть пустым для уведомлений).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Метод RPC.
    pub method: String,
    /// Параметры метода.
    #[serde(default)]
    pub params: Value,
}

impl McpRequest {
    /// Создать новый MCP запрос.
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.into(),
            params,
        }
    }

    /// Создать запрос с ID для отслеживания ответа.
    pub fn with_id(mut self, id: u64) -> Self {
        self.id = Some(json!(id));
        self
    }
}

/// MCP JSON-RPC ответ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    /// JSON-RPC версия (всегда "2.0").
    pub jsonrpc: String,
    /// ID запроса (совпадает с ID запроса).
    pub id: Value,
    /// Результат при успехе.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Ошибка при неудаче.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

impl McpResponse {
    /// Создать успешный ответ.
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Создать ошибку.
    pub fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(McpError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// JSON-RPC ошибка.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    /// Код ошибки.
    pub code: i32,
    /// Сообщение об ошибке.
    pub message: String,
    /// Дополнительные данные (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Размеченное MCP сообщение (запрос или ответ).
#[derive(Debug, Clone)]
pub enum McpMessage {
    /// Входящий запрос от клиента.
    Request(McpRequest),
    /// Исходящий ответ серверу.
    Response(McpResponse),
    /// Ошибка парсинга сообщения.
    Error(String),
}

impl McpMessage {
    /// Распарсить JSON в MCP сообщение.
    pub fn from_json(json_str: &str) -> Self {
        match serde_json::from_str::<McpRequest>(json_str) {
            Ok(req) => McpMessage::Request(req),
            Err(e) => McpMessage::Error(format!("JSON parse error: {e}")),
        }
    }

    /// Сериализовать MCP сообщение в JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        match self {
            McpMessage::Request(req) => serde_json::to_string(req),
            McpMessage::Response(resp) => serde_json::to_string(resp),
            McpMessage::Error(msg) => Ok(format!(r#"{{"error":"{}"}}"#, msg)),
        }
    }
}
