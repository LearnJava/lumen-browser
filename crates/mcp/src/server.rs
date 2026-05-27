//! MCP сервер, оборачивающий [`BrowserSession`](lumen_driver::BrowserSession).

use serde_json::{json, Value};

use lumen_driver::BrowserSession;
use lumen_core::error::Result;

use crate::protocol::{McpMessage, McpRequest, McpResource, McpResponse, McpTool};
use crate::transport::Transport;

/// MCP сервер для Lumen браузера.
///
/// Обворачивает [`BrowserSession`] и предоставляет ресурсы и инструменты
/// через Model Context Protocol.
pub struct McpServer<S: BrowserSession, T: Transport> {
    /// Браузерная сессия.
    session: S,
    /// Транспортный канал (stdio, socket и т.д.).
    transport: T,
}

impl<S: BrowserSession, T: Transport> McpServer<S, T> {
    /// Создать новый MCP сервер.
    pub fn new(session: S, transport: T) -> Self {
        Self { session, transport }
    }

    /// Основной цикл сервера: читать запросы и писать ответы.
    pub async fn run(&mut self) -> Result<()> {
        loop {
            let msg = self.transport.read_message()?;

            match msg {
                McpMessage::Request(req) => {
                    let response = self.handle_request(&req).await;
                    self.transport.write_message(&McpMessage::Response(response))?;
                }
                McpMessage::Error(e) => {
                    eprintln!("Transport error: {}", e);
                    // Продолжить цикл, может быть это временная ошибка.
                }
                McpMessage::Response(_) => {
                    // Неожиданный ответ от клиента; игнорировать.
                }
            }
        }
    }

    /// Обработать один MCP запрос.
    async fn handle_request(&mut self, req: &McpRequest) -> McpResponse {
        let id = req.id.clone().unwrap_or(json!(null));

        match req.method.as_str() {
            // ── Инициализация ──
            "initialize" => self.on_initialize(&id),
            "resources/list" => self.on_resources_list(&id),
            "tools/list" => self.on_tools_list(&id),

            // ── Ресурсы ──
            "resources/read" => self.on_resources_read(&id, &req.params),

            // ── Инструменты ──
            "tools/call" => self.on_tools_call(&id, &req.params),

            _ => McpResponse::err(id, -32601, "Method not found"),
        }
    }

    /// Инициализация сервера.
    fn on_initialize(&self, id: &Value) -> McpResponse {
        let response = json!({
            "serverVersion": "0.1.0",
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "resources": {
                    "subscribe": false,
                },
                "tools": {},
                "sampling": {}
            }
        });

        McpResponse::ok(id.clone(), response)
    }

    /// Список доступных ресурсов.
    fn on_resources_list(&self, id: &Value) -> McpResponse {
        let resources = vec![
            McpResource {
                uri: "resource://screenshot".to_string(),
                name: "screenshot".to_string(),
                description: "PNG screenshot of the current viewport".to_string(),
                mime_type: "image/png".to_string(),
            },
            McpResource {
                uri: "resource://a11y_tree".to_string(),
                name: "a11y_tree".to_string(),
                description: "Accessibility tree of the current page".to_string(),
                mime_type: "application/json".to_string(),
            },
            McpResource {
                uri: "resource://layout".to_string(),
                name: "layout".to_string(),
                description: "Layout box model snapshot".to_string(),
                mime_type: "application/json".to_string(),
            },
            McpResource {
                uri: "resource://console".to_string(),
                name: "console".to_string(),
                description: "Console log entries (log, warn, error)".to_string(),
                mime_type: "application/json".to_string(),
            },
            McpResource {
                uri: "resource://network".to_string(),
                name: "network".to_string(),
                description: "Network request log".to_string(),
                mime_type: "application/json".to_string(),
            },
        ];

        let response = json!({
            "resources": resources
        });

        McpResponse::ok(id.clone(), response)
    }

    /// Список доступных инструментов.
    fn on_tools_list(&self, id: &Value) -> McpResponse {
        let tools = vec![
            McpTool {
                name: "navigate".to_string(),
                description: "Navigate to a URL (supports file://, http://, https://)".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to navigate to"
                        }
                    }
                }),
            },
            McpTool {
                name: "click".to_string(),
                description: "Click on an element".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target"],
                    "properties": {
                        "target": {
                            "type": "object",
                            "description": "Click target (selector, node_id, or point)"
                        }
                    }
                }),
            },
            McpTool {
                name: "type".to_string(),
                description: "Type text into an input field".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target", "text"],
                    "properties": {
                        "target": {
                            "type": "object",
                            "description": "Target element"
                        },
                        "text": {
                            "type": "string",
                            "description": "Text to type"
                        }
                    }
                }),
            },
            McpTool {
                name: "scroll".to_string(),
                description: "Scroll the page".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target", "delta"],
                    "properties": {
                        "target": {
                            "type": "object",
                            "description": "Scroll target"
                        },
                        "delta": {
                            "type": "object",
                            "properties": {
                                "x": {
                                    "type": "number",
                                    "description": "Horizontal scroll in logical pixels"
                                },
                                "y": {
                                    "type": "number",
                                    "description": "Vertical scroll in logical pixels"
                                }
                            }
                        }
                    }
                }),
            },
            McpTool {
                name: "wait".to_string(),
                description: "Wait for a condition (document ready, element visible, etc)".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["condition"],
                    "properties": {
                        "condition": {
                            "type": "string",
                            "enum": ["document_ready", "visible", "stable", "network_idle", "js_idle"],
                            "description": "Wait condition type"
                        },
                        "selector": {
                            "type": "string",
                            "description": "CSS selector (for visible/stable conditions)"
                        },
                        "timeout_ms": {
                            "type": "integer",
                            "description": "Timeout in milliseconds (default 30000)"
                        }
                    }
                }),
            },
            McpTool {
                name: "eval".to_string(),
                description: "Execute JavaScript code".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["code"],
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "JavaScript code to execute"
                        }
                    }
                }),
            },
            McpTool {
                name: "query".to_string(),
                description: "Find DOM elements by CSS selector".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["selector"],
                    "properties": {
                        "selector": {
                            "type": "string",
                            "description": "CSS selector"
                        }
                    }
                }),
            },
        ];

        let response = json!({
            "tools": tools
        });

        McpResponse::ok(id.clone(), response)
    }

    /// Чтение ресурса.
    fn on_resources_read(&self, id: &Value, params: &Value) -> McpResponse {
        let uri = match params.get("uri").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return McpResponse::err(id.clone(), -32602, "Missing uri parameter"),
        };

        match uri {
            "resource://screenshot" => {
                match self.session.screenshot() {
                    Ok(bytes) => {
                        let b64 = base64_encode(&bytes);
                        McpResponse::ok(id.clone(), json!({ "contents": [{ "type": "image", "data": b64, "mimeType": "image/png" }] }))
                    }
                    Err(e) => McpResponse::err(id.clone(), -32603, format!("Screenshot error: {e}")),
                }
            }
            "resource://a11y_tree" => {
                match self.session.a11y_tree() {
                    Ok(tree) => {
                        let json_str = serde_json::to_string(&tree).unwrap_or_default();
                        McpResponse::ok(id.clone(), json!({ "contents": [{ "type": "text", "text": json_str, "mimeType": "application/json" }] }))
                    }
                    Err(e) => McpResponse::err(id.clone(), -32603, format!("A11y tree error: {e}")),
                }
            }
            "resource://layout" => {
                match self.session.layout_snapshot() {
                    Ok(boxes) => {
                        let json_str = serde_json::to_string(&boxes).unwrap_or_default();
                        McpResponse::ok(id.clone(), json!({ "contents": [{ "type": "text", "text": json_str, "mimeType": "application/json" }] }))
                    }
                    Err(e) => McpResponse::err(id.clone(), -32603, format!("Layout error: {e}")),
                }
            }
            "resource://console" => {
                match self.session.console_log() {
                    Ok(logs) => {
                        let json_str = serde_json::to_string(&logs).unwrap_or_default();
                        McpResponse::ok(id.clone(), json!({ "contents": [{ "type": "text", "text": json_str, "mimeType": "application/json" }] }))
                    }
                    Err(e) => McpResponse::err(id.clone(), -32603, format!("Console log error: {e}")),
                }
            }
            "resource://network" => {
                match self.session.network_log() {
                    Ok(logs) => {
                        let json_str = serde_json::to_string(&logs).unwrap_or_default();
                        McpResponse::ok(id.clone(), json!({ "contents": [{ "type": "text", "text": json_str, "mimeType": "application/json" }] }))
                    }
                    Err(e) => McpResponse::err(id.clone(), -32603, format!("Network log error: {e}")),
                }
            }
            _ => McpResponse::err(id.clone(), -32602, format!("Unknown resource: {uri}")),
        }
    }

    /// Вызов инструмента.
    fn on_tools_call(&mut self, id: &Value, params: &Value) -> McpResponse {
        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return McpResponse::err(id.clone(), -32602, "Missing tool name"),
        };

        let default_args = json!({});
        let args = params.get("arguments").unwrap_or(&default_args);

        let result = match name {
            "navigate" => {
                let url = match args.get("url").and_then(|v| v.as_str()) {
                    Some(u) => u,
                    None => return McpResponse::err(id.clone(), -32602, "Missing url argument"),
                };
                match self.session.navigate(url) {
                    Ok(()) => json!({ "success": true, "url": url }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Navigate error: {e}")),
                }
            }
            "click" => {
                let target_obj = match args.get("target") {
                    Some(t) => t,
                    None => return McpResponse::err(id.clone(), -32602, "Missing target argument"),
                };
                let target = parse_target(target_obj);
                match self.session.click(&target) {
                    Ok(()) => json!({ "success": true }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Click error: {e}")),
                }
            }
            "type" => {
                let target_obj = match args.get("target") {
                    Some(t) => t,
                    None => return McpResponse::err(id.clone(), -32602, "Missing target argument"),
                };
                let text = match args.get("text").and_then(|v| v.as_str()) {
                    Some(t) => t,
                    None => return McpResponse::err(id.clone(), -32602, "Missing text argument"),
                };
                let target = parse_target(target_obj);
                match self.session.type_text(&target, text) {
                    Ok(()) => json!({ "success": true, "text": text }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Type error: {e}")),
                }
            }
            "scroll" => {
                let target_obj = match args.get("target") {
                    Some(t) => t,
                    None => return McpResponse::err(id.clone(), -32602, "Missing target argument"),
                };
                let target = parse_target(target_obj);

                let delta_obj = match args.get("delta") {
                    Some(d) => d,
                    None => return McpResponse::err(id.clone(), -32602, "Missing delta argument"),
                };

                let delta_x = delta_obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let delta_y = delta_obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

                let delta = lumen_driver::ScrollDelta { x: delta_x, y: delta_y };
                match self.session.scroll(&target, delta) {
                    Ok(()) => json!({ "success": true, "delta": { "x": delta_x, "y": delta_y } }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Scroll error: {e}")),
                }
            }
            "wait" => {
                let condition_str = match args.get("condition").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => return McpResponse::err(id.clone(), -32602, "Missing condition argument"),
                };

                let selector = args.get("selector").and_then(|v| v.as_str()).unwrap_or("body");
                let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64()).unwrap_or(30000);

                let cond = match condition_str {
                    "document_ready" => lumen_driver::WaitCondition::DocumentReady,
                    "visible" => lumen_driver::WaitCondition::Visible(selector.to_string()),
                    "stable" => lumen_driver::WaitCondition::Stable(selector.to_string()),
                    "network_idle" => lumen_driver::WaitCondition::NetworkIdle,
                    "js_idle" => lumen_driver::WaitCondition::JsIdle,
                    _ => return McpResponse::err(id.clone(), -32602, format!("Unknown condition: {condition_str}")),
                };

                match self.session.wait(cond, timeout_ms) {
                    Ok(()) => json!({ "success": true, "condition": condition_str }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Wait error: {e}")),
                }
            }
            "eval" => {
                let code = match args.get("code").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => return McpResponse::err(id.clone(), -32602, "Missing code argument"),
                };
                match self.session.eval(code) {
                    Ok(result) => json!({ "success": true, "result": result }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Eval error: {e}")),
                }
            }
            "query" => {
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                match self.session.query(selector) {
                    Ok(nodes) => json!({ "nodes": nodes }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Query error: {e}")),
                }
            }
            _ => {
                return McpResponse::err(id.clone(), -32601, format!("Unknown tool: {name}"));
            }
        };

        McpResponse::ok(id.clone(), result)
    }
}

/// Parse Target from JSON object.
/// Supports { "selector": "..." }, { "node_id": 123 }, or { "point": { "x": ..., "y": ... } }
fn parse_target(obj: &Value) -> lumen_driver::Target {
    if let Some(selector) = obj.get("selector").and_then(|v| v.as_str()) {
        return lumen_driver::Target::Selector(selector.to_string());
    }

    if let Some(node_id) = obj.get("node_id").and_then(|v| v.as_u64()) {
        return lumen_driver::Target::NodeId(node_id as u32);
    }

    if let Some(point_obj) = obj.get("point").and_then(|v| v.as_object()) {
        let x = point_obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let y = point_obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        return lumen_driver::Target::Point { x, y };
    }

    // Default: treat as selector
    lumen_driver::Target::Selector("body".to_string())
}

// Helper function: base64 encode
fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;

    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    let mut i = 0;
    while i < data.len() {
        let b1 = data[i];
        let b2 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b3 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        let n = ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);

        let _ = write!(
            result,
            "{}{}{}{}",
            CHARSET[((n >> 18) & 0x3f) as usize] as char,
            CHARSET[((n >> 12) & 0x3f) as usize] as char,
            if i + 1 < data.len() {
                CHARSET[((n >> 6) & 0x3f) as usize] as char
            } else {
                '='
            },
            if i + 2 < data.len() {
                CHARSET[(n & 0x3f) as usize] as char
            } else {
                '='
            }
        );

        i += 3;
    }

    result
}
