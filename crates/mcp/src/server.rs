//! MCP сервер, оборачивающий [`BrowserSession`](lumen_driver::BrowserSession).

use serde_json::{json, Value};

use lumen_driver::BrowserSession;
use lumen_core::error::Result;

use crate::protocol::{McpMessage, McpRequest, McpResource, McpResponse, McpTool};
use crate::transport::Transport;

/// Версии протокола MCP, которые понимает сервер; первая — новейшая.
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// MCP сервер для Lumen браузера.
///
/// Обворачивает [`BrowserSession`] и предоставляет ресурсы и инструменты
/// через Model Context Protocol.
pub struct McpServer<S: BrowserSession, T: Transport> {
    /// Браузерная сессия.
    session: S,
    /// Транспортный канал (stdio, socket и т.д.).
    transport: T,
    /// Токен, обязательный для `initialize` (ADR-024 §Access model, DEVX-15).
    /// `None` — аутентификация не требуется (stdio-режим: недоступен другим
    /// локальным процессам, см. модульный doc-комментарий [`crate`]).
    required_token: Option<String>,
    /// `true`, если аутентификация не требуется, либо `initialize` уже прошёл
    /// с верным токеном. Пока `false`, любой метод кроме `initialize`
    /// отклоняется.
    authenticated: bool,
    /// Счётчик opaque-идентификаторов, которые сервер генерирует сам, когда
    /// клиент не передал `id` (`add_intercept`, `add_preload_script` —
    /// MCP-TOTAL-S1d; по образцу BiDi `state.next_id`).
    id_counter: u64,
}

/// Инструкция для ИИ-агента, отдаётся в `initialize` (MCP §Lifecycle, `instructions`).
const SERVER_INSTRUCTIONS: &str = "Lumen is a browser you drive and inspect. Workflow: (1) `navigate` (current tab) or `new_tab` with a URL; (2) `wait` with condition document_ready, or network_idle for script-heavy pages — navigation returns before the page has finished loading; (3) read the page with `query` (CSS selector -> nodes with node_id, text, rect) or `eval` (JavaScript; the result is a string, so use JSON.stringify for structured data); (4) interact with `click` / `type` / `scroll`, then `wait` again. Raw input primitives — `pointer_move`, `pointer_down`, `pointer_up`, `key_down`, `key_up`, `type_focused` — drive the live window directly and work only there (`--mcp-live-port`); a headless session answers an error. `set_files {target, paths}` fills an `<input type=file>` with real files so the page sees a FileList with readable content plus input/change events while the paths stay on the automation side — live window only too. JS dialogs (`alert`/`confirm`/`prompt`) block the page's JS thread until answered: `user_prompt` reads the open dialog, `handle_user_prompt {accept, user_text?}` answers it — both live window only, and `eval` answers busy while a dialog is open. Emulation tools flip what the page sees without touching its DOM — `set_offline`, `set_user_agent`, `set_timezone`, `set_permission` are process-global overrides (network, UA header + `navigator.userAgent`, `Intl`/`Date` timezone, permission states) and `computed_a11y` reads an element's computed accessibility role and name; they work only in the live window except `set_user_agent`, which a headless session also accepts. Network interception, cookies and preload scripts are live window only (`--mcp-live-port`; a headless session answers an error): `add_intercept {phases, url_patterns, id?}` pauses matching requests at `beforeRequestSent`/`responseStarted` until `resolve_intercept {request_id, continue}` decides them, `poll_intercepts` lists the requests paused since the last call, and `add_preload_script {function_declaration, id?}` runs a function in every document created from now on before its own scripts; `list_cookies` / `set_cookie` / `delete_cookie` read and write the active profile's real cookie jar — what `document.cookie` and requests see. Downloads (live window only): `start_download {url}` fetches a URL into the OS Downloads directory and returns its id, `list_downloads` reports every entry (`state` is pending / in_progress / completed / failed / cancelled; a completed entry's `path` holds where the file was written), `cancel_download {id}` aborts one. A popup (`window.open`, `target=_blank`) opens as a new tab but does not take over the tools: `list_tabs`, then `switch_tab` to it, and `close_tab` / `switch_tab` back when done. `target` of click/type/scroll is a CSS selector string, {selector}, {node_id} or {point: {x, y}}. `screenshot` and the accessibility, layout, console and network resources help to verify what is shown. `eval` may time out on a busy page: raise `timeout_ms`. Tools prefixed `x-` are experimental (ADR-024): causal explanation of styles and layout (`x-computed-style`, `x-explain-element`, `x-explain-page`, `x-scope-layout`), their output format may change. Errors come back as JSON-RPC errors with a message; a failed tool does not close the session.";

impl<S: BrowserSession, T: Transport> McpServer<S, T> {
    /// Создать новый MCP сервер без обязательной аутентификации (stdio-режим
    /// или тесты — недоступен другим локальным процессам).
    pub fn new(session: S, transport: T) -> Self {
        Self { session, transport, required_token: None, authenticated: true, id_counter: 0 }
    }

    /// Создать MCP сервер, требующий верный токен в параметрах `initialize`
    /// (ADR-024 §Access model, DEVX-15) — используется на всех TCP-портах
    /// (`--mcp-port`/`--mcp-live-port`), доступных любому локальному процессу.
    pub fn with_token(session: S, transport: T, token: String) -> Self {
        Self { session, transport, required_token: Some(token), authenticated: false, id_counter: 0 }
    }

    /// Основной цикл сервера: читать запросы и писать ответы.
    pub fn run(&mut self) -> Result<()> {
        loop {
            let msg = self.transport.read_message()?;

            match msg {
                McpMessage::Request(req) => {
                    // Уведомление (нет `id`) — ответа не бывает: ни успешного,
                    // ни ошибки (JSON-RPC 2.0 §4.1, MCP §Lifecycle).
                    if req.id.is_none() {
                        continue;
                    }
                    let response = self.handle_request(&req);
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
    fn handle_request(&mut self, req: &McpRequest) -> McpResponse {
        let id = req.id.clone().unwrap_or(json!(null));

        if !self.authenticated && req.method != "initialize" {
            return McpResponse::err(
                id,
                -32001,
                "Not authenticated: call `initialize` with a valid token first",
            );
        }

        match req.method.as_str() {
            // ── Инициализация ──
            "initialize" => self.on_initialize(&id, &req.params),
            "ping" => McpResponse::ok(id, json!({})),
            "resources/list" => self.on_resources_list(&id),
            "tools/list" => self.on_tools_list(&id),

            // ── Ресурсы ──
            "resources/read" => self.on_resources_read(&id, &req.params),

            // ── Инструменты ──
            "tools/call" => self.on_tools_call(&id, &req.params),

            _ => McpResponse::err(id, -32601, "Method not found"),
        }
    }

    /// Инициализация сервера. Если сервер создан через [`McpServer::with_token`],
    /// проверяет `params.token` перед тем, как отметить соединение
    /// аутентифицированным.
    fn on_initialize(&mut self, id: &Value, params: &Value) -> McpResponse {
        if let Some(expected) = &self.required_token {
            let provided = params.get("token").and_then(|v| v.as_str()).unwrap_or("");
            if !lumen_core::auth::tokens_match(expected, provided) {
                return McpResponse::err(id.clone(), -32001, "Invalid or missing token");
            }
        }
        self.authenticated = true;

        // Версия протокола: эхо версии клиента, если мы её знаем, иначе
        // наша новейшая (MCP §Lifecycle, version negotiation).
        let requested = params.get("protocolVersion").and_then(|v| v.as_str());
        let protocol_version = match requested {
            Some(v) if SUPPORTED_PROTOCOL_VERSIONS.contains(&v) => v,
            _ => SUPPORTED_PROTOCOL_VERSIONS[0],
        };

        let response = json!({
            "protocolVersion": protocol_version,
            "instructions": SERVER_INSTRUCTIONS,
            "serverInfo": {
                "name": "lumen",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "resources": {
                    "subscribe": false,
                },
                "tools": {},
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
                experimental: false,
            },
            McpResource {
                uri: "resource://a11y_tree".to_string(),
                name: "a11y_tree".to_string(),
                description: "Accessibility tree of the current page".to_string(),
                mime_type: "application/json".to_string(),
                experimental: false,
            },
            McpResource {
                uri: "resource://layout".to_string(),
                name: "layout".to_string(),
                description: "Layout box model snapshot".to_string(),
                mime_type: "application/json".to_string(),
                experimental: false,
            },
            McpResource {
                uri: "resource://console".to_string(),
                name: "console".to_string(),
                description: "Console log entries (log, warn, error)".to_string(),
                mime_type: "application/json".to_string(),
                experimental: false,
            },
            McpResource {
                uri: "resource://network".to_string(),
                name: "network".to_string(),
                description: "Network request log".to_string(),
                mime_type: "application/json".to_string(),
                experimental: false,
            },
            // ADR-024 L1 (`x-` prefix + `experimental: true`, DEVX-14): whole-page
            // display-list text dump, the same serialization `--dump-display-list`
            // prints, mechanically exposed over the wire.
            McpResource {
                uri: "resource://x-display-list".to_string(),
                name: "x-display-list".to_string(),
                description: "Full-page display list text dump (paint commands, stacking order)".to_string(),
                mime_type: "text/plain".to_string(),
                experimental: true,
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
                description: "Navigate the active tab to a URL (file://, http://, https://) and return once the navigation has been started. Returns {success, url}. The page may still be loading: follow with `wait` (document_ready or network_idle) before reading the DOM. Fails with an error for an unparsable URL or a failed load.".to_string(),
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
                experimental: false,
            },
            McpTool {
                name: "new_tab".to_string(),
                description: "Open a new tab, make it the active one and navigate it to a URL. Returns {success, url}. Subsequent tools act on this tab; call `wait` before reading the page.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to load in the new tab"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "list_tabs".to_string(),
                description: "List the open tabs of the live window in strip order. Returns {tabs: [{id, url, title, active}]}. `id` is stable for the session and is what `switch_tab` / `close_tab` take. A popup opened by `window.open()` or a `target=_blank` link shows up here as a new tab; the opener stays the target of the tools until you `switch_tab`.".to_string(),
                input_schema: json!({ "type": "object", "properties": {} }),
                experimental: false,
            },
            McpTool {
                name: "switch_tab".to_string(),
                description: "Make a tab the active one and the target of every following tool (`navigate`, `click`, `eval`, `query`, `wait`, ...). Returns {success, id}. Errors if no tab has that id.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": {
                            "type": "integer",
                            "description": "Tab id from `list_tabs`"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "close_tab".to_string(),
                description: "Close a tab. Returns {success, id}. The tab that takes the foreground (the opener of a closed popup) becomes the target. Errors if no tab has that id or it is the last tab (closing it would exit the browser).".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": {
                            "type": "integer",
                            "description": "Tab id from `list_tabs`"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "click".to_string(),
                description: "Click an element or a point. `target` is a CSS selector string, {selector}, {node_id} (from `query`) or {point: {x, y}} in document coordinates. Returns {success: true}. Errors if the selector matches nothing or the target is not in the layout (e.g. display:none) — `wait` for `visible` first.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target"],
                    "properties": {
                        "target": {
                            "type": ["string", "object"],
                            "description": "Click target: a CSS-selector string,                                             {selector}, {node_id} or {point: {x, y}}"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "type".to_string(),
                description: "Type text into an input or textarea: focuses the target and sends the characters. `target` has the same forms as for `click`. Returns {success, text}. Errors if the target does not exist; does not clear the existing value.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target", "text"],
                    "properties": {
                        "target": {
                            "type": ["string", "object"],
                            "description": "Target element: a CSS-selector string,                                             {selector}, {node_id} or {point: {x, y}}"
                        },
                        "text": {
                            "type": "string",
                            "description": "Text to type"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "scroll".to_string(),
                description: "Scroll the page or an element by a delta in logical pixels (positive y scrolls down). `target` has the same forms as for `click`. Returns {success, delta}. Content added by scrolling (lazy loading) appears asynchronously: `wait` afterwards.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target", "delta"],
                    "properties": {
                        "target": {
                            "type": ["string", "object"],
                            "description": "Scroll target: a CSS-selector string,                                             {selector}, {node_id} or {point: {x, y}}"
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
                experimental: false,
            },
            McpTool {
                name: "pointer_move".to_string(),
                description: "Move the mouse pointer to a point of the content area without pressing a button: the page receives pointermove/mouseover events and `:hover` applies. Coordinates are CSS px of the content viewport. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["x", "y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "Viewport X, CSS px of the content area"
                        },
                        "y": {
                            "type": "number",
                            "description": "Viewport Y, CSS px of the content area"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "pointer_down".to_string(),
                description: "Press the left mouse button (the only supported button) at a point of the content area (CSS px of the content viewport): runs the press half of a click and arms text-selection drag. Complete the gesture with `pointer_up`. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["x", "y"],
                    "properties": {
                        "x": {
                            "type": "number",
                            "description": "Viewport X, CSS px of the content area"
                        },
                        "y": {
                            "type": "number",
                            "description": "Viewport Y, CSS px of the content area"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "pointer_up".to_string(),
                description: "Release the left mouse button pressed by `pointer_down`: ends the armed text-selection drag, the selection itself stays. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({ "type": "object", "properties": {} }),
                experimental: false,
            },
            McpTool {
                name: "key_down".to_string(),
                description: "Send `keydown` for a non-text key to the focused element without clicking anywhere: `key` is the `KeyboardEvent.key` (`Escape`, `Enter`, `Shift`...), optional `code` is the `KeyboardEvent.code` (`KeyA`, `ShiftLeft`...) — derived from `key` when omitted; the arm recognizes modifiers (`Shift`, `Control`, `Alt`, `Meta`) by `key`. Complete with `key_up`. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "KeyboardEvent.key: Escape, Enter, Shift..."
                        },
                        "code": {
                            "type": "string",
                            "description": "KeyboardEvent.code: KeyA, ShiftLeft...; derived from key by default"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "key_up".to_string(),
                description: "Send `keyup` for a non-text key to the focused element: the counterpart of `key_down`; `key`/`code` have the same meaning, `code` is derived from `key` when omitted. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "KeyboardEvent.key: Escape, Enter, Shift..."
                        },
                        "code": {
                            "type": "string",
                            "description": "KeyboardEvent.code: KeyA, ShiftLeft...; derived from key by default"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "type_focused".to_string(),
                description: "Type text into the currently focused element without clicking first (unlike `type`): the page receives per-character key/input events. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["text"],
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to type into the focused element"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "set_files".to_string(),
                description: "Set files on an <input type=file> element (`target` has the same forms as for `click`) without the OS file dialog: every path must exist on the machine running the browser. The page gets a real FileList with readable content plus input/change events, and the `value` attribute becomes the first file's name; the paths themselves never reach the page. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target", "paths"],
                    "properties": {
                        "target": {
                            "type": ["string", "object"],
                            "description": "Target element: a CSS-selector string,                                             {selector}, {node_id} or {point: {x, y}}"
                        },
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Absolute filesystem paths of the files to set"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "user_prompt".to_string(),
                description: "Read the JS dialog currently open in the live window (alert / confirm / prompt): {open, type, message} plus default_value for a prompt. While the dialog is open the page's JS thread is blocked — eval answers busy until you handle it. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
                experimental: false,
            },
            McpTool {
                name: "handle_user_prompt".to_string(),
                description: "Answer the JS dialog currently open in the live window: accept = OK / cancel, user_text replaces the text typed into a prompt. Errors when no dialog is open. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "accept": {
                            "type": "boolean",
                            "description": "true = OK, false = cancel (default true)"
                        },
                        "user_text": {
                            "type": "string",
                            "description": "Replacement text for an open prompt dialog"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "list_downloads".to_string(),
                description: "List the downloads of the live window's download manager: {downloads: [{id, url, file_name, state, received_bytes, total_bytes, path}]}, state is pending / in_progress / completed / failed / cancelled. Terminal events are picked up on the call itself, so a download that just finished is reported completed right away. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({ "type": "object", "properties": {} }),
                experimental: false,
            },
            McpTool {
                name: "start_download".to_string(),
                description: "Start downloading `url` into the OS Downloads directory — the same path a user-initiated download takes. Returns {success, id}; poll `list_downloads` until `state` is completed (the entry's `path` then holds where the file was written). Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to download"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "cancel_download".to_string(),
                description: "Cancel the download with this `id` (from `start_download` or `list_downloads`): the entry settles into state `cancelled`. Errors when no download has that id. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": {
                            "type": "integer",
                            "description": "Download id"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "wait".to_string(),
                description: "Block until a condition holds: document_ready (document parsed and load finished), network_idle (no requests in flight), js_idle (no pending JS tasks), visible / stable (need `selector`: element has a box / its box stopped moving). Returns {success, condition}; errors on timeout (`timeout_ms`, default 30000). Typical order: navigate, then wait document_ready or network_idle, then query / eval.".to_string(),
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
                experimental: false,
            },
            McpTool {
                name: "eval".to_string(),
                description: "Run JavaScript in the page and return {success, result}, where `result` is a string. Evaluate an expression and serialize structured data yourself, e.g. `JSON.stringify(...)`, then parse it. Errors carry the JS exception text. In the live window a busy engine thread makes the call fail after `timeout_ms` (default 5000).".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["code"],
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "JavaScript code to execute"
                        },
                        "timeout_ms": {
                            "type": "integer",
                            "description": "Live window only: how long to wait for a busy engine thread before reporting it busy (default 5000)"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "query".to_string(),
                description: "Find all DOM elements matching a CSS selector. Returns {nodes: [{node_id, tag_name, text_content, bounding_rect}]} (empty array when nothing matches; rect in document coordinates). Use node_id as `target` of click/type. Run after `wait`, otherwise the DOM may be incomplete.".to_string(),
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
                experimental: false,
            },
            McpTool {
                name: "set_offline".to_string(),
                description: "Cut or restore the network: while offline every later request (navigation, `fetch()`/XHR, subresources) fails immediately instead of reaching the network. Returns {success, offline}. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["offline"],
                    "properties": {
                        "offline": {
                            "type": "boolean",
                            "description": "true to go offline, false to restore the network"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "set_user_agent".to_string(),
                description: "Override the User-Agent string: later navigations send it as the HTTP header and `navigator.userAgent` of the current page switches immediately. Returns {success, user_agent}.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["user_agent"],
                    "properties": {
                        "user_agent": {
                            "type": "string",
                            "description": "User-Agent string to use from now on"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "set_timezone".to_string(),
                description: "Override the timezone the page's JS sees: `Intl`/`Date` follow the IANA id (`Asia/Tokyo`, `America/New_York`...). Call without `timezone` (or with null) to restore the host timezone. Returns {success, timezone}. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "timezone": {
                            "type": ["string", "null"],
                            "description": "IANA timezone id; omit or null to restore the host timezone"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "set_permission".to_string(),
                description: "Set a permission state without a prompt: `name` is a permission name (`geolocation`, `camera`, `notifications`...), `state` is `granted`, `denied` or `prompt`. Returns {success, accepted, name, state}; `accepted: false` means the engine does not know the name or the state. The override is global (not per-origin, unlike BiDi's `permissions.setPermission`) and survives navigation. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["name", "state"],
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Permission name: geolocation, camera, notifications..."
                        },
                        "state": {
                            "type": "string",
                            "enum": ["granted", "denied", "prompt"],
                            "description": "State to set"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "computed_a11y".to_string(),
                description: "Resolve a chain of CSS selectors — each selector after the first matches inside the previous match's shadow root — and return the final element's accessibility {role, name}; `null` when the chain matches nothing. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["selectors"],
                    "properties": {
                        "selectors": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "CSS selectors, each matched inside the previous match's shadow root"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "add_intercept".to_string(),
                description: "Pause network requests whose URL matches one of `url_patterns` (exact-string match, empty array = all URLs) at the given phases, until `resolve_intercept` decides each pause: `beforeRequestSent` (before the request goes out) and `responseStarted` (after status line + headers); `authRequired` is not supported. `id` is optional — generated when omitted and returned. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["phases", "url_patterns"],
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Opaque intercept id; generated by the server when omitted"
                        },
                        "phases": {
                            "type": "array",
                            "items": { "type": "string", "enum": ["beforeRequestSent", "responseStarted"] },
                            "description": "Phases to pause matching requests at"
                        },
                        "url_patterns": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "URL patterns to match (exact string); empty array = all URLs"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "remove_intercept".to_string(),
                description: "Remove a previously registered intercept rule by `id` (from `add_intercept`): later requests are no longer paused; requests already paused stay paused until resolved. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "string", "description": "Intercept id from `add_intercept`" }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "resolve_intercept".to_string(),
                description: "Deliver a decision for a request paused by an intercept: `continue: true` lets it proceed unmodified, `continue: false` fails it. Returns `resolved: true` when `request_id` matched an outstanding pause; an unknown id is not an error (`resolved: false`). Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["request_id", "continue"],
                    "properties": {
                        "request_id": { "type": "string", "description": "Request id from `poll_intercepts`" },
                        "continue": { "type": "boolean", "description": "true = let the request proceed, false = fail it" }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "poll_intercepts".to_string(),
                description: "List the requests newly paused by an active intercept since the last call: {requests: [{request_id, url, phase, status}]}. Poll after navigating — a pause holds the request until `resolve_intercept` decides it. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({ "type": "object", "properties": {} }),
                experimental: false,
            },
            McpTool {
                name: "list_cookies".to_string(),
                description: "List every non-expired cookie of the active profile's cookie jar, HttpOnly ones included — what `document.cookie` and later requests see. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({ "type": "object", "properties": {} }),
                experimental: false,
            },
            McpTool {
                name: "set_cookie".to_string(),
                description: "Store (or replace) a cookie in the active profile's cookie jar — pages read it through `document.cookie` and requests send it. `expiry` is a Unix timestamp in seconds, `0` = session cookie; `same_site` is `strict`, `lax`, `none` or `default`. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["name", "value", "domain", "path", "secure", "http_only", "same_site", "expiry"],
                    "properties": {
                        "name": { "type": "string", "description": "Cookie name" },
                        "value": { "type": "string", "description": "Cookie value" },
                        "domain": { "type": "string", "description": "Domain (lowercase host, no leading dot)" },
                        "path": { "type": "string", "description": "Path (/ by default)" },
                        "secure": { "type": "boolean", "description": "Secure flag" },
                        "http_only": { "type": "boolean", "description": "HttpOnly flag" },
                        "same_site": {
                            "type": "string",
                            "enum": ["strict", "lax", "none", "default"],
                            "description": "SameSite attribute (default = attribute absent)"
                        },
                        "expiry": { "type": "integer", "description": "Expiry as a Unix timestamp in seconds; 0 = session cookie" }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "delete_cookie".to_string(),
                description: "Remove the cookie identified by the triple (`domain`, `path`, `name`) from the active profile's cookie jar. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["name", "domain", "path"],
                    "properties": {
                        "name": { "type": "string", "description": "Cookie name" },
                        "domain": { "type": "string", "description": "Cookie domain as stored in the jar" },
                        "path": { "type": "string", "description": "Cookie path" }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "add_preload_script".to_string(),
                description: "Run `function_declaration` (the source of a function, as `script.addPreloadScript` takes it) in every document created from now on, before any of that document's own scripts. `id` is optional — generated when omitted and returned; `remove_preload_script` takes it. Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["function_declaration"],
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Opaque preload-script id; generated by the server when omitted"
                        },
                        "function_declaration": {
                            "type": "string",
                            "description": "Source of a function to call in every new document"
                        }
                    }
                }),
                experimental: false,
            },
            McpTool {
                name: "remove_preload_script".to_string(),
                description: "Stop running a preload script in new documents by `id` (from `add_preload_script`). Live window only; a headless session answers an error.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "string", "description": "Preload-script id from `add_preload_script`" }
                    }
                }),
                experimental: false,
            },
            McpTool {
                // ADR-024 L1 (`x-` prefix + `experimental: true`, DEVX-10):
                // may change shape or vanish between commits. See
                // `docs/tasks/p1-introspection-track.md` §DEVX-10.
                name: "x-explain-element".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): causal chain for the first element \
                    matching a CSS selector — DOM -> style -> layout -> size -> stacking \
                    context -> paint commands -> clip depth -> compositor layer. Includes a \
                    best-effort `heuristic` field for why an element has no visible paint; \
                    that field is a guess, not a fact.".to_string(),
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
                experimental: true,
            },
            McpTool {
                // ADR-024 L1 (`x-` prefix + `experimental: true`, DEVX-11):
                // may change shape or vanish between commits. See
                // `docs/tasks/p1-introspection-track.md` §DEVX-11.
                name: "x-explain-page".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): page-level aggregate — DEVX-8a/8b \
                    invariant-firing counts by category plus telemetry (box counts, anonymous \
                    boxes, overflow elements, display-list commands, max clip depth, relayout \
                    count, timing). Counters are for comparison (diff against a previous run of \
                    the same page, or a corpus profile), not standalone reading.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
                experimental: true,
            },
            McpTool {
                // ADR-024 L1 (`x-` prefix + `experimental: true`, DEVX-12):
                // may change shape or vanish between commits. See
                // `docs/tasks/p1-introspection-track.md` §DEVX-12.
                name: "x-scope-layout".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): box-model of a selector's subtree only \
                    (the matched element plus every descendant), instead of the whole page. \
                    Empty array if the selector matches nothing.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["selector"],
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector" }
                    }
                }),
                experimental: true,
            },
            McpTool {
                name: "x-scope-screenshot".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): PNG screenshot cropped to a selector's \
                    border-box, instead of the whole viewport. Errors if the selector matches \
                    nothing or its box is fully outside the viewport.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["selector"],
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector" }
                    }
                }),
                experimental: true,
            },
            McpTool {
                name: "x-scope-display-list".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): display-list commands attributed (via \
                    DEVX-7 provenance) to a selector's subtree, in original paint order — the \
                    text serialization of just that region instead of the whole page's display \
                    list.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["selector"],
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector" }
                    }
                }),
                experimental: true,
            },
            McpTool {
                name: "x-scope-query".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): find DOM elements by CSS selector, \
                    restricted to the subtree of the first element matching `root_selector` — \
                    Element.querySelectorAll scoping, instead of `query`'s whole-document \
                    search.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["root_selector", "selector"],
                    "properties": {
                        "root_selector": { "type": "string", "description": "CSS selector for the subtree root" },
                        "selector": { "type": "string", "description": "CSS selector to match within the subtree" }
                    }
                }),
                experimental: true,
            },
            McpTool {
                name: "x-scope-relayout".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): re-run layout for a selector's subtree \
                    only, reusing the merged DirtyBits/mark_dirty/lay_out_incremental mechanism \
                    directly instead of a full-page relayout. Recomputes the selector's own style \
                    (not its descendants') before laying out, so a direct `el.style.x = ...` \
                    mutation is picked up. Precondition (not runtime-checked): the DOM structure \
                    under the selector must be unchanged since the last full layout, and any \
                    inherited-property change on the selector must not need to cascade to its \
                    descendants — use a normal mutating tool (full relayout) for those.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["selector"],
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector" }
                    }
                }),
                experimental: true,
            },
            McpTool {
                name: "x-scope-eval".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): like `eval`, but relayouts only the \
                    given selector's subtree afterward (x-scope-relayout) instead of the whole \
                    page — for scripts known to mutate only that subtree.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["selector", "code"],
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector to relayout after eval" },
                        "code": { "type": "string", "description": "JavaScript code to execute" }
                    }
                }),
                experimental: true,
            },
            // ADR-024 L1 (`x-` prefix + `experimental: true`, DEVX-14): computed
            // CSS properties of one element, mechanically exposed over the wire
            // (already available in-process via `BrowserSession::computed_style`,
            // just not previously reachable from an MCP client).
            McpTool {
                name: "x-computed-style".to_string(),
                description: "EXPERIMENTAL (ADR-024 L1): computed CSS properties of the first \
                    element matching `selector` (property name → resolved value). `null` if the \
                    selector matches nothing or the match has no layout box.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "required": ["selector"],
                    "properties": {
                        "selector": { "type": "string", "description": "CSS selector" }
                    }
                }),
                experimental: true,
            },
        ];

        let response = json!({
            "tools": tools
        });

        McpResponse::ok(id.clone(), response)
    }

    /// Чтение ресурса: приводит элементы `contents` к форме спецификации —
    /// `uri` + `text` либо `uri` + `blob` (base64) + `mimeType`.
    fn on_resources_read(&self, id: &Value, params: &Value) -> McpResponse {
        let mut response = self.read_resource(id, params);
        let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(Value::Array(items)) = response
            .result
            .as_mut()
            .and_then(|r| r.get_mut("contents"))
        {
            for item in items {
                let Some(obj) = item.as_object_mut() else { continue };
                obj.remove("type");
                if let Some(data) = obj.remove("data") {
                    obj.insert("blob".to_string(), data);
                }
                obj.insert("uri".to_string(), json!(uri));
            }
        }
        response
    }

    /// Чтение ресурса (внутренняя форма, до приведения к спецификации).
    fn read_resource(&self, id: &Value, params: &Value) -> McpResponse {
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
            "resource://x-display-list" => {
                match self.session.display_list() {
                    Ok(dump) => McpResponse::ok(id.clone(), json!({ "contents": [{ "type": "text", "text": dump, "mimeType": "text/plain" }] })),
                    Err(e) => McpResponse::err(id.clone(), -32603, format!("Display list error: {e}")),
                }
            }
            _ => McpResponse::err(id.clone(), -32602, format!("Unknown resource: {uri}")),
        }
    }

    /// Вызов инструмента. Результат — по спецификации MCP: `content` + `isError`.
    ///
    /// Ошибка выполнения инструмента (код -32603) уходит как `isError: true`
    /// результат, а не JSON-RPC-ошибка; JSON-RPC-ошибкой остаются только
    /// неизвестный инструмент и неверные параметры. Прежние поля результата
    /// (`success`, `url`, `nodes`, …) сохранены рядом с `content` — на них
    /// завязаны скрипты репозитория.
    fn on_tools_call(&mut self, id: &Value, params: &Value) -> McpResponse {
        let raw = self.dispatch_tool(id, params);
        if let Some(err) = &raw.error {
            if err.code != -32603 {
                return raw;
            }
            return McpResponse::ok(
                id.clone(),
                json!({
                    "content": [{ "type": "text", "text": err.message }],
                    "isError": true,
                }),
            );
        }
        let mut result = raw.result.unwrap_or_else(|| json!({}));
        let text = serde_json::to_string(&result).unwrap_or_default();
        let mut content = vec![json!({ "type": "text", "text": text })];
        if let Some(b64) = result.get("png_base64").and_then(|v| v.as_str()) {
            content = vec![json!({ "type": "image", "data": b64, "mimeType": "image/png" })];
        }
        let Some(obj) = result.as_object_mut() else {
            // Результат-не-объект (не бывает у нынешних инструментов) — оборачиваем.
            return McpResponse::ok(id.clone(), json!({ "content": content, "isError": false }));
        };
        obj.insert("content".to_string(), Value::Array(content));
        obj.insert("isError".to_string(), json!(false));
        McpResponse::ok(id.clone(), result)
    }

    /// Диспетчер инструментов (внутренняя форма результата).
    fn dispatch_tool(&mut self, id: &Value, params: &Value) -> McpResponse {
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
            "new_tab" => {
                let url = match args.get("url").and_then(|v| v.as_str()) {
                    Some(u) => u,
                    None => return McpResponse::err(id.clone(), -32602, "Missing url argument"),
                };
                match self.session.new_tab(url) {
                    Ok(()) => json!({ "success": true, "url": url }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("NewTab error: {e}")),
                }
            }
            "list_tabs" => match self.session.list_tabs() {
                Ok(tabs) => json!({
                    "tabs": tabs.iter().map(|t| json!({
                        "id": t.id, "url": t.url, "title": t.title, "active": t.active,
                    })).collect::<Vec<_>>()
                }),
                Err(e) => return McpResponse::err(id.clone(), -32603, format!("ListTabs error: {e}")),
            },
            "switch_tab" => {
                let Some(tab_id) = args.get("id").and_then(|v| v.as_u64()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing id argument");
                };
                match self.session.switch_tab(tab_id as usize) {
                    Ok(()) => json!({ "success": true, "id": tab_id }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("SwitchTab error: {e}")),
                }
            }
            "close_tab" => {
                let Some(tab_id) = args.get("id").and_then(|v| v.as_u64()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing id argument");
                };
                match self.session.close_tab(tab_id as usize) {
                    Ok(()) => json!({ "success": true, "id": tab_id }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("CloseTab error: {e}")),
                }
            }
            "click" => {
                let target_obj = match args.get("target") {
                    Some(t) => t,
                    None => return McpResponse::err(id.clone(), -32602, "Missing target argument"),
                };
                let Some(target) = parse_target(target_obj) else {
                    return McpResponse::err(id.clone(), -32602, TARGET_ERROR);
                };
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
                let Some(target) = parse_target(target_obj) else {
                    return McpResponse::err(id.clone(), -32602, TARGET_ERROR);
                };
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
                let Some(target) = parse_target(target_obj) else {
                    return McpResponse::err(id.clone(), -32602, TARGET_ERROR);
                };

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
                // BUG-1145: без `timeout_ms` — прежний путь с таймаутом живого
                // окна по умолчанию.
                let result = match args.get("timeout_ms").and_then(|v| v.as_u64()) {
                    Some(ms) => self.session.eval_with_timeout(code, ms),
                    None => self.session.eval(code),
                };
                match result {
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
            "pointer_move" => {
                let Some(x) = args.get("x").and_then(|v| v.as_f64()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing x argument");
                };
                let Some(y) = args.get("y").and_then(|v| v.as_f64()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing y argument");
                };
                match self.session.pointer_move(x as f32, y as f32) {
                    Ok(()) => json!({ "success": true }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("PointerMove error: {e}")),
                }
            }
            "pointer_down" => {
                let Some(x) = args.get("x").and_then(|v| v.as_f64()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing x argument");
                };
                let Some(y) = args.get("y").and_then(|v| v.as_f64()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing y argument");
                };
                match self.session.pointer_down(x as f32, y as f32) {
                    Ok(()) => json!({ "success": true }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("PointerDown error: {e}")),
                }
            }
            "pointer_up" => match self.session.pointer_up() {
                Ok(()) => json!({ "success": true }),
                Err(e) => return McpResponse::err(id.clone(), -32603, format!("PointerUp error: {e}")),
            },
            "key_down" | "key_up" => {
                let Some(key) = args.get("key").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing key argument");
                };
                let code = match args.get("code").and_then(|v| v.as_str()) {
                    Some(c) => c.to_string(),
                    None => default_dom_code(key),
                };
                let down = name == "key_down";
                match self.session.press_key(key, &code, down) {
                    Ok(()) => json!({ "success": true, "key": key, "code": code, "down": down }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Key error: {e}")),
                }
            }
            "type_focused" => {
                let Some(text) = args.get("text").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing text argument");
                };
                match self.session.type_focused(text) {
                    Ok(()) => json!({ "success": true, "text": text }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("TypeFocused error: {e}")),
                }
            }
            "set_files" => {
                let target_obj = match args.get("target") {
                    Some(t) => t,
                    None => return McpResponse::err(id.clone(), -32602, "Missing target argument"),
                };
                let Some(target) = parse_target(target_obj) else {
                    return McpResponse::err(id.clone(), -32602, TARGET_ERROR);
                };
                let paths: Vec<String> = args
                    .get("paths")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default();
                if paths.is_empty() {
                    return McpResponse::err(id.clone(), -32602, "paths must be a non-empty array of strings");
                }
                match self.session.set_files(&target, &paths) {
                    Ok(()) => json!({ "success": true, "count": paths.len() }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("SetFiles error: {e}")),
                }
            }
            "user_prompt" => {
                // MCP-TOTAL-S7a: the open dialog blocks the JS thread, so this
                // read never waits for it.
                match self.session.user_prompt() {
                    Ok(Some(d)) => {
                        let mut v = json!({ "open": true, "type": d.kind, "message": d.message });
                        if d.kind == "prompt" {
                            v["default_value"] = json!(d.default_value);
                        }
                        v
                    }
                    Ok(None) => json!({ "open": false }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("UserPrompt error: {e}")),
                }
            }
            "handle_user_prompt" => {
                // MCP-TOTAL-S7a: BiDi semantics — accept defaults to true,
                // user_text only reaches a prompt dialog.
                let accept = args.get("accept").and_then(|v| v.as_bool()).unwrap_or(true);
                let user_text = match args.get("user_text") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.as_str()),
                    Some(_) => {
                        return McpResponse::err(id.clone(), -32602, "user_text must be a string")
                    }
                };
                match self.session.handle_user_prompt(accept, user_text) {
                    Ok(()) => json!({ "success": true, "accept": accept }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("HandleUserPrompt error: {e}")),
                }
            }
            "list_downloads" => {
                match self.session.list_downloads() {
                    Ok(downloads) => json!({ "downloads": downloads }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ListDownloads error: {e}")),
                }
            }
            "start_download" => {
                let Some(url) = args.get("url").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing url argument");
                };
                match self.session.start_download(url) {
                    Ok(download_id) => json!({ "success": true, "id": download_id }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("StartDownload error: {e}")),
                }
            }
            "cancel_download" => {
                let Some(download_id) = args.get("id").and_then(|v| v.as_u64()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing id argument");
                };
                match self.session.cancel_download(download_id as u32) {
                    Ok(()) => json!({ "success": true, "id": download_id }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("CancelDownload error: {e}")),
                }
            }
            "set_offline" => {
                let Some(offline) = args.get("offline").and_then(|v| v.as_bool()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing offline argument");
                };
                match self.session.set_offline(offline) {
                    Ok(()) => json!({ "success": true, "offline": offline }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("SetOffline error: {e}")),
                }
            }
            "set_user_agent" => {
                let Some(ua) = args.get("user_agent").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing user_agent argument");
                };
                match self.session.set_user_agent(ua) {
                    Ok(()) => json!({ "success": true, "user_agent": ua }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("SetUserAgent error: {e}")),
                }
            }
            "set_timezone" => {
                let tz = match args.get("timezone") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(s)) => Some(s.as_str()),
                    Some(_) => {
                        return McpResponse::err(id.clone(), -32602, "timezone must be a string or null")
                    }
                };
                match self.session.set_timezone(tz) {
                    Ok(()) => json!({ "success": true, "timezone": tz }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("SetTimezone error: {e}")),
                }
            }
            "set_permission" => {
                let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing name argument");
                };
                let Some(state) = args.get("state").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing state argument");
                };
                match self.session.set_permission(name, state) {
                    Ok(accepted) => json!({ "success": true, "accepted": accepted, "name": name, "state": state }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("SetPermission error: {e}")),
                }
            }
            "computed_a11y" => {
                let Some(selectors) = args.get("selectors").and_then(|v| v.as_array()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing selectors argument");
                };
                let mut sels = Vec::with_capacity(selectors.len());
                for v in selectors {
                    let Some(s) = v.as_str() else {
                        return McpResponse::err(id.clone(), -32602, "selectors must be an array of strings");
                    };
                    sels.push(s.to_owned());
                }
                match self.session.computed_a11y(&sels) {
                    Ok(Some((role, name))) => json!({ "role": role, "name": name }),
                    Ok(None) => json!(null),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ComputedA11y error: {e}")),
                }
            }
            "add_intercept" => {
                let mut phases = Vec::new();
                match args.get("phases") {
                    None => return McpResponse::err(id.clone(), -32602, "Missing phases argument"),
                    Some(Value::Array(items)) => {
                        for v in items {
                            let Some(s) = v.as_str() else {
                                return McpResponse::err(id.clone(), -32602, "phases must be an array of strings");
                            };
                            // `authRequired` движок не ставит на паузу — та же
                            // честная ошибка, что у BiDi `network.addIntercept`.
                            if s != "beforeRequestSent" && s != "responseStarted" {
                                return McpResponse::err(
                                    id.clone(),
                                    -32602,
                                    format!("Unsupported phase: {s} (supported: beforeRequestSent, responseStarted)"),
                                );
                            }
                            phases.push(s.to_owned());
                        }
                    }
                    Some(_) => return McpResponse::err(id.clone(), -32602, "phases must be an array of strings"),
                }
                let mut url_patterns = Vec::new();
                match args.get("url_patterns") {
                    None => return McpResponse::err(id.clone(), -32602, "Missing url_patterns argument"),
                    Some(Value::Array(items)) => {
                        for v in items {
                            let Some(s) = v.as_str() else {
                                return McpResponse::err(id.clone(), -32602, "url_patterns must be an array of strings");
                            };
                            url_patterns.push(s.to_owned());
                        }
                    }
                    Some(_) => return McpResponse::err(id.clone(), -32602, "url_patterns must be an array of strings"),
                }
                let intercept_id = match args.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s.to_owned(),
                    None => {
                        self.id_counter += 1;
                        format!("intercept-{}", self.id_counter)
                    }
                };
                match self.session.add_intercept(&intercept_id, &phases, &url_patterns) {
                    Ok(()) => json!({ "success": true, "id": intercept_id }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("AddIntercept error: {e}")),
                }
            }
            "remove_intercept" => {
                let Some(intercept_id) = args.get("id").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing id argument");
                };
                match self.session.remove_intercept(intercept_id) {
                    Ok(()) => json!({ "success": true, "id": intercept_id }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("RemoveIntercept error: {e}")),
                }
            }
            "resolve_intercept" => {
                let Some(request_id) = args.get("request_id").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing request_id argument");
                };
                let Some(continue_request) = args.get("continue").and_then(|v| v.as_bool()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing continue argument");
                };
                match self.session.resolve_intercepted_request(request_id, continue_request) {
                    Ok(resolved) => json!({ "success": true, "request_id": request_id, "resolved": resolved }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ResolveIntercept error: {e}")),
                }
            }
            "poll_intercepts" => match self.session.poll_intercepted_requests() {
                Ok(requests) => json!({ "requests": requests }),
                Err(e) => return McpResponse::err(id.clone(), -32603, format!("PollIntercepts error: {e}")),
            },
            "list_cookies" => match self.session.get_cookies() {
                Ok(cookies) => json!({ "cookies": cookies }),
                Err(e) => return McpResponse::err(id.clone(), -32603, format!("ListCookies error: {e}")),
            },
            "set_cookie" => {
                let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing name argument");
                };
                let Some(value) = args.get("value").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing value argument");
                };
                let Some(domain) = args.get("domain").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing domain argument");
                };
                let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing path argument");
                };
                let Some(secure) = args.get("secure").and_then(|v| v.as_bool()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing secure argument");
                };
                let Some(http_only) = args.get("http_only").and_then(|v| v.as_bool()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing http_only argument");
                };
                let Some(same_site) = args.get("same_site").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing same_site argument");
                };
                let Some(expiry) = args.get("expiry").and_then(|v| v.as_u64()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing expiry argument");
                };
                let cookie = lumen_driver::AutomationCookie {
                    name: name.to_owned(),
                    value: value.to_owned(),
                    domain: domain.to_owned(),
                    path: path.to_owned(),
                    secure,
                    http_only,
                    same_site: same_site.to_owned(),
                    expiry,
                };
                match self.session.set_cookie(cookie) {
                    Ok(()) => json!({ "success": true }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("SetCookie error: {e}")),
                }
            }
            "delete_cookie" => {
                let Some(domain) = args.get("domain").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing domain argument");
                };
                let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing path argument");
                };
                let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing name argument");
                };
                match self.session.delete_cookie(domain, path, name) {
                    Ok(()) => json!({ "success": true }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("DeleteCookie error: {e}")),
                }
            }
            "add_preload_script" => {
                let Some(function_declaration) = args.get("function_declaration").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing function_declaration argument");
                };
                let script_id = match args.get("id").and_then(|v| v.as_str()) {
                    Some(s) => s.to_owned(),
                    None => {
                        self.id_counter += 1;
                        format!("preload-{}", self.id_counter)
                    }
                };
                match self.session.add_preload_script(&script_id, function_declaration) {
                    Ok(()) => json!({ "success": true, "id": script_id }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("AddPreloadScript error: {e}")),
                }
            }
            "remove_preload_script" => {
                let Some(script_id) = args.get("id").and_then(|v| v.as_str()) else {
                    return McpResponse::err(id.clone(), -32602, "Missing id argument");
                };
                match self.session.remove_preload_script(script_id) {
                    Ok(()) => json!({ "success": true, "id": script_id }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("RemovePreloadScript error: {e}")),
                }
            }
            "x-explain-element" => {
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                match self.session.explain_element(selector) {
                    Ok(explain) => match serde_json::to_value(&explain) {
                        Ok(v) => v,
                        Err(e) => return McpResponse::err(id.clone(), -32603, format!("Explain serialization error: {e}")),
                    },
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Explain error: {e}")),
                }
            }
            "x-explain-page" => {
                match self.session.explain_page() {
                    Ok(explain) => match serde_json::to_value(&explain) {
                        Ok(v) => v,
                        Err(e) => return McpResponse::err(id.clone(), -32603, format!("Explain serialization error: {e}")),
                    },
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("Explain error: {e}")),
                }
            }
            "x-scope-layout" => {
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                match self.session.layout_snapshot_scoped(selector) {
                    Ok(boxes) => json!({ "boxes": boxes }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ScopeLayout error: {e}")),
                }
            }
            "x-scope-screenshot" => {
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                match self.session.screenshot_scoped(selector) {
                    Ok(png) => json!({ "png_base64": base64_encode(&png) }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ScopeScreenshot error: {e}")),
                }
            }
            "x-scope-display-list" => {
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                match self.session.display_list_scoped(selector) {
                    Ok(dump) => json!({ "display_list": dump }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ScopeDisplayList error: {e}")),
                }
            }
            "x-scope-query" => {
                let root_selector = match args.get("root_selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing root_selector argument"),
                };
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                match self.session.query_scoped(root_selector, selector) {
                    Ok(nodes) => json!({ "nodes": nodes }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ScopeQuery error: {e}")),
                }
            }
            "x-scope-relayout" => {
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                match self.session.relayout_scoped(selector) {
                    Ok(()) => json!({ "success": true }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ScopeRelayout error: {e}")),
                }
            }
            "x-scope-eval" => {
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                let code = match args.get("code").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => return McpResponse::err(id.clone(), -32602, "Missing code argument"),
                };
                match self.session.eval_scoped(selector, code) {
                    Ok(result) => json!({ "success": true, "result": result }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ScopeEval error: {e}")),
                }
            }
            "x-computed-style" => {
                let selector = match args.get("selector").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => return McpResponse::err(id.clone(), -32602, "Missing selector argument"),
                };
                match self.session.computed_style(selector) {
                    Ok(Some(props)) => json!({ "properties": props.properties }),
                    Ok(None) => json!({ "properties": null }),
                    Err(e) => return McpResponse::err(id.clone(), -32603, format!("ComputedStyle error: {e}")),
                }
            }
            _ => {
                return McpResponse::err(id.clone(), -32601, format!("Unknown tool: {name}"));
            }
        };

        McpResponse::ok(id.clone(), result)
    }
}

/// `KeyboardEvent.code` по умолчанию из `key` (схема MCP: `code` необязателен).
/// Одна буква → `KeyA`, цифра → `Digit1`, пробел → `Space`, модификаторы — в
/// левую половину (`Shift` → `ShiftLeft`, как в DOM); у остальных (`Escape`,
/// `Enter`, `Tab`, `ArrowLeft`, `F1`…) `code` совпадает с `key`.
fn default_dom_code(key: &str) -> String {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => {
            return format!("Key{}", c.to_ascii_uppercase());
        }
        (Some(c), None) if c.is_ascii_digit() => return format!("Digit{c}"),
        (Some(' '), None) => return "Space".to_string(),
        _ => {}
    }
    match key {
        "Shift" => "ShiftLeft",
        "Control" => "ControlLeft",
        "Alt" => "AltLeft",
        "Meta" => "MetaLeft",
        _ => key,
    }
    .to_string()
}

/// Parse Target from JSON object.
/// Supports { "selector": "..." }, { "node_id": 123 }, or { "point": { "x": ..., "y": ... } }
fn parse_target(obj: &Value) -> Option<lumen_driver::Target> {
    // Голая строка — селектор. Это первая форма, которую пишет любой клиент
    // (`click {"target": "#b"}`), и до 2026-08-17 она проваливалась во все
    // ветки и молча превращалась в `body`: инструмент отвечал `success: true`,
    // клик уходил в центр страницы, а обработчик кнопки не срабатывал —
    // «клик через MCP ничего не делает» на ровном месте.
    if let Some(selector) = obj.as_str() {
        return Some(lumen_driver::Target::Selector(selector.to_string()));
    }

    if let Some(selector) = obj.get("selector").and_then(|v| v.as_str()) {
        return Some(lumen_driver::Target::Selector(selector.to_string()));
    }

    if let Some(node_id) = obj.get("node_id").and_then(|v| v.as_u64()) {
        return Some(lumen_driver::Target::NodeId(node_id as u32));
    }

    if let Some(point_obj) = obj.get("point").and_then(|v| v.as_object()) {
        let x = point_obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let y = point_obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        return Some(lumen_driver::Target::Point { x, y });
    }

    // Ни одна форма не подошла — это ошибка вызова, а не повод куда-нибудь
    // ткнуть. Прежний запасной `body` делал промах неотличимым от попадания.
    None
}

/// Сообщение об ошибке разбора цели — одно на все инструменты, принимающие
/// `target`, чтобы клиент видел полный список допустимых форм.
const TARGET_ERROR: &str =
    "Invalid target: expected a CSS-selector string, {\"selector\": \"…\"},      {\"node_id\": N} or {\"point\": {\"x\": …, \"y\": …}}";

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::VecTransport;

    /// Минимальная заглушка BrowserSession для тестов MCP сервера.
    struct MockSession;

    impl lumen_driver::BrowserSession for MockSession {
        fn screenshot(&self) -> lumen_core::error::Result<Vec<u8>> {
            Ok(b"PNG".to_vec())
        }

        fn a11y_tree(&self) -> lumen_core::error::Result<lumen_driver::A11yNode> {
            Ok(lumen_driver::A11yNode {
                node_id: 1,
                role: "document".to_string(),
                name: String::new(),
                description: String::new(),
                placeholder: String::new(),
                state: lumen_driver::A11yState::default(),
                bounds: None,
                children: vec![],
            })
        }

        fn query_a11y(&self, _q: &lumen_driver::AxQuery) -> lumen_core::error::Result<Option<lumen_driver::A11yNode>> {
            Ok(None)
        }

        fn query_a11y_all(&self, _q: &lumen_driver::AxQuery) -> lumen_core::error::Result<Vec<lumen_driver::A11yNode>> {
            Ok(vec![])
        }

        fn layout_snapshot(&self) -> lumen_core::error::Result<Vec<lumen_driver::BoxModel>> {
            Ok(vec![])
        }

        fn layout_snapshot_scoped(&self, _sel: &str) -> lumen_core::error::Result<Vec<lumen_driver::BoxModel>> {
            Ok(vec![])
        }

        fn screenshot_scoped(&self, _sel: &str) -> lumen_core::error::Result<Vec<u8>> {
            Ok(b"PNG".to_vec())
        }

        fn display_list(&self) -> lumen_core::error::Result<String> {
            Ok(String::new())
        }

        fn display_list_scoped(&self, _sel: &str) -> lumen_core::error::Result<String> {
            Ok(String::new())
        }

        fn computed_style(&self, _sel: &str) -> lumen_core::error::Result<Option<lumen_driver::ComputedProperties>> {
            Ok(None)
        }

        fn network_log(&self) -> lumen_core::error::Result<Vec<lumen_driver::NetworkEntry>> {
            Ok(vec![])
        }

        fn console_log(&self) -> lumen_core::error::Result<Vec<lumen_driver::ConsoleEntry>> {
            Ok(vec![])
        }

        fn computed_style_snapshot(&self, _sel: &str) -> lumen_core::error::Result<Option<lumen_driver::ComputedStyleSnapshot>> {
            Ok(None)
        }

        fn current_url(&self) -> String {
            "about:blank".to_string()
        }

        fn navigate(&mut self, url: &str) -> lumen_core::error::Result<()> {
            if url.starts_with("fail://") {
                return Err(lumen_core::error::Error::Network("boom".to_string()));
            }
            Ok(())
        }

        fn list_tabs(&self) -> lumen_core::error::Result<Vec<lumen_driver::TabInfo>> {
            Ok(vec![
                lumen_driver::TabInfo { id: 1, url: "https://a.test/".into(), title: "A".into(), active: false },
                lumen_driver::TabInfo { id: 2, url: "https://b.test/popup".into(), title: "B".into(), active: true },
            ])
        }

        fn switch_tab(&mut self, id: usize) -> lumen_core::error::Result<()> {
            if id > 2 {
                return Err(lumen_core::error::Error::Other(format!("нет вкладки {id}")));
            }
            Ok(())
        }

        fn close_tab(&mut self, id: usize) -> lumen_core::error::Result<()> {
            self.switch_tab(id)
        }

        fn click(&mut self, _t: &lumen_driver::Target) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn type_text(&mut self, _t: &lumen_driver::Target, _text: &str) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn scroll(&mut self, _t: &lumen_driver::Target, _d: lumen_driver::ScrollDelta) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn wait(&mut self, _c: lumen_driver::WaitCondition, _ms: u64) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn eval(&mut self, _js: &str) -> lumen_core::error::Result<String> {
            Ok("null".to_string())
        }

        fn eval_with_timeout(&mut self, _js: &str, timeout_ms: u64) -> lumen_core::error::Result<String> {
            Ok(format!("\"waited {timeout_ms}\""))
        }

        fn query(&self, _sel: &str) -> lumen_core::error::Result<Vec<lumen_driver::NodeRef>> {
            Ok(vec![])
        }

        fn query_scoped(&self, _root_sel: &str, _sel: &str) -> lumen_core::error::Result<Vec<lumen_driver::NodeRef>> {
            Ok(vec![])
        }

        fn relayout_scoped(&mut self, _sel: &str) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn eval_scoped(&mut self, _sel: &str, _js: &str) -> lumen_core::error::Result<String> {
            Ok("null".to_string())
        }

        fn layout_box_by_selector(&self, _sel: &str) -> lumen_core::error::Result<Option<lumen_driver::BoxModel>> {
            Ok(None)
        }

        fn all_layout_boxes_by_selector(&self, _sel: &str) -> lumen_core::error::Result<Vec<lumen_driver::BoxModel>> {
            Ok(vec![])
        }

        fn explain_element(&self, _sel: &str) -> lumen_core::error::Result<lumen_driver::ExplainElement> {
            Ok(lumen_driver::ExplainElement::default())
        }

        fn explain_page(&self) -> lumen_core::error::Result<lumen_driver::ExplainPage> {
            Ok(lumen_driver::ExplainPage::default())
        }

        fn fingerprint_profile(&self) -> lumen_driver::FingerprintProfile {
            lumen_driver::FingerprintProfile::Standard
        }

        fn set_fingerprint_profile(&mut self, _p: lumen_driver::FingerprintProfile) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn user_agent(&self) -> String {
            "Lumen/0.1".to_string()
        }

        fn set_user_agent(&mut self, _ua: &str) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn set_clock(&mut self, _mode: lumen_driver::ClockMode) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn set_rng_seed(&mut self, _seed: Option<u64>) -> lumen_core::error::Result<()> {
            Ok(())
        }

        fn freeze_fingerprint(&mut self, _profile: lumen_driver::FingerprintProfile) -> lumen_core::error::Result<()> {
            Ok(())
        }
    }

    fn make_request(method: &str, params: serde_json::Value) -> String {
        serde_json::to_string(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        }))
        .unwrap()
    }

    fn run_one(server: &mut McpServer<MockSession, VecTransport>, req_json: &str) -> McpResponse {
        server.transport.push_incoming(req_json);
        let msg = server.transport.read_message().unwrap();
        match msg {
            McpMessage::Request(req) => server.handle_request(&req),
            _ => panic!("expected Request"),
        }
    }

    #[test]
    fn initialize_returns_instructions_and_tools_are_described() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let resp = run_one(&mut server, &make_request("initialize", serde_json::json!({})));
        let result = resp.result.unwrap();
        assert!(!result["instructions"].as_str().unwrap_or("").is_empty());
        let resp = run_one(&mut server, &make_request("tools/list", serde_json::json!({})));
        let tools = resp.result.unwrap()["tools"].as_array().cloned().unwrap_or_default();
        for t in tools {
            let d = t["description"].as_str().unwrap_or("");
            assert!(d.ends_with('.') || d.contains(". "), "tool {} lacks a full sentence: {d}", t["name"]);
        }
    }

    #[test]
    fn initialize_returns_capabilities() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("initialize", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2025-06-18");
        assert!(result["capabilities"].is_object());
    }

    #[test]
    fn resources_list_returns_six_resources() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("resources/list", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let resources = resp.result.unwrap()["resources"].as_array().cloned().unwrap_or_default();
        assert_eq!(resources.len(), 6);
        let uris: Vec<_> = resources.iter().map(|r| r["uri"].as_str().unwrap_or("")).collect();
        assert!(uris.contains(&"resource://screenshot"));
        assert!(uris.contains(&"resource://a11y_tree"));
        assert!(uris.contains(&"resource://layout"));
        assert!(uris.contains(&"resource://console"));
        assert!(uris.contains(&"resource://network"));
        assert!(uris.contains(&"resource://x-display-list"));
    }

    /// ADR-024 L1: `resource://x-display-list` must carry `"experimental": true`
    /// in `resources/list`; the pre-ADR-024 resources must NOT carry the key at
    /// all (`skip_serializing_if`) — same convention as the tools-list test below.
    #[test]
    fn resources_list_marks_only_x_prefixed_resources_as_experimental() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("resources/list", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        let resources = resp.result.unwrap()["resources"].as_array().cloned().unwrap_or_default();
        for resource in &resources {
            let uri = resource["uri"].as_str().unwrap_or("");
            if uri.starts_with("resource://x-") {
                assert_eq!(resource["experimental"], true, "{uri} must be marked experimental");
            } else {
                assert!(
                    resource.get("experimental").is_none(),
                    "{uri} must not carry an experimental key (ADR-024: legacy resources keep their original shape)"
                );
            }
        }
    }

    #[test]
    fn tools_list_returns_forty_six_tools() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("tools/list", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let tools = resp.result.unwrap()["tools"].as_array().cloned().unwrap_or_default();
        assert_eq!(tools.len(), 46);
        let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap_or("")).collect();
        assert!(names.contains(&"navigate"));
        assert!(names.contains(&"new_tab"));
        assert!(names.contains(&"list_tabs"));
        assert!(names.contains(&"switch_tab"));
        assert!(names.contains(&"close_tab"));
        assert!(names.contains(&"click"));
        assert!(names.contains(&"type"));
        assert!(names.contains(&"scroll"));
        assert!(names.contains(&"set_files"));
        assert!(names.contains(&"pointer_move"));
        assert!(names.contains(&"pointer_down"));
        assert!(names.contains(&"pointer_up"));
        assert!(names.contains(&"key_down"));
        assert!(names.contains(&"key_up"));
        assert!(names.contains(&"type_focused"));
        assert!(names.contains(&"user_prompt"));
        assert!(names.contains(&"handle_user_prompt"));
        assert!(names.contains(&"list_downloads"));
        assert!(names.contains(&"start_download"));
        assert!(names.contains(&"cancel_download"));
        assert!(names.contains(&"set_offline"));
        assert!(names.contains(&"set_user_agent"));
        assert!(names.contains(&"set_timezone"));
        assert!(names.contains(&"set_permission"));
        assert!(names.contains(&"computed_a11y"));
        assert!(names.contains(&"wait"));
        assert!(names.contains(&"eval"));
        assert!(names.contains(&"query"));
        assert!(names.contains(&"add_intercept"));
        assert!(names.contains(&"remove_intercept"));
        assert!(names.contains(&"resolve_intercept"));
        assert!(names.contains(&"poll_intercepts"));
        assert!(names.contains(&"list_cookies"));
        assert!(names.contains(&"set_cookie"));
        assert!(names.contains(&"delete_cookie"));
        assert!(names.contains(&"add_preload_script"));
        assert!(names.contains(&"remove_preload_script"));
        assert!(names.contains(&"x-explain-element"));
        assert!(names.contains(&"x-explain-page"));
        assert!(names.contains(&"x-scope-layout"));
        assert!(names.contains(&"x-scope-screenshot"));
        assert!(names.contains(&"x-scope-display-list"));
        assert!(names.contains(&"x-scope-query"));
        assert!(names.contains(&"x-scope-relayout"));
        assert!(names.contains(&"x-scope-eval"));
        assert!(names.contains(&"x-computed-style"));
    }

    /// ADR-024 L1: `x-explain-element`/`x-explain-page` must carry
    /// `"experimental": true` in `tools/list`; the pre-ADR-024 tools must NOT
    /// carry the key at all (`skip_serializing_if`) — their wire shape must
    /// stay exactly as it was before this task.
    #[test]
    fn tools_list_marks_only_x_prefixed_tools_as_experimental() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("tools/list", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        let tools = resp.result.unwrap()["tools"].as_array().cloned().unwrap_or_default();
        for tool in &tools {
            let name = tool["name"].as_str().unwrap_or("");
            if name.starts_with("x-") {
                assert_eq!(tool["experimental"], true, "{name} must be marked experimental");
            } else {
                assert!(
                    tool.get("experimental").is_none(),
                    "{name} must not carry an experimental key (ADR-024: legacy tools keep their original shape)"
                );
            }
        }
    }

    /// Инструменты ввода, файлового input, JS-диалогов и загрузок — только
    /// для живого окна: сессия без живого окна отвечает `isError: true` с
    /// честным текстом дефолта трейта (MCP-TOTAL-S1b, S7a, S7b, S7c), а не
    /// молчаливым успехом.
    #[test]
    fn input_tools_error_without_a_live_window() {
        let calls = [
            ("pointer_move", serde_json::json!({ "x": 10.0, "y": 20.0 })),
            ("pointer_down", serde_json::json!({ "x": 10.0, "y": 20.0 })),
            ("pointer_up", serde_json::json!({})),
            ("key_down", serde_json::json!({ "key": "Escape" })),
            ("key_up", serde_json::json!({ "key": "Escape" })),
            ("type_focused", serde_json::json!({ "text": "hi" })),
            // MCP-TOTAL-S7b: файловый input — тоже только живого окна.
            ("set_files", serde_json::json!({ "target": "#f", "paths": ["C:/tmp/a.txt"] })),
            // MCP-TOTAL-S7a: JS-диалоги — тоже только живого окна.
            ("user_prompt", serde_json::json!({})),
            ("handle_user_prompt", serde_json::json!({ "accept": true })),
            // MCP-TOTAL-S7c: загрузки — тоже только живого окна.
            ("list_downloads", serde_json::json!({})),
            ("start_download", serde_json::json!({ "url": "file:///tmp/a.bin" })),
            ("cancel_download", serde_json::json!({ "id": 1 })),
        ];
        for (name, args) in calls {
            let mut server = McpServer::new(MockSession, VecTransport::new());
            let req = make_request("tools/call", serde_json::json!({ "name": name, "arguments": args }));
            let resp = run_one(&mut server, &req);
            assert!(resp.error.is_none(), "{name}");
            let result = resp.result.unwrap_or_else(|| panic!("{name} must answer"));
            assert_eq!(result["isError"], true, "{name}");
            let text = result["content"][0]["text"].as_str().unwrap_or("");
            assert!(text.contains("только для живого окна"), "{name}: {text}");
        }
    }

    /// Четыре инструмента эмуляции — только для живого окна (Err-дефолты
    /// трейта из MCP-TOTAL-S1a): сессия без живого окна отвечает
    /// `isError: true` с текстом дефолта, не молчаливым успехом;
    /// `set_user_agent` в headless-сессии работает (InProcessSession его
    /// реализует) — проверяем и это.
    #[test]
    fn emulation_tools_error_without_a_live_window() {
        let calls = [
            ("set_offline", serde_json::json!({ "offline": true })),
            ("set_timezone", serde_json::json!({ "timezone": "Asia/Tokyo" })),
            ("set_permission", serde_json::json!({ "name": "geolocation", "state": "granted" })),
            ("computed_a11y", serde_json::json!({ "selectors": ["button"] })),
        ];
        for (name, args) in calls {
            let mut server = McpServer::new(MockSession, VecTransport::new());
            let req = make_request("tools/call", serde_json::json!({ "name": name, "arguments": args }));
            let resp = run_one(&mut server, &req);
            assert!(resp.error.is_none(), "{name}");
            let result = resp.result.unwrap_or_else(|| panic!("{name} must answer"));
            assert_eq!(result["isError"], true, "{name}");
            let text = result["content"][0]["text"].as_str().unwrap_or("");
            assert!(text.contains("только для живого окна"), "{name}: {text}");
        }
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "set_user_agent", "arguments": { "user_agent": "Probe/1.0" } }),
        );
        let resp = run_one(&mut server, &req);
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["user_agent"], "Probe/1.0");
    }

    /// Девять инструментов перехвата, cookies и preload — только для живого
    /// окна (Err-дефолты трейта из MCP-TOTAL-S1a): сессия без живого окна
    /// отвечает `isError: true` с текстом дефолта, не молчаливым успехом
    /// (MCP-TOTAL-S1d).
    #[test]
    fn intercept_tools_error_without_a_live_window() {
        let calls = [
            ("add_intercept", serde_json::json!({ "phases": ["beforeRequestSent"], "url_patterns": ["http://localhost/"] })),
            ("remove_intercept", serde_json::json!({ "id": "intercept-1" })),
            ("resolve_intercept", serde_json::json!({ "request_id": "req-1", "continue": true })),
            ("poll_intercepts", serde_json::json!({})),
            ("list_cookies", serde_json::json!({})),
            ("set_cookie", serde_json::json!({
                "name": "k", "value": "v", "domain": "localhost", "path": "/",
                "secure": false, "http_only": false, "same_site": "lax", "expiry": 0
            })),
            ("delete_cookie", serde_json::json!({ "name": "k", "domain": "localhost", "path": "/" })),
            ("add_preload_script", serde_json::json!({ "function_declaration": "() => {}" })),
            ("remove_preload_script", serde_json::json!({ "id": "preload-1" })),
        ];
        for (name, args) in calls {
            let mut server = McpServer::new(MockSession, VecTransport::new());
            let req = make_request("tools/call", serde_json::json!({ "name": name, "arguments": args }));
            let resp = run_one(&mut server, &req);
            assert!(resp.error.is_none(), "{name}");
            let result = resp.result.unwrap_or_else(|| panic!("{name} must answer"));
            assert_eq!(result["isError"], true, "{name}");
            let text = result["content"][0]["text"].as_str().unwrap_or("");
            assert!(text.contains("только для живого окна"), "{name}: {text}");
        }
    }

    /// Неизвестная фаза перехвата (`authRequired` движок не ставит на паузу —
    /// как у BiDi `network.addIntercept`) и чужой тип аргумента — `-32602`
    /// до вызова сессии, не `-32603` (MCP-TOTAL-S1d).
    #[test]
    fn add_intercept_rejects_unsupported_phase_and_wrong_types() {
        let calls = [
            (
                serde_json::json!({ "phases": ["authRequired"], "url_patterns": [] }),
                "Unsupported phase",
            ),
            (
                serde_json::json!({ "phases": ["beforeRequestSent", 42], "url_patterns": [] }),
                "array of strings",
            ),
            (
                serde_json::json!({ "phases": ["beforeRequestSent"], "url_patterns": "http://x/" }),
                "array of strings",
            ),
        ];
        for (args, expected) in calls {
            let mut server = McpServer::new(MockSession, VecTransport::new());
            let req = make_request("tools/call", serde_json::json!({ "name": "add_intercept", "arguments": args }));
            let resp = run_one(&mut server, &req);
            let err = resp.error.unwrap_or_else(|| panic!("{args} must be a -32602 error"));
            assert_eq!(err.code, -32602, "{args}");
            assert!(err.message.contains(expected), "{args}: {}", err.message);
        }
    }

    /// Схема MCP: `code` у `key_down`/`key_up` необязателен и выводится из
    /// `key` — буквы/цифры/пробел по DOM-правилу, модификаторы в левую
    /// половину, у именованных клавиш `code` совпадает с `key`.
    #[test]
    fn default_dom_code_follows_dom_rules() {
        assert_eq!(super::default_dom_code("a"), "KeyA");
        assert_eq!(super::default_dom_code("A"), "KeyA");
        assert_eq!(super::default_dom_code("1"), "Digit1");
        assert_eq!(super::default_dom_code(" "), "Space");
        assert_eq!(super::default_dom_code("Shift"), "ShiftLeft");
        assert_eq!(super::default_dom_code("Control"), "ControlLeft");
        assert_eq!(super::default_dom_code("Alt"), "AltLeft");
        assert_eq!(super::default_dom_code("Meta"), "MetaLeft");
        assert_eq!(super::default_dom_code("Escape"), "Escape");
        assert_eq!(super::default_dom_code("Enter"), "Enter");
        assert_eq!(super::default_dom_code("ArrowLeft"), "ArrowLeft");
        assert_eq!(super::default_dom_code("F5"), "F5");
    }

    #[test]
    fn tools_call_explain_element_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-explain-element", "arguments": { "selector": "#missing" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["in_dom"], false);
    }

    #[test]
    fn tools_call_explain_element_missing_selector_errors() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-explain-element", "arguments": {} }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
    }

    #[test]
    fn tools_call_explain_page_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-explain-page", "arguments": {} }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["box_count"], 0);
    }

    #[test]
    fn tools_call_scope_layout_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-scope-layout", "arguments": { "selector": "#box" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        assert!(resp.result.unwrap()["boxes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn tools_call_scope_layout_missing_selector_errors() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("tools/call", serde_json::json!({ "name": "x-scope-layout", "arguments": {} }));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
    }

    #[test]
    fn tools_call_scope_query_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-scope-query", "arguments": { "root_selector": "#a", "selector": "p" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        assert!(resp.result.unwrap()["nodes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn tools_call_scope_query_missing_root_selector_errors() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-scope-query", "arguments": { "selector": "p" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
    }

    #[test]
    fn tools_call_scope_relayout_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-scope-relayout", "arguments": { "selector": "#box" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["success"], true);
    }

    #[test]
    fn tools_call_scope_eval_missing_code_errors() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-scope-eval", "arguments": { "selector": "#box" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
    }

    #[test]
    fn tools_call_computed_style_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-computed-style", "arguments": { "selector": "#missing" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        assert!(resp.result.unwrap()["properties"].is_null());
    }

    #[test]
    fn tools_call_computed_style_missing_selector_errors() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "x-computed-style", "arguments": {} }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
    }

    #[test]
    fn resources_read_screenshot_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "resources/read",
            serde_json::json!({ "uri": "resource://screenshot" }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let contents = &resp.result.unwrap()["contents"];
        assert_eq!(contents[0]["uri"], "resource://screenshot");
        assert_eq!(contents[0]["mimeType"], "image/png");
        // "UE5H" is base64("PNG")
        assert_eq!(contents[0]["blob"], "UE5H");
    }

    #[test]
    fn resources_read_a11y_tree_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "resources/read",
            serde_json::json!({ "uri": "resource://a11y_tree" }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let text = resp.result.unwrap()["contents"][0]["text"].as_str().unwrap().to_string();
        assert!(text.contains("document"));
    }

    #[test]
    fn resources_read_x_display_list_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "resources/read",
            serde_json::json!({ "uri": "resource://x-display-list" }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let contents = &resp.result.unwrap()["contents"];
        assert_eq!(contents[0]["mimeType"], "text/plain");
    }

    #[test]
    fn resources_read_unknown_uri_errors() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "resources/read",
            serde_json::json!({ "uri": "resource://nonexistent" }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    /// Разбор цели инструментов `click`/`type`/`scroll`.
    ///
    /// Главная строка здесь — предпоследняя: форма, которую разобрать нельзя,
    /// обязана давать `None`. До 2026-08-17 на её месте стоял запасной
    /// `Selector("body")`, и промах по цели выглядел как успешный клик в центр
    /// страницы: инструмент отвечал `success: true`, обработчик кнопки не
    /// срабатывал, и дефект уезжал в движок, которого там не было.
    #[test]
    fn parse_target_forms() {
        use lumen_driver::Target;
        assert!(
            matches!(parse_target(&serde_json::json!("#b")), Some(Target::Selector(s)) if s == "#b"),
            "голая строка — самая частая форма вызова"
        );
        assert!(matches!(
            parse_target(&serde_json::json!({ "selector": "#b" })),
            Some(Target::Selector(s)) if s == "#b"
        ));
        assert!(matches!(
            parse_target(&serde_json::json!({ "node_id": 7 })),
            Some(Target::NodeId(7))
        ));
        assert!(matches!(
            parse_target(&serde_json::json!({ "point": { "x": 3.0, "y": 4.0 } })),
            Some(Target::Point { x, y }) if x == 3.0 && y == 4.0
        ));
        assert!(parse_target(&serde_json::json!({ "selektor": "#b" })).is_none());
        assert!(parse_target(&serde_json::json!({})).is_none());
        assert!(parse_target(&serde_json::json!(42)).is_none());
    }

    /// Тот же гейт на уровне протокола: нераспознанная цель — ошибка вызова,
    /// а не «успешный» клик неизвестно куда.
    #[test]
    fn tool_click_unrecognized_target_errors() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "click", "arguments": { "target": { "sel": "#b" } } }),
        );
        let resp = run_one(&mut server, &req);
        assert_eq!(resp.error.expect("нераспознанная цель — ошибка").code, -32602);
    }

    /// Строковая цель доезжает до сессии (у `MockSession` клик всегда успешен —
    /// проверяется именно то, что вызов не отбит разбором).
    #[test]
    fn tool_click_accepts_bare_selector_string() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "click", "arguments": { "target": "#b" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["success"], true);
    }

    #[test]
    fn tool_navigate_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "navigate", "arguments": { "url": "https://example.com" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["success"], true);
    }

    /// BUG-1246: полное рукопожатие стандартного MCP-клиента.
    #[test]
    fn spec_handshake_initialize_ping_list_call() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let init = make_request(
            "initialize",
            serde_json::json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
        );
        let result = run_one(&mut server, &init).result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "lumen");
        assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
        assert!(result["capabilities"].get("sampling").is_none());
        assert!(result.get("serverVersion").is_none());

        // Уведомление и ping, затем tools/list — за один проход run().
        server.transport.push_incoming(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        server.transport.push_incoming(&make_request("ping", serde_json::json!({})));
        server.transport.push_incoming(&make_request("tools/list", serde_json::json!({})));
        let _ = server.run(); // завершится ошибкой пустого транспорта
        let out = server.transport.take_outgoing();
        assert_eq!(out.len(), 2, "на уведомление ответа быть не должно: {out:?}");
        let ping: serde_json::Value = serde_json::from_str(&out[0]).unwrap();
        assert_eq!(ping["result"], serde_json::json!({}));
        let list: serde_json::Value = serde_json::from_str(&out[1]).unwrap();
        for tool in list["result"]["tools"].as_array().unwrap() {
            assert!(tool["inputSchema"].is_object(), "{tool}");
            assert!(tool.get("input_schema").is_none());
        }

        let res = run_one(
            &mut server,
            &make_request("resources/list", serde_json::json!({})),
        )
        .result
        .unwrap();
        assert!(res["resources"][0]["mimeType"].is_string());
    }

    #[test]
    fn initialize_unknown_protocol_version_falls_back_to_latest() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("initialize", serde_json::json!({ "protocolVersion": "1999-01-01" }));
        let result = run_one(&mut server, &req).result.unwrap();
        assert_eq!(result["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn tools_call_result_has_content_and_is_error_false() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "eval", "arguments": { "code": "1" } }),
        );
        let result = run_one(&mut server, &req).result.unwrap();
        assert_eq!(result["content"][0]["type"], "text");
        assert_eq!(result["isError"], false);
        // прежние поля остаются для скриптов репозитория
        assert_eq!(result["success"], true);
    }

    #[test]
    fn tools_call_execution_failure_is_is_error_result_not_rpc_error() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "navigate", "arguments": { "url": "fail://x" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"][0]["type"], "text");
        assert!(result["content"][0]["text"].as_str().unwrap().contains("boom"));
    }

    #[test]
    fn tool_list_tabs_returns_id_url_title_active() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("tools/call", serde_json::json!({ "name": "list_tabs", "arguments": {} }));
        let result = run_one(&mut server, &req).result.unwrap();
        let tabs = result["tabs"].as_array().unwrap();
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[1]["id"], 2);
        assert_eq!(tabs[1]["url"], "https://b.test/popup");
        assert_eq!(tabs[1]["title"], "B");
        assert_eq!(tabs[1]["active"], true);
        assert_eq!(tabs[0]["active"], false);
    }

    #[test]
    fn tool_switch_and_close_tab_take_id() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        for name in ["switch_tab", "close_tab"] {
            let ok = make_request("tools/call", serde_json::json!({ "name": name, "arguments": { "id": 2 } }));
            let result = run_one(&mut server, &ok).result.unwrap();
            assert_eq!(result["success"], true, "{name}");
            assert_eq!(result["id"], 2, "{name}");
            let missing = make_request("tools/call", serde_json::json!({ "name": name, "arguments": {} }));
            assert!(run_one(&mut server, &missing).error.is_some(), "{name}: id обязателен");
            let unknown = make_request("tools/call", serde_json::json!({ "name": name, "arguments": { "id": 9 } }));
            let result = run_one(&mut server, &unknown).result.unwrap();
            assert_eq!(result["isError"], true, "{name}: неизвестный id — isError");
        }
    }

    #[test]
    fn tool_navigate_missing_url_errors() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "navigate", "arguments": {} }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
    }

    #[test]
    fn tool_eval_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "eval", "arguments": { "code": "1+1" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["success"], true);
    }

    #[test]
    fn tool_eval_timeout_ms_reaches_session() {
        // BUG-1145: `timeout_ms` уходит в `eval_with_timeout`, а не теряется.
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "eval", "arguments": { "code": "1+1", "timeout_ms": 30000 } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap()["result"], "\"waited 30000\"");
    }

    #[test]
    fn tool_query_ok() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "query", "arguments": { "selector": "div" } }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
        let nodes = resp.result.unwrap()["nodes"].as_array().cloned().unwrap_or_default();
        assert!(nodes.is_empty()); // MockSession returns []
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("foobarbaz", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn unknown_tool_returns_error() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request(
            "tools/call",
            serde_json::json!({ "name": "nonexistent_tool", "arguments": {} }),
        );
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    // ── ADR-024 §Access model (DEVX-15): token auth ──

    #[test]
    fn with_token_rejects_calls_before_initialize() {
        let mut server = McpServer::with_token(MockSession, VecTransport::new(), "secret".to_string());
        let req = make_request("tools/list", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32001);
    }

    #[test]
    fn with_token_rejects_initialize_with_wrong_token() {
        let mut server = McpServer::with_token(MockSession, VecTransport::new(), "secret".to_string());
        let req = make_request("initialize", serde_json::json!({ "token": "wrong" }));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32001);

        // Still unauthenticated afterwards — a failed attempt must not unlock the connection.
        let req2 = make_request("tools/list", serde_json::json!({}));
        let resp2 = run_one(&mut server, &req2);
        assert!(resp2.error.is_some());
    }

    #[test]
    fn with_token_rejects_initialize_with_missing_token() {
        let mut server = McpServer::with_token(MockSession, VecTransport::new(), "secret".to_string());
        let req = make_request("initialize", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32001);
    }

    #[test]
    fn with_token_accepts_initialize_with_correct_token_and_unlocks() {
        let mut server = McpServer::with_token(MockSession, VecTransport::new(), "secret".to_string());
        let req = make_request("initialize", serde_json::json!({ "token": "secret" }));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());

        let req2 = make_request("tools/list", serde_json::json!({}));
        let resp2 = run_one(&mut server, &req2);
        assert!(resp2.error.is_none());
    }

    #[test]
    fn new_without_token_never_gates_requests() {
        let mut server = McpServer::new(MockSession, VecTransport::new());
        let req = make_request("tools/list", serde_json::json!({}));
        let resp = run_one(&mut server, &req);
        assert!(resp.error.is_none());
    }
}
