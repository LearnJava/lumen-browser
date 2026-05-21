# lumen-devtools ✅ (Phase 0 minimal CDP)

## Scope

WebSocket DevTools server with minimal Chrome DevTools Protocol (CDP) support.
Enables browser engine debugging via Chrome DevTools / VSCode / devtools clients.

## Done

- **WebSocket server** (RFC 6455): HTTP Upgrade handshake (`Sec-WebSocket-Accept` = base64(SHA-1)), text frame read/write, Close/Ping/Pong handling. Server→client frames unmasked; client→server frames unmasked or masked.
- **CDP dispatcher** (JSON-RPC style `{"id":N,"method":"D.m","params":{...}}`):
  - `Browser.getVersion` → protocolVersion, product, revision, userAgent, jsVersion
  - `DOM.getDocument` → stub Document node (nodeType=9, nodeName="#document")
  - `Network.enable` / `CSS.enable` / `Page.enable` / `Runtime.enable` → ACK `{}`
  - Unknown methods → error `{"code": -32601, "message": "Method not found: X"}`
  - Invalid JSON → error `{"code": -32700, "message": "Parse error: ..."}`
- **TCP server**: `DevToolsServer::spawn(port)` binds `127.0.0.1:port`, one thread per connection, 30-second read timeout.
- **Shell integration**: `lumen --devtools-port N` starts the server before entering window/dump mode.

## Tests

10 unit tests:
- `ws`: read_unmasked, read_masked, write_frame_format, close_frame, ws_accept_key (RFC 6455 §1.3 example)
- `cdp`: browser_get_version, network_enable, dom_get_document, unknown_method (-32601), invalid_json (-32700)

## Invariants

- SHA-1 / base64 helpers live in `lumen-core::hash` (not here) — reusable protocol primitives.
- `JsonValue::Display` lives in `lumen-core::json` — CDP response serialization uses it.
- No async: blocking std::net I/O with one thread per connection — sufficient for prototype DevTools.
- Binary WebSocket frames → `UnsupportedOpcode` error (CDP uses text only).
- Max frame size: 1 MB guard in `read_raw_frame`.

## Deferred

- DOM.getDocument with real document tree (requires JS/DOM integration).
- CSS.getComputedStyleForNode (requires P1/P2 integration).
- Network.requestWillBeSent / Network.responseReceived events (requires EventSink integration).
- Debugger domain (requires JS engine hooks, Phase 2).
- TLS/WSS support (devtools is localhost-only, no need in Phase 0).
